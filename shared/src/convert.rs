use std::str::from_utf8;

use anyhow::{bail, Result};
use log::*;
use midly::{MetaMessage, MidiMessage, Smf, TrackEventKind};

use crate::*;

type MidlyTempo = midly::num::u24; // MetaMessage::Tempo
type MidlyTimeSig = (u8, u8, u8, u8); // MetaMessage::TimeSignature
#[derive(Default)]
struct MidiTiming {
    tempo: Option<MidlyTempo>,
    timesig: Option<MidlyTimeSig>,
    result: Option<f64>,
    accumilated_inaccuracy: f64,
    stretch: bool,
}
impl MidiTiming {
    fn new(stretch: bool) -> Self {
        Self {
            stretch,
            ..Default::default()
        }
    }
    fn calculate(&mut self) {
        let (timesig_num, timesig_denom, timesig_ticks, _) = self.timesig.unwrap();
        let midi_bpm = 60000000.0 / (self.tempo.unwrap().as_int() as f64);
        let (opti_bpm, tick_wait) = optimal_bpm(midi_bpm, timesig_num as i32, timesig_denom as i32);
        let tick_divisor = (timesig_ticks as f64) / (tick_wait as f64); // this is probably not correct...
        info!(
            "MIDI BPM: {} | Optimal WASM-4 BPM: {} | Optimal WASM-4 tick-wait: {} | Optimal WASM-4 tick-divisor: {}",
            midi_bpm, opti_bpm, tick_wait, tick_divisor
        );
        if self.stretch {
            self.result = Some(tick_divisor);
        } else {
            let tick_wait = 3600.0 / (midi_bpm * (timesig_num as f64));
            let tick_divisor = (timesig_ticks as f64) / tick_wait; // this is probably not correct...

            info!(
                "Disregarding optimal, using: WASM-4 tick-wait: {} | WASM-4 tick-divisor: {}",
                tick_wait, tick_divisor
            );
            self.result = Some(tick_divisor);
        }
    }
    fn get_w4_ticks(&mut self, midi_ticks: usize) -> Result<usize> {
        if let Some(tick_divisor) = &self.result {
            let w4tick_f = (midi_ticks as f64) / tick_divisor;
            self.accumilated_inaccuracy += (w4tick_f - w4tick_f.round()).abs();
            Ok(w4tick_f.round() as usize)
        } else if midi_ticks == 0 {
            Ok(0)
        } else {
            bail!("midi tempo or timesig missing");
        }
    }
    fn set_tempo(&mut self, tempo: MidlyTempo) -> Result<()> {
        if let Some(cur_tempo) = &self.tempo {
            if *cur_tempo != tempo {
                bail!("tempo changes (currently) not allowed");
            }
        }
        self.tempo = Some(tempo);
        if self.timesig.is_some() {
            self.calculate();
        }
        Ok(())
    }
    fn set_timesig(&mut self, timesig: MidlyTimeSig) -> Result<()> {
        if let Some(cur_timesig) = &self.timesig {
            if *cur_timesig != timesig {
                bail!("time signature changes (currently) not allowed");
            }
        }
        self.timesig = Some(timesig);
        if self.tempo.is_some() {
            self.calculate();
        }
        Ok(())
    }
}

fn midi_to_track_events(def: &SongConfig, smf: Smf, stretch: bool) -> Result<Vec<Vec<TrackEvent>>> {
    // Parse all MIDI events and convert into WASM-4 aligned "raw" events
    let mut timing = MidiTiming::new(stretch);
    let mut track_events: [Vec<TrackEvent>; 16] = Default::default();
    let mut last_event_tick: [usize; 16] = Default::default();
    let mut mapper = MidiEventMapper::new();
    let mut event_buffer = Vec::<TrackEvent>::new();
    mapper.set_tracks(def.channels.clone()); // TODO: no clone
    for midi_events in smf.tracks {
        let mut track_name: Option<String> = None;
        let mut midi_ticks: usize = 0;
        for event in midi_events {
            midi_ticks += event.delta.as_int() as usize;
            match event.kind {
                TrackEventKind::Midi { channel, message } => {
                    event_buffer.clear();
                    match message {
                        MidiMessage::NoteOn { key, vel } => {
                            mapper.note_on(&mut event_buffer, channel.as_int(), key.as_int(), vel.as_int());
                        }
                        MidiMessage::NoteOff { key, .. } => {
                            mapper.note_off(&mut event_buffer, channel.as_int(), key.as_int());
                        }
                        MidiMessage::Controller { controller, value } => {
                            // 10 is pan
                            if controller == 10 {
                                mapper.pan(channel.as_int(), value.as_int());
                            }
                        }
                        _ => {}
                    }
                    if !event_buffer.is_empty() {
                        let ch = channel.as_int() as usize;
                        let ticks = timing.get_w4_ticks(midi_ticks).unwrap();
                        if ticks > last_event_tick[ch] {
                            let delta = ticks - last_event_tick[ch];
                            last_event_tick[ch] = ticks;
                            track_events[ch].push(TrackEvent::Delta(delta));
                        }
                        track_events[ch].append(&mut event_buffer);
                    }
                }
                TrackEventKind::Meta(MetaMessage::Tempo(tempo)) => {
                    timing.set_tempo(tempo).unwrap();
                }
                TrackEventKind::Meta(MetaMessage::TimeSignature(a, b, c, d)) => {
                    timing.set_timesig((a, b, c, d)).unwrap();
                }
                TrackEventKind::Meta(MetaMessage::TrackName(name)) => {
                    if let Some(current) = track_name {
                        panic!("midi track name already set (was {current})");
                    }
                    track_name = Some(from_utf8(name).expect("invalid track name utf8").to_owned());
                }
                TrackEventKind::Meta(MetaMessage::EndOfTrack) => {}
                _ => {
                    warn!("Unhandled MIDI event: {:?}", event.kind);
                }
            }
        }
    }
    info!("Inaccuracy: {}", timing.accumilated_inaccuracy,);
    Ok(track_events.into_iter().filter(|t| !t.is_empty()).collect())
}
const PATTERN_CREATE_COST: usize = 3; // cost of using a pattern (u8) + pattern length (u16)

fn collapse_tracks(tracks: Vec<Vec<TrackEvent>>) -> Vec<Vec<TrackEvent>> {
    tracks
        .into_iter()
        .filter(|t| !t.is_empty())
        .map(|t| {
            let mut new_track = Vec::<TrackEvent>::with_capacity(t.len());
            for e in t {
                if let TrackEvent::NotesOff = e {
                    if let Some(pe) = new_track.last_mut() {
                        if let TrackEvent::Delta(d) = pe {
                            *pe = TrackEvent::DeltaNotesOff(*d);
                            continue;
                        }
                    }
                }
                new_track.push(e);
            }
            new_track
        })
        .collect()
}

pub fn convert(conf: &SongConfig, midi_bytes: &[u8], stretch: bool, crunch: bool) -> Result<Vec<u8>> {
    let smf = Smf::parse(midi_bytes)?;

    // Now that everything is loaded, here are the general steps:
    // - Convert all MIDI events into WASM-4 tick-aligned events w4on2 track events
    // - Collapse into convert unique optimizations (e.g. DeltaNotesOff)
    // - Crunch everything into patterns
    // - Serialize into binary data

    // Convert
    let tracks = midi_to_track_events(conf, smf, stretch)?;
    // Collapse
    let tracks = collapse_tracks(tracks);
    // Crunch/create song
    let song = if crunch {
        debug!("Crunching...");
        let (dict, usages) = crunch::crunch(tracks.clone(), W4ON2_MAX_PATTERNS as usize, PATTERN_CREATE_COST);
        assert_eq!(crunch::uncrunch(&dict, &usages), tracks);
        W4PlayerSong {
            patterns: dict,
            tracks: usages,
        }
    } else {
        W4PlayerSong {
            tracks: (0..tracks.len()).map(|i| vec![i]).collect(),
            patterns: tracks,
        }
    };

    // Output
    Ok(song.serialize())
}
