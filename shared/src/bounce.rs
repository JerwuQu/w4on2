use crate::*;

unsafe extern "C" fn bounce_apu_tone(frequency: u32, duration: u32, volume: u32, flags: u32, userdata: *mut c_void) {
    let apu = unsafe { &mut *(userdata as *mut wasm4_apu::APU) };
    apu.tone(frequency, duration, volume, flags);
}

pub const WASM4_SAMPLE_RATE: u32 = 44100;

pub fn bounce_pcm(w4on2_bytes: &[u8]) -> Vec<i16> {
    const SAMPLES_PER_TICK: u32 = WASM4_SAMPLE_RATE / 60;
    const PADDING_TICKS: usize = 5;

    let apu_raw = Box::into_raw(Box::new(wasm4_apu::APU::new(WASM4_SAMPLE_RATE))); // force WASM-4 sample-rate
    let apu = unsafe { &mut *apu_raw };
    let (mut rt, mut ply) = unsafe {
        let mut rt = std::mem::zeroed::<w4on2_rt_t>();
        let mut ply = std::mem::zeroed::<w4on2_player_t>();
        w4on2_rt_init(&mut rt, Some(bounce_apu_tone), apu_raw as *mut c_void);
        w4on2_player_init(&mut ply, w4on2_bytes.as_ptr());
        (rt, ply)
    };

    let mut sample_vec = Vec::<i16>::new();
    let mut sample_buf: [i16; SAMPLES_PER_TICK as usize * 2] = [0; (SAMPLES_PER_TICK as usize * 2)];
    let mut gen_samples = |rt| {
        unsafe { w4on2_rt_tick(rt) }
        apu.tick();
        apu.write_samples(&mut sample_buf, SAMPLES_PER_TICK as usize);
        sample_vec.extend(sample_buf);
    };
    for _ in 0..PADDING_TICKS {
        gen_samples(&mut rt);
    }
    loop {
        gen_samples(&mut rt);
        if unsafe { w4on2_player_tick(&mut ply, &mut rt) } == 0 {
            break;
        }
    }
    for _ in 0..PADDING_TICKS {
        gen_samples(&mut rt);
    }

    sample_vec
}

pub fn write_wav<W: std::io::Write + std::io::Seek>(pcm: Vec<i16>, w: &mut W) -> Result<()> {
    Ok(wav::write(
        wav::Header::new(wav::WAV_FORMAT_PCM, 2, WASM4_SAMPLE_RATE, 16),
        &wav::BitDepth::Sixteen(pcm),
        w,
    )?)
}
