use native_dialog::MessageDialog;
use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, containers::Frame, emath, Align, Layout, RichText},
    EguiState,
};
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::{
    ffi::c_void,
    ops::RangeInclusive,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};
use w4on2_shared::SONG_TRACK_CONFIG_DEFAULT as STCD;
use w4on2_shared::{optimal_bpm, runtime::*, Channel, PulseDuty, SongTrackConfig, TrackEvent};
use w4on2_shared::{wasm4_apu, MidiEventMapper, SongConfig};
use widgets::Knob;

mod widgets;

unsafe extern "C" fn apu_tone(frequency: u32, duration: u32, volume: u32, flags: u32, userdata: *mut c_void) {
    // trace!("apu_tone {frequency} {duration} {volume} {flags}");
    let apu = unsafe { &mut *(userdata as *mut wasm4_apu::APU) };
    apu.tone(frequency, duration, volume, flags);
}

struct Generator {
    sample_rate: u32,
    sample: usize,
    apu: *mut wasm4_apu::APU,
    engine: w4on2_rt_t,
    timing: (f64, f64),
    mapper: MidiEventMapper,
    event_buffer: Vec<TrackEvent>,
    serialize_buffer: Vec<u8>,
}
unsafe impl Send for Generator {}
impl Generator {
    fn new(sample_rate: u32) -> Self {
        let apu_raw = Box::into_raw(Box::new(wasm4_apu::APU::new(sample_rate)));
        let engine = unsafe {
            let mut w = std::mem::zeroed::<w4on2_rt_t>();
            w4on2_rt_init(&mut w, Some(apu_tone), apu_raw as *mut c_void);
            w
        };
        Self {
            sample_rate,
            sample: 0,
            apu: apu_raw,
            engine,
            timing: (0.0, 0.0),
            mapper: MidiEventMapper::new(),
            event_buffer: Vec::with_capacity(16),
            serialize_buffer: Vec::with_capacity(16),
        }
    }
    fn reload_instruments(&mut self, conf: &SongConfig) {
        self.mapper.set_tracks(conf.channels.clone());
    }
}

enum ConvertStatus {
    NoPath,
    Waiting(PathBuf),
    Converting,
    Failed(PathBuf),
    Ok(PathBuf, Vec<u8>),
}

pub struct W4ON2 {
    params: Arc<W4ON2Params>,
    generator: Arc<Mutex<Option<Generator>>>,
    convert_status: Arc<Mutex<ConvertStatus>>,
}
impl Default for W4ON2 {
    fn default() -> Self {
        Self {
            params: Arc::new(W4ON2Params::default()),
            generator: Arc::new(Mutex::new(None)),
            convert_status: Arc::new(Mutex::new(ConvertStatus::NoPath)),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct ConvertConfig {
    stretch: bool,
    crunch: bool,
}
impl Default for ConvertConfig {
    fn default() -> Self {
        Self {
            stretch: true,
            crunch: false,
        }
    }
}

#[derive(Params)]
pub struct W4ON2Params {
    #[persist = "song_config"]
    song_config: Arc<RwLock<SongConfig>>,

    #[persist = "convert_config"]
    convert_config: Arc<RwLock<ConvertConfig>>,
}

impl Default for W4ON2Params {
    fn default() -> Self {
        Self {
            song_config: Arc::new(RwLock::new(SongConfig::default())),
            convert_config: Arc::new(RwLock::new(ConvertConfig::default())),
        }
    }
}

fn simple_error(text: String) {
    std::thread::spawn(move || {
        MessageDialog::new()
            .set_type(native_dialog::MessageType::Error)
            .set_title("w4on2 error")
            .set_text(&text)
            .show_alert()
            .unwrap();
    });
}

fn num_ctrl<Num: emath::Numeric>(
    changed: &mut bool,
    ui: &mut egui::Ui,
    label: &str,
    val: &mut Num,
    range: RangeInclusive<Num>,
    def: Num,
) {
    ui.allocate_ui(egui::vec2(50.0, 150.0), |ui| {
        Frame::group(ui.style()).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(Align::Center), |ui| {
                ui.label(label);
                *changed |= ui.add(Knob::new(label, val, range, Some(def))).changed();
            });
        });
    });
}

fn load_toml_path(toml_path: &PathBuf) -> Option<SongConfig> {
    if let Ok(toml) = std::fs::read_to_string(toml_path) {
        match SongConfig::from_toml(&toml) {
            Ok(conf) => {
                return Some(conf);
            }
            Err(err) => simple_error(format!("Failed to parse file: {}", err)),
        }
    } else {
        simple_error("Failed to read file".to_owned());
    }
    None
}
fn save_toml_path(config: &SongConfig, toml_path: &PathBuf) {
    let str = config.to_toml().unwrap();
    if let Err(err) = std::fs::write(toml_path, str) {
        simple_error(format!("Failed to write file: {}", err));
    }
}
fn run_convert_midi(
    status: Arc<Mutex<ConvertStatus>>,
    // requires taking ownership because this will run on another thread
    song_conf: SongConfig,
    conv_conf: ConvertConfig,
    midi_path: PathBuf,
) {
    std::thread::spawn(move || {
        *status.lock().unwrap() = ConvertStatus::Converting;
        if let Ok(midi_bytes) = std::fs::read(&midi_path) {
            match w4on2_shared::convert::convert(&song_conf, &midi_bytes, conv_conf.stretch, conv_conf.crunch) {
                Ok(converted) => {
                    *status.lock().unwrap() = ConvertStatus::Ok(midi_path, converted);
                }
                Err(err) => {
                    *status.lock().unwrap() = ConvertStatus::Failed(midi_path);
                    simple_error(format!("Failed to parse file: {}", err))
                }
            }
        } else {
            *status.lock().unwrap() = ConvertStatus::Failed(midi_path);
            simple_error("Failed to read file".to_owned());
        }
    });
}
fn save_w4on2(w4on2_path: &PathBuf, data: &[u8]) {
    if let Err(err) = std::fs::write(w4on2_path, data) {
        simple_error(format!("Failed to write file: {}", err));
    }
}

fn channel_ctrl_ui(ui: &mut egui::Ui, ch: &mut SongTrackConfig) -> bool {
    let mut changed = Frame::group(ui.style())
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("WASM-4 Channel");
                egui::ComboBox::from_id_source("channel")
                    .selected_text(ch.channel.to_string())
                    .show_ui(ui, |ui| {
                        Channel::types().iter().fold(false, |a, t| {
                            ui.selectable_value(&mut ch.channel, t.clone(), t.to_string()).clicked() || a
                        })
                    })
                    .inner
                    .unwrap_or(false)
                    || match ch.channel {
                        Channel::Pulse1(ref mut dc) | Channel::Pulse2(ref mut dc) => {
                            egui::ComboBox::from_id_source("duty")
                                .selected_text(dc.to_string())
                                .show_ui(ui, |ui| {
                                    PulseDuty::types()
                                        .iter()
                                        .fold(false, |a, t| ui.selectable_value(dc, *t, t.to_string()).clicked() || a)
                                })
                                .inner
                                .unwrap_or(false)
                        }
                        _ => false,
                    }
            })
            .inner
        })
        .inner;
    ui.horizontal(|ui| {
        num_ctrl(
            &mut changed,
            ui,
            "Vol",
            &mut ch.volume,
            0..=W4ON2_VOLUME_MAX as u8,
            STCD.volume,
        );
        num_ctrl(&mut changed, ui, "A", &mut ch.adsr.0, 0..=255, STCD.adsr.0);
        num_ctrl(&mut changed, ui, "D", &mut ch.adsr.1, 0..=255, STCD.adsr.1);
        num_ctrl(
            &mut changed,
            ui,
            "S",
            &mut ch.adsr.2,
            0..=W4ON2_SUSTAIN_MAX as u8,
            STCD.adsr.2,
        );
        num_ctrl(&mut changed, ui, "R", &mut ch.adsr.3, 0..=255, STCD.adsr.3);
        num_ctrl(&mut changed, ui, "Porta", &mut ch.portamento, 0..=255, STCD.portamento);
    });
    ui.horizontal(|ui| {
        Frame::group(ui.style()).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label("Arpeggio");
                ui.horizontal(|ui| {
                    num_ctrl(
                        &mut changed,
                        ui,
                        "Rate",
                        &mut ch.arpeggio.rate,
                        0..=255,
                        STCD.arpeggio.rate,
                    );
                });
            });
        });
        Frame::group(ui.style()).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label("Pitch Envelope");
                ui.horizontal(|ui| {
                    num_ctrl(
                        &mut changed,
                        ui,
                        "Offset",
                        &mut ch.pitch_env.note_offset,
                        -127..=127,
                        STCD.pitch_env.note_offset,
                    );
                    num_ctrl(
                        &mut changed,
                        ui,
                        "Dur",
                        &mut ch.pitch_env.duration,
                        0..=255,
                        STCD.pitch_env.duration,
                    );
                });
            });
        });
        Frame::group(ui.style()).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label("Vibrato");
                ui.horizontal(|ui| {
                    num_ctrl(
                        &mut changed,
                        ui,
                        "Speed",
                        &mut ch.vibrato.speed,
                        0..=255,
                        STCD.vibrato.speed,
                    );
                    num_ctrl(
                        &mut changed,
                        ui,
                        "Depth",
                        &mut ch.vibrato.depth,
                        0..=255,
                        STCD.vibrato.depth,
                    );
                });
            });
        });
    });
    changed
}

#[derive(PartialEq)]
enum UIMode {
    Compose,
    Convert,
}

impl Plugin for W4ON2 {
    const NAME: &'static str = "w4on2";
    const VENDOR: &'static str = "Marcus Ramse";
    const URL: &'static str = "https://ramse.se/";
    const EMAIL: &'static str = "marcus@ramse.se";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let gen = self.generator.clone();
        let convert_status = self.convert_status.clone();
        create_egui_editor(
            EguiState::from_size(600, 400), // force size
            (UIMode::Compose, 0),
            |_, _| {},
            move |egui_ctx, _setter, (selected_mode, selected_channel)| {
                egui::CentralPanel::default().show(egui_ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.horizontal(|ui| {
                                if ui.button("Load TOML...").clicked() {
                                    let t_gen = gen.clone();
                                    let conf_t = params.song_config.clone();
                                    std::thread::spawn(move || {
                                        let file = FileDialog::new()
                                            .add_filter("toml", &["toml"])
                                            .set_directory("/")
                                            .pick_file();
                                        if let Some(f) = file {
                                            if let Some(new_conf) = load_toml_path(&f) {
                                                let gen = &mut *t_gen.lock().unwrap();
                                                let conf = &mut *conf_t.write().unwrap();
                                                gen.as_mut().unwrap().reload_instruments(&new_conf);
                                                *conf = new_conf;
                                            }
                                        }
                                    });
                                }
                                if ui.button("Save TOML...").clicked() {
                                    let conf_t = params.song_config.clone();
                                    std::thread::spawn(move || {
                                        let file = FileDialog::new()
                                            .add_filter("toml", &["toml"])
                                            .set_directory("/")
                                            .save_file();
                                        if let Some(f) = file {
                                            save_toml_path(&conf_t.read().unwrap(), &f);
                                        }
                                    });
                                }
                            });

                            ui.with_layout(Layout::top_down(Align::Max), |ui| {
                                let sample_rate = {
                                    let gen_lock = gen.lock().unwrap();
                                    gen_lock.as_ref().map(|g| g.sample_rate).unwrap_or(0)
                                };
                                ui.label(
                                    RichText::new(format!("Sample Rate: {} | WASM-4 Sample Rate: 44100", sample_rate))
                                        .color(if sample_rate == 44100 {
                                            egui::Color32::GREEN
                                        } else {
                                            egui::Color32::YELLOW
                                        }),
                                );
                                let (midi_bpm, opti_bpm) =
                                    gen.lock().unwrap().as_ref().map(|g| g.timing).unwrap_or((0.0, 0.0));
                                ui.label(
                                    RichText::new(format!(
                                        "BPM: {:.3} | Closest Optimal BPM: {:.3}",
                                        midi_bpm, opti_bpm
                                    ))
                                    .color(
                                        if (midi_bpm - opti_bpm).abs() < 0.001 {
                                            egui::Color32::GREEN
                                        } else {
                                            egui::Color32::YELLOW
                                        },
                                    ),
                                );
                            });
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.selectable_value(selected_mode, UIMode::Compose, "Compose");
                            ui.selectable_value(selected_mode, UIMode::Convert, "Convert");
                        });
                        ui.separator();
                        match selected_mode {
                            UIMode::Compose => {
                                let song_conf = &mut *params.song_config.write().unwrap();
                                ui.horizontal(|ui| {
                                    let format_nick = |i: usize, s: &str| {
                                        if s.is_empty() {
                                            format!("Channel #{}", i + 1)
                                        } else {
                                            format!("Channel #{} ({})", i + 1, s)
                                        }
                                    };
                                    egui::ComboBox::from_label("")
                                        .selected_text(format_nick(
                                            *selected_channel,
                                            &song_conf.channels[*selected_channel].nickname,
                                        ))
                                        .height(400.0) // show all
                                        .show_ui(ui, |ui| {
                                            for ch_i in 0..16 {
                                                ui.selectable_value(
                                                    selected_channel,
                                                    ch_i,
                                                    format_nick(ch_i, &song_conf.channels[ch_i].nickname),
                                                );
                                            }
                                        });
                                    // I would use a text input here but... https://github.com/robbert-vdh/nih-plug/issues/105
                                    //if ui.button("Rename").clicked() {
                                    //  TODO: eframe?
                                    //}
                                });
                                if channel_ctrl_ui(ui, &mut song_conf.channels[*selected_channel]) {
                                    gen.lock().unwrap().as_mut().unwrap().reload_instruments(song_conf);
                                }
                                // TODO: show channel sound bars to the right :]
                            }
                            UIMode::Convert => {
                                let song_conf = &mut params.song_config.write().unwrap();
                                let conv_conf = &mut params.convert_config.write().unwrap();
                                let status = &*convert_status.lock().unwrap();

                                ui.add_enabled_ui(!matches!(status, ConvertStatus::Converting), |ui| {
                                    if ui.button("Load MIDI...").clicked() {
                                        let conv_t = convert_status.clone();
                                        std::thread::spawn(move || {
                                            let file = FileDialog::new()
                                                .add_filter("mid", &["mid", "midi"])
                                                .set_directory("/")
                                                .pick_file();
                                            if let Some(f) = file {
                                                *conv_t.lock().unwrap() = ConvertStatus::Waiting(f);
                                            }
                                        });
                                    }
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut conv_conf.stretch, "Stretch to optimal BPM");
                                        ui.checkbox(&mut conv_conf.crunch, "Crunch/compress file (slow)");
                                    });
                                });
                                ui.horizontal(|ui| {
                                    let conv_path = match status {
                                        ConvertStatus::NoPath => None,
                                        ConvertStatus::Converting => None,
                                        ConvertStatus::Waiting(p) => Some(p),
                                        ConvertStatus::Failed(p) => Some(p),
                                        ConvertStatus::Ok(p, _) => Some(p),
                                    };
                                    ui.add_enabled_ui(conv_path.is_some(), |ui| {
                                        if ui.button("Run").clicked() {
                                            run_convert_midi(
                                                convert_status.clone(),
                                                song_conf.clone(),
                                                conv_conf.clone(),
                                                conv_path.unwrap().clone(),
                                            );
                                        }
                                    });
                                    match status {
                                        ConvertStatus::NoPath => {
                                            ui.label("No path");
                                        }
                                        ConvertStatus::Waiting(_) => {
                                            ui.label("Ready");
                                        }
                                        ConvertStatus::Converting => {
                                            ui.label("Converting...");
                                        }
                                        ConvertStatus::Failed(_) => {
                                            ui.label("Failed!");
                                        }
                                        ConvertStatus::Ok(_, c) => {
                                            ui.label(format!("Converted! {} bytes.", c.len()));
                                        }
                                    }
                                });
                                ui.add_enabled_ui(matches!(status, ConvertStatus::Ok(_, _)), |ui| {
                                    if ui.button("Save w4on2...").clicked() {
                                        let conv_t = convert_status.clone();
                                        std::thread::spawn(move || {
                                            let file = FileDialog::new()
                                                .add_filter("w4on2", &["w4on2"])
                                                .set_directory("/")
                                                .save_file();
                                            if let Some(f) = file {
                                                if let ConvertStatus::Ok(_, data) = &*conv_t.lock().unwrap() {
                                                    save_w4on2(&f, data);
                                                }
                                            }
                                        });
                                    }
                                });
                            }
                        }
                    });
                });
            },
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        *self.generator.lock().unwrap() = Some(Generator::new(buffer_config.sample_rate as u32));
        true
    }

    fn reset(&mut self) {
        // TODO: proper runtime reset...?
        self.generator
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .reload_instruments(&self.params.song_config.read().unwrap());
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut gen_lock = self.generator.lock().unwrap();
        let gen = gen_lock.as_mut().unwrap();
        let apu = unsafe { &mut *gen.apu };

        let mut next_event = context.next_event();
        for (sample_id, channel_samples) in buffer.iter_samples().enumerate() {
            // Handle MIDI events
            while let Some(event) = next_event {
                if event.timing() > sample_id as u32 {
                    break;
                }

                // Handle event
                gen.event_buffer.clear();
                match event {
                    NoteEvent::NoteOn {
                        note,
                        velocity,
                        channel,
                        ..
                    } => {
                        //info!("[note-on] key:{note} | vel:{velocity} | ch:{channel}");
                        let vel = (velocity * 127.0) as u8;
                        gen.mapper.note_on(&mut gen.event_buffer, channel, note, vel);
                    }
                    NoteEvent::NoteOff { note, channel, .. } => {
                        //info!("[note-off] key:{note} | ch:{channel}");
                        gen.mapper.note_off(&mut gen.event_buffer, channel, note);
                    }
                    NoteEvent::MidiCC { channel, cc, value, .. } => {
                        if cc == control_change::PAN_MSB {
                            gen.mapper.pan(channel, (value * 127.0) as u8);
                        }
                    }
                    _ => {}
                }
                let event_ch = event.channel().unwrap_or(0);
                for e in &gen.event_buffer {
                    gen.serialize_buffer.clear();
                    e.serialize_into(&mut gen.serialize_buffer);
                    unsafe {
                        w4on2_rt_feed_event(&mut gen.engine, event_ch, gen.serialize_buffer.as_ptr());
                    }
                }

                next_event = context.next_event();
            }

            // WASM-4 tick
            let prev_tick = gen.sample.checked_sub(1).map(|v| v * 60 / (gen.sample_rate as usize));
            let tick = gen.sample * 60 / (gen.sample_rate as usize);
            let new_ticks = prev_tick.map(|t| tick - t).unwrap_or(1);
            // NOTE: If `new_ticks` increased by more than one tick we are no longer sample-perfect.
            // If that happened, the computer likely had a CPU spike or the audio buffer is too large, so it's not hugely important.
            // Though still... TODO: rewrite code to be sample-perfect.
            for _ in 0..new_ticks {
                unsafe {
                    w4on2_rt_tick(&mut gen.engine);
                }
                apu.tick();
            }

            // WASM-4 APU sample generation
            let mut ss: [i16; 2] = [0; 2];
            apu.write_samples(&mut ss, 1);
            for (i, sample) in channel_samples.into_iter().enumerate() {
                if i < 2 {
                    *sample = (ss[i] as f32) / 32768.0;
                    // trace!("sample: #{i}, val: {}", *sample);
                }
            }
            gen.sample += 1;
        }

        // update timing
        let midi_bpm = context.transport().tempo.unwrap_or(0.0);
        let midi_num = context.transport().time_sig_numerator.unwrap_or(4);
        let midi_denom = context.transport().time_sig_numerator.unwrap_or(4);
        let (opti_bpm, _) = optimal_bpm(midi_bpm, midi_num, midi_denom);
        gen.timing = (midi_bpm, opti_bpm);

        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for W4ON2 {
    const CLAP_ID: &'static str = "se.ramse.w4on2";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("w4on2 composer assistant");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for W4ON2 {
    const VST3_CLASS_ID: [u8; 16] = *b"se.ramse.w4on2__";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Synth,
        Vst3SubCategory::Tools,
    ];
}

nih_export_clap!(W4ON2);
nih_export_vst3!(W4ON2);
