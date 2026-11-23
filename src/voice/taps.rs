// Debug recording of the mic chain: `WAFFLE_VOICE_TAPS=<dir>` writes one 48 kHz mono float WAV per
// stage (mic, front, denoise, agc, ...) when the processor drops, so a call can be replayed
// stage by stage to find where an artifact starts. Grows without bound while enabled; debug only.

use std::{collections::BTreeMap, path::PathBuf};

use super::audio::SAMPLE_RATE;

pub struct Taps {
    dir: PathBuf,
    stages: BTreeMap<&'static str, Vec<f32>>, // lint-allow(unbounded-growth): a developer capture, only built when WAFFLE_VOICE_TAPS names a directory, written out and dropped when the call ends
}

impl Taps {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("WAFFLE_VOICE_TAPS").map(|dir| Self::new(dir.into()))
    }

    pub fn new(dir: PathBuf) -> Self {
        Self { dir, stages: BTreeMap::new() }
    }

    pub fn record(&mut self, stage: &'static str, samples: &[f32]) {
        self.stages.entry(stage).or_default().extend_from_slice(samples);
    }

    pub fn write(&self) {
        if let Err(e) = std::fs::create_dir_all(&self.dir) {
            return eprintln!("voice: taps: {e}");
        }
        for (stage, samples) in &self.stages {
            if let Err(e) = std::fs::write(self.dir.join(format!("{stage}.wav")), wav(samples)) {
                eprintln!("voice: taps: {stage}: {e}");
            }
        }
    }
}

impl Drop for Taps {
    fn drop(&mut self) {
        self.write();
    }
}

// 32-bit float mono WAV.
pub fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 4) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 4);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    samples.iter().for_each(|s| out.extend_from_slice(&s.to_le_bytes()));
    out
}
