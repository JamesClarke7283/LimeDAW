//! Runtime effect processors. Each one mirrors a variant of
//! [`crate::model::EffectKind`] and is rebuilt when the model changes shape.

use crate::audio::dsp::*;
use crate::model::{
    BitcrushParams, ChorusParams, CompressorParams, DelayParams, DistortionParams, EffectKind,
    EqParams, FilterParams, FlangerParams, GateParams, LimiterParams, PhaserParams, ReverbParams,
    StereoParams,
};

/// Stereo comb/all-pass tunings, scaled from the classic Freeverb constants at
/// 44.1 kHz to whatever the device actually runs at.
const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

pub struct Reverb {
    combs_l: Vec<Comb>,
    combs_r: Vec<Comb>,
    aps_l: Vec<AllPassDelay>,
    aps_r: Vec<AllPassDelay>,
    predelay_l: DelayLine,
    predelay_r: DelayLine,
}

impl Reverb {
    fn new(sr: f32) -> Self {
        let scale = sr / 44_100.0;
        let s = |n: usize| ((n as f32 * scale) as usize).max(4);
        Self {
            combs_l: COMB_TUNING.iter().map(|&n| Comb::new(s(n))).collect(),
            combs_r: COMB_TUNING.iter().map(|&n| Comb::new(s(n + STEREO_SPREAD))).collect(),
            aps_l: ALLPASS_TUNING.iter().map(|&n| AllPassDelay::new(s(n), 0.5)).collect(),
            aps_r: ALLPASS_TUNING.iter().map(|&n| AllPassDelay::new(s(n + STEREO_SPREAD), 0.5)).collect(),
            predelay_l: DelayLine::new((sr * 0.25) as usize),
            predelay_r: DelayLine::new((sr * 0.25) as usize),
        }
    }

    fn process(&mut self, l: f32, r: f32, p: &ReverbParams, sr: f32) -> (f32, f32) {
        let pd = (p.predelay * 0.001 * sr).max(1.0);
        self.predelay_l.write(l);
        self.predelay_r.write(r);
        let dl = self.predelay_l.read(pd);
        let dr = self.predelay_r.read(pd);

        let input = (dl + dr) * 0.5 * 0.015;
        let feedback = 0.7 + p.size.clamp(0.0, 1.0) * 0.28;
        let damp = p.damping.clamp(0.0, 1.0) * 0.4;

        let mut out_l = 0.0;
        let mut out_r = 0.0;
        for c in &mut self.combs_l {
            out_l += c.process(input, feedback, damp);
        }
        for c in &mut self.combs_r {
            out_r += c.process(input, feedback, damp);
        }
        for a in &mut self.aps_l {
            out_l = a.process(out_l);
        }
        for a in &mut self.aps_r {
            out_r = a.process(out_r);
        }

        // Width control blends the two channels back toward mono.
        let w = p.width.clamp(0.0, 1.0);
        let mid = (out_l + out_r) * 0.5;
        let wet_l = mid + (out_l - mid) * w;
        let wet_r = mid + (out_r - mid) * w;

        let mix = p.mix.clamp(0.0, 1.0);
        (l * (1.0 - mix) + wet_l * mix * 3.0, r * (1.0 - mix) + wet_r * mix * 3.0)
    }
}

pub struct Delay {
    line_l: DelayLine,
    line_r: DelayLine,
    damp_l: OnePole,
    damp_r: OnePole,
    time: Smoothed,
    /// The first block jumps straight to the delay time instead of gliding up
    /// from an arbitrary starting value.
    primed: bool,
}

impl Delay {
    fn new(sr: f32) -> Self {
        let max = (sr * 8.0) as usize;
        let mut time = Smoothed::new(sr * 0.25);
        time.set_time(0.15, sr);
        Self {
            line_l: DelayLine::new(max),
            line_r: DelayLine::new(max),
            damp_l: OnePole::default(),
            damp_r: OnePole::default(),
            time,
            primed: false,
        }
    }

    fn process(&mut self, l: f32, r: f32, p: &DelayParams, sr: f32, spb: f32) -> (f32, f32) {
        let target = if p.sync { p.time * spb * sr } else { p.time * sr };
        let target = target.clamp(8.0, self.line_l.capacity() as f32 - 4.0);
        if !self.primed {
            self.time.jump(target);
            self.primed = true;
        }
        self.time.set_target(target);
        let d = self.time.next();

        let read_l = self.line_l.read(d);
        let read_r = self.line_r.read(d);
        let damped_l = self.damp_l.process(read_l, p.damping, sr);
        let damped_r = self.damp_r.process(read_r, p.damping, sr);
        let fb = p.feedback.clamp(0.0, 0.98);

        if p.ping_pong {
            self.line_l.write(l + damped_r * fb);
            self.line_r.write(r + damped_l * fb);
        } else {
            self.line_l.write(l + damped_l * fb);
            self.line_r.write(r + damped_r * fb);
        }

        let mix = p.mix.clamp(0.0, 1.0);
        (l + read_l * mix, r + read_r * mix)
    }
}

pub struct Chorus {
    line_l: DelayLine,
    line_r: DelayLine,
    phase: f32,
}

impl Chorus {
    fn new(sr: f32) -> Self {
        let max = (sr * 0.1) as usize;
        Self { line_l: DelayLine::new(max), line_r: DelayLine::new(max), phase: 0.0 }
    }

    fn process(&mut self, l: f32, r: f32, p: &ChorusParams, sr: f32) -> (f32, f32) {
        self.line_l.write(l);
        self.line_r.write(r);
        self.phase += p.rate.clamp(0.01, 10.0) / sr;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        let voices = p.voices.clamp(1, 6) as usize;
        let base = 0.012 * sr;
        let depth = p.depth.clamp(0.0, 1.0) * 0.008 * sr;
        let mut wet_l = 0.0;
        let mut wet_r = 0.0;
        for v in 0..voices {
            let offset = v as f32 / voices as f32;
            let lfo_l = ((self.phase + offset) * TAU).sin();
            let lfo_r = ((self.phase + offset + p.spread.clamp(0.0, 1.0) * 0.5) * TAU).sin();
            wet_l += self.line_l.read(base + depth * lfo_l);
            wet_r += self.line_r.read(base + depth * lfo_r);
        }
        let norm = 1.0 / voices as f32;
        wet_l *= norm;
        wet_r *= norm;

        let mix = p.mix.clamp(0.0, 1.0);
        (l * (1.0 - mix * 0.5) + wet_l * mix, r * (1.0 - mix * 0.5) + wet_r * mix)
    }
}

pub struct Distortion {
    tone_l: OnePole,
    tone_r: OnePole,
}

impl Distortion {
    fn new() -> Self {
        Self { tone_l: OnePole::default(), tone_r: OnePole::default() }
    }

    fn shape(x: f32, kind: u8) -> f32 {
        match kind {
            0 => soft_clip(x),
            1 => hard_clip(x),
            _ => foldback(x, 0.7),
        }
    }

    fn process(&mut self, l: f32, r: f32, p: &DistortionParams, sr: f32) -> (f32, f32) {
        let drive = 1.0 + p.drive.clamp(0.0, 1.0) * 40.0;
        let mut wl = Self::shape(l * drive, p.shape);
        let mut wr = Self::shape(r * drive, p.shape);
        // Tone rolls off the fizz the drive adds.
        let cutoff = 400.0 * (60.0f32).powf(p.tone.clamp(0.0, 1.0));
        wl = self.tone_l.process(wl, cutoff, sr);
        wr = self.tone_r.process(wr, cutoff, sr);
        wl *= p.output.clamp(0.0, 2.0);
        wr *= p.output.clamp(0.0, 2.0);
        let mix = p.mix.clamp(0.0, 1.0);
        (l * (1.0 - mix) + wl * mix, r * (1.0 - mix) + wr * mix)
    }
}

pub struct FilterFx {
    svf_l: Svf,
    svf_r: Svf,
    follower: OnePole,
}

impl FilterFx {
    fn new() -> Self {
        Self { svf_l: Svf::default(), svf_r: Svf::default(), follower: OnePole::default() }
    }

    fn process(&mut self, l: f32, r: f32, p: &FilterParams, sr: f32) -> (f32, f32) {
        let env = self.follower.process((l.abs() + r.abs()) * 0.5, 12.0, sr);
        let cutoff = if p.env_follow > 0.0 {
            p.cutoff * (1.0 + env * p.env_follow * 12.0)
        } else {
            p.cutoff
        };
        let wl = self.svf_l.process(l, p.kind, cutoff, p.resonance, sr);
        let wr = self.svf_r.process(r, p.kind, cutoff, p.resonance, sr);
        let mix = p.mix.clamp(0.0, 1.0);
        (l * (1.0 - mix) + wl * mix, r * (1.0 - mix) + wr * mix)
    }
}

pub struct Eq {
    low_l: Biquad,
    low_r: Biquad,
    mid_l: Biquad,
    mid_r: Biquad,
    high_l: Biquad,
    high_r: Biquad,
    cached: EqParams,
    sr: f32,
}

impl Eq {
    fn new(sr: f32) -> Self {
        let mut eq = Self {
            low_l: Biquad::default(),
            low_r: Biquad::default(),
            mid_l: Biquad::default(),
            mid_r: Biquad::default(),
            high_l: Biquad::default(),
            high_r: Biquad::default(),
            cached: EqParams { low_gain: f32::NAN, ..Default::default() },
            sr,
        };
        eq.retune(&EqParams::default());
        eq
    }

    fn retune(&mut self, p: &EqParams) {
        self.low_l.low_shelf(p.low_freq, p.low_gain, self.sr);
        self.low_r.low_shelf(p.low_freq, p.low_gain, self.sr);
        self.mid_l.peaking(p.mid_freq, p.mid_gain, p.mid_q, self.sr);
        self.mid_r.peaking(p.mid_freq, p.mid_gain, p.mid_q, self.sr);
        self.high_l.high_shelf(p.high_freq, p.high_gain, self.sr);
        self.high_r.high_shelf(p.high_freq, p.high_gain, self.sr);
        self.cached = *p;
    }

    fn process(&mut self, l: f32, r: f32, p: &EqParams) -> (f32, f32) {
        if *p != self.cached {
            self.retune(p);
        }
        let out_l = self.high_l.process(self.mid_l.process(self.low_l.process(l)));
        let out_r = self.high_r.process(self.mid_r.process(self.low_r.process(r)));
        (out_l, out_r)
    }
}

pub struct Compressor {
    env: f32,
    gain_reduction: f32,
}

impl Compressor {
    fn new() -> Self {
        Self { env: 0.0, gain_reduction: 1.0 }
    }

    fn process(&mut self, l: f32, r: f32, p: &CompressorParams, sr: f32) -> (f32, f32) {
        let detect = (l.abs()).max(r.abs());
        let attack = (-1.0 / (p.attack.max(0.1) * 0.001 * sr)).exp();
        let release = (-1.0 / (p.release.max(1.0) * 0.001 * sr)).exp();
        let coeff = if detect > self.env { attack } else { release };
        self.env = detect + coeff * (self.env - detect);
        if !self.env.is_finite() {
            self.env = 0.0;
        }

        let level_db = gain_to_db(self.env.max(1e-9));
        let over = level_db - p.threshold;
        let knee = p.knee.max(0.0);
        let reduction_db = if knee > 0.0 && over > -knee / 2.0 && over < knee / 2.0 {
            // Soft knee: quadratic interpolation across the knee width.
            let x = over + knee / 2.0;
            (1.0 / p.ratio.max(1.0) - 1.0) * x * x / (2.0 * knee)
        } else if over > 0.0 {
            over * (1.0 / p.ratio.max(1.0) - 1.0)
        } else {
            0.0
        };

        self.gain_reduction = db_to_gain(reduction_db);
        let makeup = db_to_gain(p.makeup);
        let g = self.gain_reduction * makeup;
        let mix = p.mix.clamp(0.0, 1.0);
        (l * (1.0 - mix) + l * g * mix, r * (1.0 - mix) + r * g * mix)
    }

    /// Current gain reduction in decibels, for the mixer's meter.
    pub fn reduction_db(&self) -> f32 {
        gain_to_db(self.gain_reduction)
    }
}

pub struct Limiter {
    env: f32,
    /// A short look-ahead keeps fast transients from poking through.
    delay_l: DelayLine,
    delay_r: DelayLine,
    lookahead: f32,
}

impl Limiter {
    fn new(sr: f32) -> Self {
        let la = (sr * 0.003).max(2.0);
        Self {
            env: 0.0,
            delay_l: DelayLine::new(la as usize + 8),
            delay_r: DelayLine::new(la as usize + 8),
            lookahead: la,
        }
    }

    fn process(&mut self, l: f32, r: f32, p: &LimiterParams, sr: f32) -> (f32, f32) {
        let ceiling = db_to_gain(p.ceiling.clamp(-24.0, 0.0));
        let peak = l.abs().max(r.abs());
        let target = if peak > ceiling { ceiling / peak.max(1e-9) } else { 1.0 };
        let release = (-1.0 / (p.release.max(1.0) * 0.001 * sr)).exp();
        if target < self.env || self.env == 0.0 {
            self.env = target;
        } else {
            self.env = target + release * (self.env - target);
        }
        if !self.env.is_finite() {
            self.env = 1.0;
        }
        self.delay_l.write(l);
        self.delay_r.write(r);
        let dl = self.delay_l.read(self.lookahead);
        let dr = self.delay_r.read(self.lookahead);
        (
            (dl * self.env).clamp(-ceiling, ceiling),
            (dr * self.env).clamp(-ceiling, ceiling),
        )
    }
}

pub struct Bitcrush {
    hold_l: f32,
    hold_r: f32,
    counter: f32,
}

impl Bitcrush {
    fn new() -> Self {
        Self { hold_l: 0.0, hold_r: 0.0, counter: 0.0 }
    }

    fn process(&mut self, l: f32, r: f32, p: &BitcrushParams) -> (f32, f32) {
        self.counter += 1.0;
        if self.counter >= p.downsample.max(1.0) {
            self.counter = 0.0;
            // Quantise around zero so signals hotter than full scale are
            // crushed rather than clipped on the way in.
            let steps = (2.0f32).powf(p.bits.clamp(1.0, 16.0)) * 0.5;
            self.hold_l = (l * steps).round() / steps;
            self.hold_r = (r * steps).round() / steps;
        }
        let mix = p.mix.clamp(0.0, 1.0);
        (l * (1.0 - mix) + self.hold_l * mix, r * (1.0 - mix) + self.hold_r * mix)
    }
}

pub struct Phaser {
    stages_l: Vec<AllPass1>,
    stages_r: Vec<AllPass1>,
    phase: f32,
    fb_l: f32,
    fb_r: f32,
}

impl Phaser {
    fn new() -> Self {
        Self {
            stages_l: (0..12).map(|_| AllPass1::default()).collect(),
            stages_r: (0..12).map(|_| AllPass1::default()).collect(),
            phase: 0.0,
            fb_l: 0.0,
            fb_r: 0.0,
        }
    }

    fn process(&mut self, l: f32, r: f32, p: &PhaserParams, sr: f32) -> (f32, f32) {
        self.phase += p.rate.clamp(0.01, 10.0) / sr;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let lfo = (self.phase * TAU).sin() * 0.5 + 0.5;
        let lfo_r = ((self.phase + 0.25) * TAU).sin() * 0.5 + 0.5;
        let stages = p.stages.clamp(2, 12) as usize;
        let depth = p.depth.clamp(0.0, 1.0);

        let sweep = |v: f32| {
            let freq = 200.0 * (40.0f32).powf(v * depth);
            let w = (std::f32::consts::PI * freq / sr).tan();
            (1.0 - w) / (1.0 + w)
        };
        let coeff_l = sweep(lfo);
        let coeff_r = sweep(lfo_r);

        let fb = p.feedback.clamp(0.0, 0.95);
        let mut wl = l + self.fb_l * fb;
        let mut wr = r + self.fb_r * fb;
        for s in self.stages_l.iter_mut().take(stages) {
            wl = s.process(wl, coeff_l);
        }
        for s in self.stages_r.iter_mut().take(stages) {
            wr = s.process(wr, coeff_r);
        }
        self.fb_l = if wl.is_finite() { wl } else { 0.0 };
        self.fb_r = if wr.is_finite() { wr } else { 0.0 };

        let mix = p.mix.clamp(0.0, 1.0);
        (l * (1.0 - mix) + wl * mix, r * (1.0 - mix) + wr * mix)
    }
}

pub struct Flanger {
    line_l: DelayLine,
    line_r: DelayLine,
    phase: f32,
    fb_l: f32,
    fb_r: f32,
}

impl Flanger {
    fn new(sr: f32) -> Self {
        let max = (sr * 0.03) as usize;
        Self {
            line_l: DelayLine::new(max),
            line_r: DelayLine::new(max),
            phase: 0.0,
            fb_l: 0.0,
            fb_r: 0.0,
        }
    }

    fn process(&mut self, l: f32, r: f32, p: &FlangerParams, sr: f32) -> (f32, f32) {
        self.phase += p.rate.clamp(0.01, 10.0) / sr;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let fb = p.feedback.clamp(-0.95, 0.95);
        self.line_l.write(l + self.fb_l * fb);
        self.line_r.write(r + self.fb_r * fb);

        let base = 0.001 * sr;
        let depth = p.depth.clamp(0.0, 1.0) * 0.006 * sr;
        let wl = self.line_l.read(base + depth * ((self.phase * TAU).sin() * 0.5 + 0.5));
        let wr = self.line_r.read(base + depth * (((self.phase + 0.5) * TAU).sin() * 0.5 + 0.5));
        self.fb_l = wl;
        self.fb_r = wr;

        let mix = p.mix.clamp(0.0, 1.0);
        (l * (1.0 - mix * 0.5) + wl * mix, r * (1.0 - mix * 0.5) + wr * mix)
    }
}

pub struct Gate {
    env: f32,
    gain: f32,
    hold_counter: f32,
}

impl Gate {
    fn new() -> Self {
        Self { env: 0.0, gain: 0.0, hold_counter: 0.0 }
    }

    fn process(&mut self, l: f32, r: f32, p: &GateParams, sr: f32) -> (f32, f32) {
        let detect = l.abs().max(r.abs());
        self.env = detect.max(self.env * 0.999);
        let open = gain_to_db(self.env.max(1e-9)) > p.threshold;
        if open {
            self.hold_counter = p.hold * 0.001 * sr;
        } else if self.hold_counter > 0.0 {
            self.hold_counter -= 1.0;
        }
        let target = if open || self.hold_counter > 0.0 { 1.0 } else { 0.0 };
        let time = if target > self.gain { p.attack } else { p.release };
        let coeff = 1.0 - (-1.0 / (time.max(0.1) * 0.001 * sr)).exp();
        self.gain += (target - self.gain) * coeff;
        (l * self.gain, r * self.gain)
    }
}

pub struct Stereo {
    bass_l: Biquad,
    bass_r: Biquad,
    cached_freq: f32,
    sr: f32,
}

impl Stereo {
    fn new(sr: f32) -> Self {
        Self { bass_l: Biquad::default(), bass_r: Biquad::default(), cached_freq: -1.0, sr }
    }

    fn process(&mut self, l: f32, r: f32, p: &StereoParams) -> (f32, f32) {
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.5 * p.width.clamp(0.0, 2.0);
        let (mut out_l, mut out_r) = (mid + side, mid - side);

        if p.bass_mono > 20.0 {
            if (p.bass_mono - self.cached_freq).abs() > 0.5 {
                self.bass_l.low_pass(p.bass_mono, 0.707, self.sr);
                self.bass_r.low_pass(p.bass_mono, 0.707, self.sr);
                self.cached_freq = p.bass_mono;
            }
            // Sum only the low band to mono, leave the rest of the width alone.
            let low_l = self.bass_l.process(out_l);
            let low_r = self.bass_r.process(out_r);
            let low_mono = (low_l + low_r) * 0.5;
            out_l = out_l - low_l + low_mono;
            out_r = out_r - low_r + low_mono;
        }
        (out_l, out_r)
    }
}

/// A runtime effect: state plus a snapshot of its parameters.
pub enum EffectProc {
    Reverb(Box<Reverb>),
    Delay(Box<Delay>),
    Chorus(Box<Chorus>),
    Distortion(Distortion),
    Filter(FilterFx),
    Eq(Box<Eq>),
    Compressor(Compressor),
    Limiter(Box<Limiter>),
    Bitcrush(Bitcrush),
    Phaser(Phaser),
    Flanger(Box<Flanger>),
    Gate(Gate),
    Stereo(Stereo),
}

impl EffectProc {
    /// Build the processor that matches a model effect kind.
    pub fn new(kind: &EffectKind, sr: f32) -> Self {
        match kind {
            EffectKind::Reverb(_) => EffectProc::Reverb(Box::new(Reverb::new(sr))),
            EffectKind::Delay(_) => EffectProc::Delay(Box::new(Delay::new(sr))),
            EffectKind::Chorus(_) => EffectProc::Chorus(Box::new(Chorus::new(sr))),
            EffectKind::Distortion(_) => EffectProc::Distortion(Distortion::new()),
            EffectKind::Filter(_) => EffectProc::Filter(FilterFx::new()),
            EffectKind::Eq(_) => EffectProc::Eq(Box::new(Eq::new(sr))),
            EffectKind::Compressor(_) => EffectProc::Compressor(Compressor::new()),
            EffectKind::Limiter(_) => EffectProc::Limiter(Box::new(Limiter::new(sr))),
            EffectKind::Bitcrush(_) => EffectProc::Bitcrush(Bitcrush::new()),
            EffectKind::Phaser(_) => EffectProc::Phaser(Phaser::new()),
            EffectKind::Flanger(_) => EffectProc::Flanger(Box::new(Flanger::new(sr))),
            EffectKind::Gate(_) => EffectProc::Gate(Gate::new()),
            EffectKind::Stereo(_) => EffectProc::Stereo(Stereo::new(sr)),
        }
    }

    /// Whether this processor still matches the model variant it was built for.
    pub fn matches(&self, kind: &EffectKind) -> bool {
        matches!(
            (self, kind),
            (EffectProc::Reverb(_), EffectKind::Reverb(_))
                | (EffectProc::Delay(_), EffectKind::Delay(_))
                | (EffectProc::Chorus(_), EffectKind::Chorus(_))
                | (EffectProc::Distortion(_), EffectKind::Distortion(_))
                | (EffectProc::Filter(_), EffectKind::Filter(_))
                | (EffectProc::Eq(_), EffectKind::Eq(_))
                | (EffectProc::Compressor(_), EffectKind::Compressor(_))
                | (EffectProc::Limiter(_), EffectKind::Limiter(_))
                | (EffectProc::Bitcrush(_), EffectKind::Bitcrush(_))
                | (EffectProc::Phaser(_), EffectKind::Phaser(_))
                | (EffectProc::Flanger(_), EffectKind::Flanger(_))
                | (EffectProc::Gate(_), EffectKind::Gate(_))
                | (EffectProc::Stereo(_), EffectKind::Stereo(_))
        )
    }

    /// Process one stereo frame. `spb` is seconds per beat, for tempo sync.
    #[inline]
    pub fn process(&mut self, l: f32, r: f32, kind: &EffectKind, sr: f32, spb: f32) -> (f32, f32) {
        match (self, kind) {
            (EffectProc::Reverb(s), EffectKind::Reverb(p)) => s.process(l, r, p, sr),
            (EffectProc::Delay(s), EffectKind::Delay(p)) => s.process(l, r, p, sr, spb),
            (EffectProc::Chorus(s), EffectKind::Chorus(p)) => s.process(l, r, p, sr),
            (EffectProc::Distortion(s), EffectKind::Distortion(p)) => s.process(l, r, p, sr),
            (EffectProc::Filter(s), EffectKind::Filter(p)) => s.process(l, r, p, sr),
            (EffectProc::Eq(s), EffectKind::Eq(p)) => s.process(l, r, p),
            (EffectProc::Compressor(s), EffectKind::Compressor(p)) => s.process(l, r, p, sr),
            (EffectProc::Limiter(s), EffectKind::Limiter(p)) => s.process(l, r, p, sr),
            (EffectProc::Bitcrush(s), EffectKind::Bitcrush(p)) => s.process(l, r, p),
            (EffectProc::Phaser(s), EffectKind::Phaser(p)) => s.process(l, r, p, sr),
            (EffectProc::Flanger(s), EffectKind::Flanger(p)) => s.process(l, r, p, sr),
            (EffectProc::Gate(s), EffectKind::Gate(p)) => s.process(l, r, p, sr),
            (EffectProc::Stereo(s), EffectKind::Stereo(p)) => s.process(l, r, p),
            // Mismatched pair: the chain is about to be rebuilt anyway.
            _ => (l, r),
        }
    }
}

/// A chain of effect processors kept in sync with a model effect list.
#[derive(Default)]
pub struct EffectChain {
    procs: Vec<EffectProc>,
}

impl EffectChain {
    /// Rebuild any slot whose kind no longer matches, leaving the rest alone so
    /// reverb tails survive a knob tweak elsewhere in the chain.
    pub fn sync(&mut self, effects: &[crate::model::Effect], sr: f32) {
        self.procs.truncate(effects.len());
        for (i, e) in effects.iter().enumerate() {
            match self.procs.get(i) {
                Some(p) if p.matches(&e.kind) => {}
                Some(_) => self.procs[i] = EffectProc::new(&e.kind, sr),
                None => self.procs.push(EffectProc::new(&e.kind, sr)),
            }
        }
    }

    #[inline]
    pub fn process(
        &mut self,
        mut l: f32,
        mut r: f32,
        effects: &[crate::model::Effect],
        sr: f32,
        spb: f32,
    ) -> (f32, f32) {
        for (proc, e) in self.procs.iter_mut().zip(effects.iter()) {
            if e.enabled {
                let (nl, nr) = proc.process(l, r, &e.kind, sr, spb);
                l = nl;
                r = nr;
            }
        }
        (l, r)
    }
}
