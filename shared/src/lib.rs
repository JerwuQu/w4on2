pub mod bounce;
pub mod convert;
mod crunch;
pub mod runtime;
pub mod wasm4_apu;

use std::{ffi::c_void, fmt::Display};

use anyhow::Result;
use lazy_static::lazy_static;
use runtime::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
pub enum PulseDuty {
    #[serde(rename = "12.5%")]
    D12_5 = 0,
    #[serde(rename = "25%")]
    D25 = 1,
    #[serde(rename = "50%")]
    D50 = 2,
    #[serde(rename = "75%")]
    D75 = 3,
}
impl PulseDuty {
    pub fn types() -> [PulseDuty; 4] {
        [PulseDuty::D12_5, PulseDuty::D25, PulseDuty::D50, PulseDuty::D75]
    }
}
impl Display for PulseDuty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PulseDuty::D12_5 => "12.5%",
            PulseDuty::D25 => "25%",
            PulseDuty::D50 => "50%",
            PulseDuty::D75 => "75%",
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Pulse1(PulseDuty),
    Pulse2(PulseDuty),
    Triangle,
    Noise,
}
impl Channel {
    pub fn to_wasm4_flags(&self) -> u8 {
        match self {
            Channel::Pulse1(d) => (*d as u8) << 2,
            Channel::Pulse2(d) => 1 | ((*d as u8) << 2),
            Channel::Triangle => 2,
            Channel::Noise => 3,
        }
    }
    pub fn types() -> [Channel; 4] {
        [
            Channel::Pulse1(PulseDuty::D12_5),
            Channel::Pulse2(PulseDuty::D12_5),
            Channel::Triangle,
            Channel::Noise,
        ]
    }
}
impl Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Channel::Pulse1(_) => "PULSE_1",
            Channel::Pulse2(_) => "PULSE_2",
            Channel::Triangle => "TRIANGLE",
            Channel::Noise => "NOISE",
        })
    }
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Pan {
    Stereo = 0,
    Left = 1,
    Right = 2,
}

// TODO: add Hold to ADSR
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ADSR(pub u8, pub u8, pub u8, pub u8);

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PitchEnv {
    pub note_offset: i8,
    pub duration: u8,
}

#[derive(Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DelayPingPong {
    #[default]
    No,
    Left,
    Right,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Delay {
    pub ticks: u8,
    pub ramp: u8,
    pub wet: u8,
    #[serde(default)]
    pub ping_pong: DelayPingPong,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Arpeggio {
    pub rate: u8,
    // TODO: extend arpeggio:
    // - ping-pong stereo (kinda cool effect)
    // - gate
    // - direction, though not really needed since we can compose direction manually aside from ping-pong
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Vibrato {
    pub speed: u8,
    pub depth: u8,
    // TODO: extend vibrato:
    // - ramp/delay to progressively increase depth
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "snake_case")]
pub struct SongTrackConfig {
    pub nickname: String,
    pub channel: Channel,
    pub volume: u8,
    pub adsr: ADSR,
    pub pitch_env: PitchEnv,
    pub portamento: u8,
    pub arpeggio: Arpeggio,
    pub vibrato: Vibrato,
    // TODO: delay: Option<Delay>,
}
impl Default for SongTrackConfig {
    fn default() -> Self {
        Self {
            nickname: "".to_owned(),
            channel: Channel::Pulse1(PulseDuty::D12_5),
            volume: W4ON2_VOLUME_MAX as u8,
            adsr: ADSR(0, 0, W4ON2_SUSTAIN_MAX as u8, 0),
            pitch_env: PitchEnv::default(),
            portamento: 0,
            arpeggio: Arpeggio::default(),
            vibrato: Vibrato::default(),
        }
    }
}
lazy_static! {
    pub static ref SONG_TRACK_CONFIG_DEFAULT: SongTrackConfig = SongTrackConfig::default();
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SongConfig {
    pub channels: [SongTrackConfig; 16],
}
impl SongConfig {
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        Ok(toml::from_str::<SongConfig>(toml_str)?)
    }
    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }
}

// NOTE: This enum is what is serialized into w4on2 track events
// The events should correspond to events in `w4on2.h`, but types can be whatever makes most sense.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackEvent {
    Delta(usize),         // Wait/delay for some ticks
    DeltaNotesOff(usize), // Delta(...) followed by a NotesOff - used for more efficient storage by `convert`
    NoteOn(u8), // trigger a note - if gotten before "NotesOff", will act as slide or arpeggio depending on instrument
    NotesOff,   // when all notes on this track have ended
    SetFlags(u8), // channel, pulse, pan
    SetVolume(u8),
    SetPan(Pan),
    SetVelocity(u8),
    SetADSR(ADSR),
    SetA(u8),
    SetD(u8),
    SetS(u8),
    SetR(u8),
    SetPitchEnv(PitchEnv),
    SetArpeggio(Arpeggio),
    SetPortamento(u8),
    SetVibrato(Vibrato),
    //SetDelay(Delay),
}
impl TrackEvent {
    pub fn serialize_into(&self, into: &mut Vec<u8>) {
        match self {
            TrackEvent::SetVolume(v) => into.extend([W4ON2_FMT_SET_VOLUME_ARG1_ID as u8, *v]),
            TrackEvent::SetADSR(ADSR(a, d, s, r)) => into.extend([W4ON2_FMT_SET_ADSR_ARG4_ID as u8, *a, *d, *s, *r]),
            TrackEvent::SetFlags(f) => into.extend([W4ON2_FMT_SET_FLAGS_ARG1_ID as u8, *f]),
            TrackEvent::SetVelocity(v) => into.extend([W4ON2_FMT_SET_VELOCITY_ARG1_ID as u8, *v]),
            TrackEvent::SetPan(p) => {
                into.extend([W4ON2_FMT_SET_PAN_8_START as u8 + *p as u8]);
            }
            TrackEvent::SetArpeggio(arp) => {
                into.extend([W4ON2_FMT_SET_ARP_RATE_ARG1_ID as u8, arp.rate]);
            }
            TrackEvent::SetPortamento(p) => {
                into.extend([W4ON2_FMT_SET_PORTAMENTO_ARG1_ID as u8, *p]);
            }
            TrackEvent::Delta(d) => {
                assert!(*d > 0);
                assert!(*d <= 0xffff);
                if *d <= W4ON2_FMT_SHORT_DELTA_2_COUNT as usize {
                    into.extend([W4ON2_FMT_SHORT_DELTA_2_START as u8 + ((*d) - 1) as u8])
                } else {
                    let buf = ((*d as u16) - W4ON2_FMT_SHORT_DELTA_2_COUNT as u16 - 1).to_be_bytes();
                    into.extend([W4ON2_FMT_LONG_DELTA_ARG2_ID as u8, buf[0], buf[1]]);
                }
            }
            TrackEvent::NoteOn(n) => {
                assert!(*n < W4ON2_FMT_NOTE_ON_4_COUNT as u8);
                into.extend([W4ON2_FMT_NOTE_ON_4_START as u8 + *n])
            }
            TrackEvent::NotesOff => into.extend([W4ON2_FMT_NOTES_OFF_ID as u8]),
            TrackEvent::DeltaNotesOff(d) => {
                assert!(*d > 0);
                assert!(*d <= 0xffff);
                if *d <= W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_COUNT as usize {
                    into.extend([W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_START as u8 + ((*d) - 1) as u8])
                } else {
                    let buf = ((*d as u16) - W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_COUNT as u16 - 1).to_be_bytes();
                    into.extend([W4ON2_FMT_LONG_DELTA_NOTES_OFF_ARG2_ID as u8, buf[0], buf[1]]);
                }
            }
            TrackEvent::SetPitchEnv(pe) => {
                into.extend([W4ON2_FMT_SET_PITCH_ENV_ARG2_ID as u8, pe.note_offset as u8, pe.duration])
            }
            TrackEvent::SetA(a) => into.extend([W4ON2_FMT_SET_A_ARG1_ID as u8, *a]),
            TrackEvent::SetD(d) => into.extend([W4ON2_FMT_SET_D_ARG1_ID as u8, *d]),
            TrackEvent::SetS(s) => into.extend([W4ON2_FMT_SET_S_ARG1_ID as u8, *s]),
            TrackEvent::SetR(r) => into.extend([W4ON2_FMT_SET_R_ARG1_ID as u8, *r]),
            TrackEvent::SetVibrato(v) => into.extend([W4ON2_FMT_SET_VIBRATO_ARG2_ID as u8, v.speed, v.depth]),
        };
    }
}

pub fn optimal_bpm(midi_bpm: f64, timesig_num: i32, timesig_denom: i32) -> (f64, usize) {
    if midi_bpm == 0.0 || timesig_num == 0 || timesig_denom == 0 {
        (midi_bpm, 1) // can't convert - TODO: warn probably
    } else {
        let tick_wait = (3600.0 / (midi_bpm * (timesig_num as f64))).round() as usize;
        let rounded_bpm = 3600.0 / ((tick_wait * (timesig_num as usize)) as f64);
        (rounded_bpm, tick_wait)
    }
}

// Struct that gets serialized into a complete w4on2 song
pub struct W4PlayerSong {
    pub patterns: Vec<Vec<TrackEvent>>,
    pub tracks: Vec<Vec<usize>>, // indices into patterns
}
impl W4PlayerSong {
    pub fn serialize(&self) -> Vec<u8> {
        // init with total size to be replaced
        let mut out: Vec<u8> = vec![0, 0];
        // pattern/track counts
        assert!(self.patterns.len() <= W4ON2_MAX_PATTERNS as usize);
        out.push(self.patterns.len() as u8);
        assert!(self.tracks.len() <= W4ON2_TRACK_COUNT as usize);
        out.push(self.tracks.len() as u8);
        // offset placeholders
        let mut pattern_offset_is = vec![0; self.patterns.len()];
        for ix in &mut pattern_offset_is {
            *ix = out.len();
            out.extend([0, 0]);
        }
        let mut track_offset_is = vec![0; self.tracks.len()];
        for ix in &mut track_offset_is {
            *ix = out.len();
            out.extend([0, 0]);
        }
        // insert
        for (i, p) in self.patterns.iter().enumerate() {
            assert!(out.len() <= 0xffff);
            out.splice(
                pattern_offset_is[i]..pattern_offset_is[i] + 2,
                (out.len() as u16).to_be_bytes(),
            ); // replace offset
            for e in p {
                e.serialize_into(&mut out);
            }
        }
        for (i, t) in self.tracks.iter().enumerate() {
            assert!(out.len() <= 0xffff);
            out.splice(
                track_offset_is[i]..track_offset_is[i] + 2,
                (out.len() as u16).to_be_bytes(),
            ); // replace offset
            for ptn in t {
                assert!(*ptn <= 255);
                out.push(*ptn as u8);
            }
        }
        // replace start size
        assert!(out.len() <= 0xffff);
        out.splice(0..2, (out.len() as u16).to_be_bytes());
        out
    }
}

struct MidiEventMapperTrack {
    cur_conf: SongTrackConfig,
    want_conf: SongTrackConfig,
    // track/instrument properties not present in SongTrackConfig
    cur_vel: u8,
    cur_key: u8,
    cur_pan: Pan,
    want_pan: Pan,
}
impl Default for MidiEventMapperTrack {
    fn default() -> Self {
        Self {
            cur_conf: Default::default(),
            want_conf: Default::default(),
            cur_vel: W4ON2_VELOCITY_MAX as u8,
            cur_key: Default::default(),
            cur_pan: Pan::Stereo,
            want_pan: Pan::Stereo,
        }
    }
}

// Takes care of sending instrument parameters as required, keeping track of notes, and other playback state
pub struct MidiEventMapper {
    tracks: [MidiEventMapperTrack; 16],
}
impl Default for MidiEventMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl MidiEventMapper {
    pub fn new() -> Self {
        Self {
            tracks: Default::default(),
        }
    }
    // TODO: don't require ownership?
    pub fn set_tracks(&mut self, tracks: [SongTrackConfig; 16]) {
        for (i, t) in tracks.into_iter().enumerate() {
            self.tracks[i].want_conf = t;
        }
    }
    fn maybe_init(&mut self, into: &mut Vec<TrackEvent>, track_i: u8) {
        let track = &mut self.tracks[track_i as usize];
        let w = &track.want_conf;
        let c = &mut track.cur_conf;
        if c.channel != w.channel {
            into.push(TrackEvent::SetFlags(w.channel.to_wasm4_flags()));
            c.channel = w.channel.clone();
        }
        if c.volume != w.volume {
            into.push(TrackEvent::SetVolume(w.volume));
            c.volume = w.volume;
        }
        if c.adsr != w.adsr {
            into.push(TrackEvent::SetADSR(w.adsr.clone()));
            c.adsr = w.adsr.clone();
        }
        if c.arpeggio != w.arpeggio {
            into.push(TrackEvent::SetArpeggio(w.arpeggio.clone()));
            c.arpeggio = w.arpeggio.clone();
        }
        if c.portamento != w.portamento {
            into.push(TrackEvent::SetPortamento(w.portamento));
            c.portamento = w.portamento;
        }
        if c.pitch_env != w.pitch_env {
            into.push(TrackEvent::SetPitchEnv(w.pitch_env.clone()));
            c.pitch_env = w.pitch_env.clone();
        }
        if c.vibrato != w.vibrato {
            into.push(TrackEvent::SetVibrato(w.vibrato.clone()));
            c.vibrato = w.vibrato.clone();
        }
    }
    pub fn note_on(&mut self, into: &mut Vec<TrackEvent>, midi_ch: u8, midi_key: u8, vel: u8) {
        self.maybe_init(into, midi_ch);
        let track = &mut self.tracks[midi_ch as usize];
        track.cur_key = midi_key;
        if track.cur_vel != vel {
            track.cur_vel = vel;
            into.push(TrackEvent::SetVelocity(vel));
        }
        if track.cur_pan != track.want_pan {
            track.cur_pan = track.want_pan;
            into.push(TrackEvent::SetPan(track.want_pan));
        }
        into.push(TrackEvent::NoteOn(midi_key));
    }
    pub fn note_off(&mut self, into: &mut Vec<TrackEvent>, midi_ch: u8, midi_key: u8) {
        self.maybe_init(into, midi_ch);
        let track = &mut self.tracks[midi_ch as usize];
        if track.cur_key == midi_key {
            into.push(TrackEvent::NotesOff);
        }
    }
    pub fn pan(&mut self, midi_ch: u8, pan: u8) {
        self.tracks[midi_ch as usize].want_pan = if pan < 43 {
            Pan::Left
        } else if pan > 84 {
            Pan::Right
        } else {
            Pan::Stereo
        };
    }
}
