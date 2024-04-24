// based on https://github.com/aduros/wasm4/blob/b874b419d5171c194f889407262d88f4b4f796bb/runtimes/native/src/apu.c
// -> c2rust translate
// -> `libc` types replaced with rust builtin types
// -> further rustify: create struct to avoid globals, remove some easily removable unsafe, remove repr(C), remove unneeded pubs
// -> configurable SAMPLE_RATE
// -> further cleanup
// -> updated to https://github.com/aduros/wasm4/blob/0dff7ad4e6c7b28b87a6555bea8574e5aa748e27/runtimes/native/src/apu.c

/*
Copyright (c) Bruno Garcia

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE.
*/

#![allow(
    dead_code,
    mutable_transmutes,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_assignments,
    unused_mut
)]
#[derive(Copy, Clone)]
enum ChannelData {
    pulse { dutyCycle: f32 },
    noise { seed: u16, lastRandom: i16 },
}
impl Default for ChannelData {
    fn default() -> Self {
        ChannelData::pulse { dutyCycle: 0.0 }
    }
}
#[derive(Copy, Clone, Default)]
struct Channel {
    freq1: f32,
    freq2: f32,
    startTime: u64,
    attackTime: u64,
    decayTime: u64,
    sustainTime: u64,
    releaseTime: u64,
    endTick: u64,
    sustainVolume: i16,
    peakVolume: i16,
    phase: f32,
    pan: u8,
    data: ChannelData,
}

pub struct APU {
    time: u64,
    ticks: u64,
    sample_rate: u32,
    channels: [Channel; 4],
}
impl APU {
    pub fn new(sample_rate: u32) -> APU {
        APU {
            time: 0,
            ticks: 0,
            sample_rate,
            channels: [
                Channel::default(),
                Channel::default(),
                Channel::default(),
                Channel {
                    data: ChannelData::noise {
                        seed: 0x1,
                        lastRandom: 0x0,
                    },
                    ..Default::default()
                },
            ],
        }
    }
    pub fn tick(&mut self) {
        self.ticks += 1;
    }
    pub fn tone(&mut self, frequency: u32, duration: u32, volume: u32, flags: u32) {
        let freq1 = frequency & 0xffff;
        let freq2 = frequency >> 16 & 0xffff;
        let sustain = duration & 0xff;
        let release = duration >> 8 & 0xff;
        let decay = duration >> 16 & 0xff;
        let attack = duration >> 24 & 0xff;
        let sustainVolume = std::cmp::min(volume & 0xff, 100);
        let peakVolume = std::cmp::min(volume >> 8, 100);
        let channelIdx = flags & 0x3;
        let mode = flags >> 2 & 0x3;
        let pan = flags >> 4 & 0x3;
        let noteMode = flags & 0x40;
        let channel = &mut self.channels[channelIdx as usize];
        if self.time > channel.releaseTime && self.ticks != channel.endTick {
            channel.phase = (if channelIdx == 2 { 0.25f64 } else { 0_i32 as f64 }) as f32;
        }
        if noteMode != 0 {
            channel.freq1 = midiFreq((freq1 & 0xff) as u8, (freq1 >> 8) as u8);
            channel.freq2 = if freq2 == 0 {
                0.0
            } else {
                midiFreq((freq2 & 0xff) as u8, (freq2 >> 8) as u8)
            };
        } else {
            channel.freq1 = freq1 as f32;
            channel.freq2 = freq2 as f32;
        }
        channel.startTime = self.time;
        channel.attackTime = (channel.startTime).wrapping_add((self.sample_rate * attack / 60) as u64);
        channel.decayTime = (channel.attackTime).wrapping_add((self.sample_rate * decay / 60) as u64);
        channel.sustainTime = (channel.decayTime).wrapping_add((self.sample_rate * sustain / 60) as u64);
        channel.releaseTime = (channel.sustainTime).wrapping_add((self.sample_rate * release / 60) as u64);
        channel.endTick = self.ticks + attack as u64 + decay as u64 + sustain as u64 + release as u64;
        let maxVolume = if channelIdx == 2 { 0x2000 } else { 0x1333 };
        channel.sustainVolume = (maxVolume * sustainVolume / 100) as i16;
        channel.peakVolume = (if peakVolume != 0 {
            maxVolume * peakVolume / 100
        } else {
            maxVolume
        }) as i16;
        channel.pan = pan as u8;
        if channelIdx == 0 || channelIdx == 1 {
            if let ChannelData::pulse { ref mut dutyCycle } = channel.data {
                *dutyCycle = match mode {
                    0 => 0.125f32,
                    2 => 0.5f32,
                    _ => 0.25f32,
                };
            }
        } else if channelIdx == 2 && release == 0 {
            channel.releaseTime = channel
                .releaseTime
                .wrapping_add((self.sample_rate as i32 / 1000_i32) as u64);
        }
    }
    pub fn write_samples(&mut self, output: &mut [i16], frames: usize) {
        for ii in 0..frames {
            let mut mix_left: i16 = 0;
            let mut mix_right: i16 = 0;
            for channelIdx in 0..4 {
                let channel = &mut self.channels[channelIdx as usize];
                if self.time < channel.releaseTime || self.ticks == channel.endTick {
                    let freq = getCurrentFrequency(channel, self.time);
                    let volume = getCurrentVolume(channel, self.time, self.sample_rate);
                    let mut sample: i16 = 0;
                    if channelIdx == 3_i32 {
                        channel.phase += (freq * freq) / (1000000.0f32 / 44100.0f32 * self.sample_rate as f32);
                        if let ChannelData::noise {
                            ref mut seed,
                            ref mut lastRandom,
                        } = channel.data
                        {
                            while channel.phase > 0_i32 as f32 {
                                channel.phase -= 1.;
                                *seed = (*seed as i32 ^ *seed as i32 >> 7_i32) as u16;
                                *seed = (*seed as i32 ^ (*seed as i32) << 9_i32) as u16;
                                *seed = (*seed as i32 ^ *seed as i32 >> 13_i32) as u16;
                                *lastRandom = (2_i32 * (*seed as i32 & 0x1_i32) - 1_i32) as i16;
                            }
                            sample = (volume as i32 * *lastRandom as i32) as i16;
                        }
                    } else {
                        let phaseInc = freq / self.sample_rate as f32;
                        channel.phase += phaseInc;
                        if channel.phase >= 1_f32 {
                            channel.phase -= 1.;
                        }
                        if channelIdx == 2_i32 {
                            sample = (volume as i32 as f64
                                * (2_f64 * ((2_f32 * channel.phase - 1_f32) as f64).abs() - 1_f64))
                                as i16;
                        } else {
                            let mut dutyPhase: f32 = 0.;
                            let mut dutyPhaseInc: f32 = 0.;
                            let mut multiplier: i16 = 0;
                            if let ChannelData::pulse { dutyCycle } = channel.data {
                                if channel.phase < dutyCycle {
                                    dutyPhase = channel.phase / dutyCycle;
                                    dutyPhaseInc = phaseInc / dutyCycle;
                                    multiplier = volume;
                                } else {
                                    dutyPhase = (channel.phase - dutyCycle) / (1.0f32 - dutyCycle);
                                    dutyPhaseInc = phaseInc / (1.0f32 - dutyCycle);
                                    multiplier = -(volume as i32) as i16;
                                }
                            }
                            sample = (multiplier as i32 as f32 * polyblep(dutyPhase, dutyPhaseInc)) as i16;
                        }
                    }
                    if channel.pan as i32 != 1_i32 {
                        mix_right = (mix_right as i32 + sample as i32) as i16;
                    }
                    if channel.pan as i32 != 2_i32 {
                        mix_left = (mix_left as i32 + sample as i32) as i16;
                    }
                }
            }
            output[ii * 2] = mix_left;
            output[ii * 2 + 1] = mix_right;
            self.time += 1;
        }
    }
}
fn lerp(value1: i32, value2: i32, t: f32) -> i32 {
    (value1 as f32 + t * (value2 - value1) as f32) as i32
}
fn lerpf(value1: f32, value2: f32, t: f32) -> f32 {
    value1 + t * (value2 - value1)
}
fn ramp(time: u64, value1: i32, value2: i32, time1: u64, time2: u64) -> i32 {
    if time >= time2 {
        value2
    } else {
        let t: f32 = time.wrapping_sub(time1) as f32 / time2.wrapping_sub(time1) as f32;
        lerp(value1, value2, t)
    }
}
fn rampf(time: u64, value1: f32, value2: f32, time1: u64, time2: u64) -> f32 {
    if time >= time2 {
        value2
    } else {
        let t: f32 = time.wrapping_sub(time1) as f32 / time2.wrapping_sub(time1) as f32;
        lerpf(value1, value2, t)
    }
}
fn getCurrentFrequency(channel: &Channel, time: u64) -> f32 {
    if channel.freq2 > 0.0 {
        rampf(
            time,
            channel.freq1,
            channel.freq2,
            channel.startTime,
            channel.releaseTime,
        )
    } else {
        channel.freq1
    }
}
fn getCurrentVolume(channel: &Channel, time: u64, sample_rate: u32) -> i16 {
    if time >= channel.sustainTime && (channel.releaseTime - channel.sustainTime) > (sample_rate as u64 / 1000) {
        ramp(
            time,
            channel.sustainVolume as i32,
            0_i32,
            channel.sustainTime,
            channel.releaseTime,
        ) as i16
    } else if time >= channel.decayTime {
        channel.sustainVolume
    } else if time >= channel.attackTime {
        ramp(
            time,
            channel.peakVolume as i32,
            channel.sustainVolume as i32,
            channel.attackTime,
            channel.decayTime,
        ) as i16
    } else {
        ramp(
            time,
            0_i32,
            channel.peakVolume as i32,
            channel.startTime,
            channel.attackTime,
        ) as i16
    }
}
fn polyblep(phase: f32, phaseInc: f32) -> f32 {
    if phase < phaseInc {
        let t: f32 = phase / phaseInc;
        t + t - t * t
    } else if phase > 1.0f32 - phaseInc {
        let t: f32 = (phase - (1.0f32 - phaseInc)) / phaseInc;
        1.0 - (t + t - t * t)
    } else {
        1.0
    }
}
fn midiFreq(note: u8, bend: u8) -> f32 {
    2.0f32.powf((note as f32 - 69.0f32 + bend as f32 / 256.0f32) / 12.0f32) * 440.0f32
}
