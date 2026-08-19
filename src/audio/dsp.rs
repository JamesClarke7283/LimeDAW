//! Small, allocation-free DSP building blocks used all over the engine.

use crate::model::{FilterKind, Waveform};

pub const TAU: f32 = std::f32::consts::TAU;

/// Convert a MIDI key (with fractional semitones) to frequency in Hz.
#[inline]
pub fn key_to_hz(key: f32) -> f32 {
    440.0 * ((key - 69.0) / 12.0).exp2()
}

#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

#[inline]
pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 1e-9 {
        -180.0
    } else {
        20.0 * gain.log10()
    }
}

/// Equal-power pan. `pan` is -1..1; returns (left, right) gains.
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let p = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * TAU * 0.5; // 0..=pi/2
    (p.cos(), p.sin())
}

/// A cheap deterministic noise source. Not cryptographic, just white.
#[derive(Debug, Clone)]
pub struct Noise {
    state: u32,
}

impl Default for Noise {
    fn default() -> Self {
        Self { state: 0x2545_F491 }
    }
}

// `next` reads naturally for a sample generator, and these are hot
// enough that wrapping them in `Iterator` is not worth the indirection.
#[allow(clippy::should_implement_trait)]
impl Noise {
    pub fn with_seed(seed: u32) -> Self {
        Self { state: seed | 1 }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        // xorshift32
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// PolyBLEP band-limited step, to keep saw/square edges from aliasing.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// A single band-limited oscillator.
#[derive(Debug, Clone)]
pub struct Osc {
    pub phase: f32,
    noise: Noise,
    /// State for the triangle's leaky integrator.
    tri_state: f32,
}

impl Default for Osc {
    fn default() -> Self {
        Self {
            phase: 0.0,
            noise: Noise::default(),
            tri_state: 0.0,
        }
    }
}

// `next` reads naturally for a sample generator, and these are hot
// enough that wrapping them in `Iterator` is not worth the indirection.
#[allow(clippy::should_implement_trait)]
impl Osc {
    pub fn with_seed(seed: u32) -> Self {
        Self {
            phase: 0.0,
            noise: Noise::with_seed(seed),
            tri_state: 0.0,
        }
    }

    pub fn reset(&mut self, phase: f32) {
        self.phase = phase.rem_euclid(1.0);
        self.tri_state = 0.0;
    }

    /// Advance by `inc` (frequency / sample_rate) and produce one sample.
    #[inline]
    pub fn next(&mut self, waveform: Waveform, inc: f32, pulse_width: f32) -> f32 {
        let dt = inc;
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let t = self.phase;
        match waveform {
            Waveform::Sine => (t * TAU).sin(),
            Waveform::Saw => {
                let mut v = 2.0 * t - 1.0;
                v -= poly_blep(t, dt);
                v
            }
            Waveform::Square | Waveform::Pulse25 | Waveform::Pulse12 => {
                let width = match waveform {
                    Waveform::Square => pulse_width.clamp(0.02, 0.98),
                    Waveform::Pulse25 => (pulse_width * 0.5).clamp(0.02, 0.98),
                    _ => (pulse_width * 0.25).clamp(0.02, 0.98),
                };
                let mut v = if t < width { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                let t2 = (t + (1.0 - width)) % 1.0;
                v -= poly_blep(t2, dt);
                v
            }
            Waveform::Triangle => {
                // Integrated band-limited square gives a clean triangle.
                let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                sq += poly_blep(t, dt);
                sq -= poly_blep((t + 0.5) % 1.0, dt);
                self.tri_state = sq * 4.0 * dt + (1.0 - 4.0 * dt) * self.tri_state;
                self.tri_state * 2.2
            }
            Waveform::Noise => self.noise.next(),
        }
    }
}

/// A state-variable filter (Andrew Simper's TPT topology). Stable when
/// modulated hard, which matters for filter envelopes.
#[derive(Debug, Clone, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    #[inline]
    pub fn process(
        &mut self,
        input: f32,
        kind: FilterKind,
        cutoff: f32,
        resonance: f32,
        sr: f32,
    ) -> f32 {
        if matches!(kind, FilterKind::Off) {
            return input;
        }
        let cutoff = cutoff.clamp(20.0, sr * 0.45);
        let g = (std::f32::consts::PI * cutoff / sr).tan();
        // Map 0..1 resonance to Q of 0.5..=12
        let q = 0.5 + resonance.clamp(0.0, 0.98) * 11.5;
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;

        let out = match kind {
            FilterKind::LowPass => v2,
            FilterKind::HighPass => input - k * v1 - v2,
            FilterKind::BandPass => v1,
            FilterKind::Notch => input - k * v1,
            FilterKind::Off => input,
        };
        if out.is_finite() {
            out
        } else {
            self.reset();
            0.0
        }
    }
}

/// A one-pole low-pass, for damping and smoothing.
#[derive(Debug, Clone, Default)]
pub struct OnePole {
    z: f32,
}

impl OnePole {
    #[inline]
    pub fn process(&mut self, input: f32, cutoff: f32, sr: f32) -> f32 {
        let x = (-TAU * cutoff.clamp(1.0, sr * 0.49) / sr).exp();
        self.z = input * (1.0 - x) + self.z * x;
        if !self.z.is_finite() {
            self.z = 0.0;
        }
        self.z
    }

    #[inline]
    pub fn value(&self) -> f32 {
        self.z
    }

    pub fn reset(&mut self) {
        self.z = 0.0;
    }
}

/// A parameter that glides to its target instead of stepping, so knob moves
/// and automation don't click.
#[derive(Debug, Clone, Copy)]
pub struct Smoothed {
    current: f32,
    target: f32,
    coeff: f32,
}

// `next` reads naturally for a sample generator, and these are hot enough that
// wrapping them in `Iterator` is not worth the indirection.
#[allow(clippy::should_implement_trait)]
impl Smoothed {
    pub fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            coeff: 0.002,
        }
    }

    pub fn set_time(&mut self, seconds: f32, sr: f32) {
        self.coeff = 1.0 - (-1.0 / (seconds.max(1e-4) * sr)).exp();
    }

    #[inline]
    pub fn set_target(&mut self, value: f32) {
        self.target = value;
    }

    #[inline]
    pub fn jump(&mut self, value: f32) {
        self.current = value;
        self.target = value;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        self.current += (self.target - self.current) * self.coeff;
        self.current
    }

    #[inline]
    pub fn value(&self) -> f32 {
        self.current
    }
}

/// Which phase an ADSR is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// An exponential-ish ADSR envelope.
#[derive(Debug, Clone)]
pub struct Adsr {
    pub stage: EnvStage,
    level: f32,
    attack_rate: f32,
    decay_rate: f32,
    sustain: f32,
    release_rate: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Self {
            stage: EnvStage::Idle,
            level: 0.0,
            attack_rate: 0.01,
            decay_rate: 0.001,
            sustain: 0.8,
            release_rate: 0.001,
        }
    }
}

// `next` reads naturally for a sample generator, and these are hot
// enough that wrapping them in `Iterator` is not worth the indirection.
#[allow(clippy::should_implement_trait)]
impl Adsr {
    pub fn configure(&mut self, env: &crate::model::Envelope, sr: f32) {
        self.attack_rate = 1.0 / (env.attack.max(0.0005) * sr);
        self.decay_rate = 1.0 / (env.decay.max(0.0005) * sr);
        self.sustain = env.sustain.clamp(0.0, 1.0);
        self.release_rate = 1.0 / (env.release.max(0.0005) * sr);
    }

    pub fn trigger(&mut self) {
        self.stage = EnvStage::Attack;
    }

    pub fn retrigger_from_zero(&mut self) {
        self.level = 0.0;
        self.stage = EnvStage::Attack;
    }

    pub fn release(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
        }
    }

    pub fn kill(&mut self) {
        self.stage = EnvStage::Idle;
        self.level = 0.0;
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.stage != EnvStage::Idle
    }

    #[inline]
    pub fn level(&self) -> f32 {
        self.level
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            EnvStage::Idle => self.level = 0.0,
            EnvStage::Attack => {
                self.level += self.attack_rate;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvStage::Decay;
                }
            }
            EnvStage::Decay => {
                // Exponential-ish approach to the sustain level.
                self.level -= self.decay_rate * (self.level - self.sustain + 0.02);
                if self.level <= self.sustain + 1e-4 {
                    self.level = self.sustain;
                    self.stage = EnvStage::Sustain;
                }
            }
            EnvStage::Sustain => {
                self.level = self.sustain;
                if self.sustain <= 0.0 {
                    self.stage = EnvStage::Idle;
                }
            }
            EnvStage::Release => {
                self.level -= self.release_rate * (self.level + 0.02);
                if self.level <= 1e-4 {
                    self.level = 0.0;
                    self.stage = EnvStage::Idle;
                }
            }
        }
        self.level
    }
}

/// A fractional-delay line with linear interpolation.
#[derive(Debug, Clone)]
pub struct DelayLine {
    buffer: Vec<f32>,
    write: usize,
}

impl DelayLine {
    pub fn new(max_samples: usize) -> Self {
        Self {
            buffer: vec![0.0; max_samples.max(2)],
            write: 0,
        }
    }

    pub fn clear(&mut self) {
        self.buffer.iter_mut().for_each(|s| *s = 0.0);
        self.write = 0;
    }

    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub fn write(&mut self, value: f32) {
        let v = if value.is_finite() { value } else { 0.0 };
        self.buffer[self.write] = v;
        self.write = (self.write + 1) % self.buffer.len();
    }

    /// Read `delay` samples back from the write head.
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let len = self.buffer.len();
        let d = delay.clamp(1.0, len as f32 - 2.0);
        let pos = self.write as f32 - d;
        let pos = if pos < 0.0 { pos + len as f32 } else { pos };
        let i0 = pos.floor() as usize % len;
        let i1 = (i0 + 1) % len;
        let frac = pos - pos.floor();
        self.buffer[i0] * (1.0 - frac) + self.buffer[i1] * frac
    }
}

/// A first-order all-pass, the building block of phasers.
#[derive(Debug, Clone, Default)]
pub struct AllPass1 {
    z: f32,
}

impl AllPass1 {
    #[inline]
    pub fn process(&mut self, input: f32, coeff: f32) -> f32 {
        let out = -input * coeff + self.z;
        self.z = input + out * coeff;
        if !self.z.is_finite() {
            self.z = 0.0;
        }
        out
    }
}

/// A Schroeder all-pass with a fixed delay, for reverb diffusion.
#[derive(Debug, Clone)]
pub struct AllPassDelay {
    buffer: Vec<f32>,
    index: usize,
    feedback: f32,
}

impl AllPassDelay {
    pub fn new(len: usize, feedback: f32) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            index: 0,
            feedback,
        }
    }

    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.index];
        let output = -input + buffered;
        self.buffer[self.index] = input + buffered * self.feedback;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }

    pub fn clear(&mut self) {
        self.buffer.iter_mut().for_each(|s| *s = 0.0);
    }
}

/// A damped comb filter, the other half of a Freeverb.
#[derive(Debug, Clone)]
pub struct Comb {
    buffer: Vec<f32>,
    index: usize,
    filter_store: f32,
}

impl Comb {
    pub fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            index: 0,
            filter_store: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, input: f32, feedback: f32, damp: f32) -> f32 {
        let output = self.buffer[self.index];
        self.filter_store = output * (1.0 - damp) + self.filter_store * damp;
        if !self.filter_store.is_finite() {
            self.filter_store = 0.0;
        }
        let v = input + self.filter_store * feedback;
        self.buffer[self.index] = if v.is_finite() { v } else { 0.0 };
        self.index = (self.index + 1) % self.buffer.len();
        output
    }

    pub fn clear(&mut self) {
        self.buffer.iter_mut().for_each(|s| *s = 0.0);
        self.filter_store = 0.0;
    }
}

/// A biquad, used for the shelving and peaking bands of the EQ.
#[derive(Debug, Clone, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    pub fn low_shelf(&mut self, freq: f32, gain_db: f32, sr: f32) {
        let a = db_to_gain(gain_db / 2.0);
        let w = TAU * freq.clamp(20.0, sr * 0.45) / sr;
        let (sn, cs) = w.sin_cos();
        let alpha = sn / 2.0 * (2.0f32).sqrt();
        let sq = 2.0 * a.sqrt() * alpha;
        let a0 = (a + 1.0) + (a - 1.0) * cs + sq;
        self.b0 = a * ((a + 1.0) - (a - 1.0) * cs + sq) / a0;
        self.b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cs) / a0;
        self.b2 = a * ((a + 1.0) - (a - 1.0) * cs - sq) / a0;
        self.a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cs) / a0;
        self.a2 = ((a + 1.0) + (a - 1.0) * cs - sq) / a0;
    }

    pub fn high_shelf(&mut self, freq: f32, gain_db: f32, sr: f32) {
        let a = db_to_gain(gain_db / 2.0);
        let w = TAU * freq.clamp(20.0, sr * 0.45) / sr;
        let (sn, cs) = w.sin_cos();
        let alpha = sn / 2.0 * (2.0f32).sqrt();
        let sq = 2.0 * a.sqrt() * alpha;
        let a0 = (a + 1.0) - (a - 1.0) * cs + sq;
        self.b0 = a * ((a + 1.0) + (a - 1.0) * cs + sq) / a0;
        self.b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cs) / a0;
        self.b2 = a * ((a + 1.0) + (a - 1.0) * cs - sq) / a0;
        self.a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cs) / a0;
        self.a2 = ((a + 1.0) - (a - 1.0) * cs - sq) / a0;
    }

    pub fn peaking(&mut self, freq: f32, gain_db: f32, q: f32, sr: f32) {
        let a = db_to_gain(gain_db / 2.0);
        let w = TAU * freq.clamp(20.0, sr * 0.45) / sr;
        let (sn, cs) = w.sin_cos();
        let alpha = sn / (2.0 * q.max(0.05));
        let a0 = 1.0 + alpha / a;
        self.b0 = (1.0 + alpha * a) / a0;
        self.b1 = -2.0 * cs / a0;
        self.b2 = (1.0 - alpha * a) / a0;
        self.a1 = -2.0 * cs / a0;
        self.a2 = (1.0 - alpha / a) / a0;
    }

    pub fn low_pass(&mut self, freq: f32, q: f32, sr: f32) {
        let w = TAU * freq.clamp(20.0, sr * 0.45) / sr;
        let (sn, cs) = w.sin_cos();
        let alpha = sn / (2.0 * q.max(0.05));
        let a0 = 1.0 + alpha;
        self.b0 = (1.0 - cs) / 2.0 / a0;
        self.b1 = (1.0 - cs) / a0;
        self.b2 = self.b0;
        self.a1 = -2.0 * cs / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        let y = if y.is_finite() { y } else { 0.0 };
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// Soft saturation, gentler than a hard clip and much nicer on transients.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    x.tanh()
}

#[inline]
pub fn hard_clip(x: f32) -> f32 {
    x.clamp(-1.0, 1.0)
}

/// Foldback distortion: reflects the signal back inside the rails.
#[inline]
pub fn foldback(x: f32, threshold: f32) -> f32 {
    let t = threshold.max(0.01);
    if x.abs() <= t {
        return x;
    }
    let mut v = x;
    for _ in 0..4 {
        if v.abs() <= t {
            break;
        }
        v = (v.signum() * t * 2.0) - v;
    }
    v
}
