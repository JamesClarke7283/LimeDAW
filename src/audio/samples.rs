//! Sample data: procedurally generated drum hits plus loaded audio files.

use std::collections::HashMap;
use std::sync::Arc;

use crate::audio::dsp::*;
use crate::model::DrumModel;

/// A block of interleaved-free audio: one `Vec` per channel.
#[derive(Debug, Clone)]
pub struct Sample {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub sample_rate: f32,
    pub name: String,
}

impl Sample {
    pub fn len(&self) -> usize {
        self.left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }

    pub fn duration_secs(&self) -> f32 {
        self.left.len() as f32 / self.sample_rate.max(1.0)
    }

    /// Read a stereo frame at a fractional position, linearly interpolated.
    #[inline]
    pub fn read(&self, pos: f32) -> (f32, f32) {
        let len = self.left.len();
        if len == 0 {
            return (0.0, 0.0);
        }
        if pos < 0.0 {
            return (0.0, 0.0);
        }
        let i0 = pos as usize;
        if i0 >= len {
            return (0.0, 0.0);
        }
        let i1 = (i0 + 1).min(len - 1);
        let frac = pos - i0 as f32;
        let l = self.left[i0] * (1.0 - frac) + self.left[i1] * frac;
        let r = self.right[i0] * (1.0 - frac) + self.right[i1] * frac;
        (l, r)
    }

    /// A coarse min/max envelope for drawing the waveform in the UI.
    pub fn peaks(&self, buckets: usize) -> Vec<(f32, f32)> {
        if self.left.is_empty() || buckets == 0 {
            return Vec::new();
        }
        let per = (self.left.len() as f32 / buckets as f32).max(1.0);
        (0..buckets)
            .map(|b| {
                let start = (b as f32 * per) as usize;
                let end = (((b + 1) as f32 * per) as usize).min(self.left.len());
                let slice = &self.left[start..end.max(start + 1).min(self.left.len())];
                let mut lo = 0.0f32;
                let mut hi = 0.0f32;
                for &s in slice {
                    lo = lo.min(s);
                    hi = hi.max(s);
                }
                (lo, hi)
            })
            .collect()
    }
}

/// Render one of the built-in drum models at the engine's sample rate. These
/// are deliberately simple analogue-style voices: they make a usable kit
/// available the moment the app opens, with no sample library to install.
pub fn render_drum(model: DrumModel, sr: f32) -> Sample {
    let mut noise = Noise::with_seed(0x9E37_79B9 ^ (model as u32).wrapping_mul(2_654_435_761));
    let secs = match model {
        DrumModel::Kick => 0.7,
        DrumModel::Snare => 0.45,
        DrumModel::ClosedHat => 0.09,
        DrumModel::OpenHat => 0.55,
        DrumModel::Clap => 0.5,
        DrumModel::Tom => 0.6,
        DrumModel::Rim => 0.12,
        DrumModel::Cymbal => 1.6,
        DrumModel::Cowbell => 0.4,
    };
    let n = (secs * sr) as usize;
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);

    let mut phase = 0.0f32;
    let mut phase2 = 0.0f32;
    let mut hp = Svf::default();
    let mut bp = Svf::default();
    let mut lp = Svf::default();

    for i in 0..n {
        let t = i as f32 / sr;
        let sample = match model {
            DrumModel::Kick => {
                // Pitch sweeps from a click down to the body note.
                let freq = 48.0 + 130.0 * (-t * 42.0).exp();
                phase += freq / sr;
                let body = (phase * TAU).sin();
                let click = noise.next() * (-t * 700.0).exp() * 0.35;
                let env = (-t * 6.0).exp();
                soft_clip((body * env * 1.4) + click)
            }
            DrumModel::Snare => {
                let freq = 185.0 + 90.0 * (-t * 60.0).exp();
                phase += freq / sr;
                let tone = (phase * TAU).sin() * (-t * 26.0).exp() * 0.55;
                let n = noise.next();
                let rattle = hp.process(n, crate::model::FilterKind::HighPass, 1_400.0, 0.25, sr)
                    * (-t * 18.0).exp();
                soft_clip(tone + rattle * 0.85)
            }
            DrumModel::ClosedHat | DrumModel::OpenHat => {
                let decay = if matches!(model, DrumModel::ClosedHat) { 78.0 } else { 9.0 };
                let n = noise.next();
                let metallic = bp.process(n, crate::model::FilterKind::HighPass, 7_500.0, 0.35, sr);
                metallic * (-t * decay).exp() * 0.75
            }
            DrumModel::Clap => {
                // Three quick bursts then a tail, the way a clap actually reads.
                let bursts = [(0.0, 1.0), (0.011, 0.85), (0.023, 0.75)];
                let mut amp = 0.0;
                for (offset, gain) in bursts {
                    if t >= offset {
                        amp += gain * (-(t - offset) * 190.0).exp();
                    }
                }
                amp += 0.55 * (-(t.max(0.03) - 0.03) * 16.0).exp();
                let n = noise.next();
                bp.process(n, crate::model::FilterKind::BandPass, 1_150.0, 0.42, sr) * amp * 1.7
            }
            DrumModel::Tom => {
                let freq = 110.0 + 70.0 * (-t * 30.0).exp();
                phase += freq / sr;
                let body = (phase * TAU).sin();
                let n = noise.next() * (-t * 120.0).exp() * 0.18;
                soft_clip((body * (-t * 7.5).exp() * 1.2) + n)
            }
            DrumModel::Rim => {
                phase += 1_700.0 / sr;
                phase2 += 2_390.0 / sr;
                let tone = ((phase * TAU).sin() + (phase2 * TAU).sin() * 0.7) * 0.5;
                let n = noise.next() * 0.4;
                (tone + n) * (-t * 130.0).exp()
            }
            DrumModel::Cymbal => {
                // A stack of inharmonic squares, the classic 808-style approach.
                const RATIOS: [f32; 6] = [1.0, 1.4142, 1.7818, 2.0, 2.4495, 2.8284];
                let mut acc = 0.0;
                for (k, ratio) in RATIOS.iter().enumerate() {
                    let f = 320.0 * ratio;
                    let ph = (t * f + k as f32 * 0.17) % 1.0;
                    acc += if ph < 0.5 { 1.0 } else { -1.0 };
                }
                acc /= RATIOS.len() as f32;
                let shimmer = hp.process(acc, crate::model::FilterKind::HighPass, 5_200.0, 0.2, sr);
                let n = noise.next() * 0.3;
                (shimmer + n * 0.4) * (-t * 3.2).exp() * 0.55
            }
            DrumModel::Cowbell => {
                phase += 587.0 / sr;
                phase2 += 845.0 / sr;
                let sq = |p: f32| if p % 1.0 < 0.5 { 1.0 } else { -1.0 };
                let tone = (sq(phase) + sq(phase2)) * 0.5;
                lp.process(tone, crate::model::FilterKind::LowPass, 5_000.0, 0.1, sr)
                    * (-t * 12.0).exp()
                    * 0.6
            }
        };
        // A short fade at the tail prevents a click when the buffer ends.
        let fade = if i > n.saturating_sub(64) {
            (n - i) as f32 / 64.0
        } else {
            1.0
        };
        let s = (sample * fade).clamp(-1.0, 1.0);
        left.push(s);
        right.push(s);
    }

    Sample { left, right, sample_rate: sr, name: model.label().to_string() }
}

/// Holds every sample the engine can play, keyed by path (or a `model:` URI).
#[derive(Default)]
pub struct SampleBank {
    samples: HashMap<String, Arc<Sample>>,
}

impl SampleBank {
    pub fn key_for_model(model: DrumModel) -> String {
        format!("model:{}", model.label())
    }

    pub fn get(&self, key: &str) -> Option<&Arc<Sample>> {
        self.samples.get(key)
    }

    pub fn insert(&mut self, key: impl Into<String>, sample: Arc<Sample>) {
        self.samples.insert(key.into(), sample);
    }

    pub fn contains(&self, key: &str) -> bool {
        self.samples.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.samples.keys()
    }

    /// Fill the bank with every procedural drum model.
    pub fn populate_models(&mut self, sr: f32) {
        for model in DrumModel::ALL {
            let key = Self::key_for_model(model);
            if !self.samples.contains_key(&key) {
                self.samples.insert(key, Arc::new(render_drum(model, sr)));
            }
        }
    }

    /// Re-render the procedural drums for a new sample rate.
    pub fn rerender_models(&mut self, sr: f32) {
        for model in DrumModel::ALL {
            self.samples
                .insert(Self::key_for_model(model), Arc::new(render_drum(model, sr)));
        }
    }
}

/// Decode a WAV file into a [`Sample`]. Anything hound can read works, which
/// covers 8/16/24/32-bit integer and float WAVs.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_wav(path: &std::path::Path) -> anyhow::Result<Sample> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;

    let frames: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / max)
                .collect()
        }
    };

    let mut left = Vec::with_capacity(frames.len() / channels);
    let mut right = Vec::with_capacity(frames.len() / channels);
    for frame in frames.chunks(channels) {
        left.push(frame[0]);
        right.push(if channels > 1 { frame[1] } else { frame[0] });
    }

    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "sample".into());
    Ok(Sample { left, right, sample_rate: spec.sample_rate as f32, name })
}
