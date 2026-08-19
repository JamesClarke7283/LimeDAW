//! Insert effects. Each variant carries its own parameter block so a patch
//! round-trips through JSON without a separate parameter table.

use serde::{Deserialize, Serialize};

use super::FilterKind;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReverbParams {
    /// Tail length, 0..1.
    pub size: f32,
    /// High-frequency damping, 0..1.
    pub damping: f32,
    /// Stereo width, 0..1.
    pub width: f32,
    /// Pre-delay in milliseconds.
    pub predelay: f32,
    /// Wet/dry, 0..1.
    pub mix: f32,
}

impl Default for ReverbParams {
    fn default() -> Self {
        Self { size: 0.6, damping: 0.45, width: 0.9, predelay: 12.0, mix: 0.25 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DelayParams {
    /// Delay time in beats when `sync` is on, else in seconds.
    pub time: f32,
    pub sync: bool,
    pub feedback: f32,
    pub ping_pong: bool,
    /// Low-pass in the feedback path, in Hz.
    pub damping: f32,
    pub mix: f32,
}

impl Default for DelayParams {
    fn default() -> Self {
        Self { time: 0.375, sync: true, feedback: 0.38, ping_pong: true, damping: 6_000.0, mix: 0.22 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChorusParams {
    pub rate: f32,
    pub depth: f32,
    pub voices: u8,
    pub spread: f32,
    pub mix: f32,
}

impl Default for ChorusParams {
    fn default() -> Self {
        Self { rate: 0.8, depth: 0.35, voices: 3, spread: 0.7, mix: 0.35 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DistortionParams {
    pub drive: f32,
    /// 0 = soft clip, 1 = hard clip, 2 = foldback.
    pub shape: u8,
    pub tone: f32,
    pub output: f32,
    pub mix: f32,
}

impl Default for DistortionParams {
    fn default() -> Self {
        Self { drive: 0.35, shape: 0, tone: 0.6, output: 0.8, mix: 1.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FilterParams {
    pub kind: FilterKind,
    pub cutoff: f32,
    pub resonance: f32,
    /// Envelope-follower amount, 0..1.
    pub env_follow: f32,
    pub mix: f32,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self { kind: FilterKind::LowPass, cutoff: 8_000.0, resonance: 0.2, env_follow: 0.0, mix: 1.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqParams {
    /// Gains in decibels.
    pub low_gain: f32,
    pub mid_gain: f32,
    pub high_gain: f32,
    pub low_freq: f32,
    pub mid_freq: f32,
    pub high_freq: f32,
    pub mid_q: f32,
}

impl Default for EqParams {
    fn default() -> Self {
        Self {
            low_gain: 0.0,
            mid_gain: 0.0,
            high_gain: 0.0,
            low_freq: 180.0,
            mid_freq: 1_200.0,
            high_freq: 6_000.0,
            mid_q: 0.9,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CompressorParams {
    /// Threshold in decibels.
    pub threshold: f32,
    pub ratio: f32,
    /// Attack and release in milliseconds.
    pub attack: f32,
    pub release: f32,
    /// Makeup gain in decibels.
    pub makeup: f32,
    /// Soft-knee width in decibels.
    pub knee: f32,
    pub mix: f32,
}

impl Default for CompressorParams {
    fn default() -> Self {
        Self { threshold: -18.0, ratio: 4.0, attack: 8.0, release: 120.0, makeup: 0.0, knee: 6.0, mix: 1.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LimiterParams {
    pub ceiling: f32,
    pub release: f32,
}

impl Default for LimiterParams {
    fn default() -> Self {
        Self { ceiling: -0.3, release: 60.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BitcrushParams {
    /// Bit depth, 1..16.
    pub bits: f32,
    /// Sample-rate reduction factor, 1..64.
    pub downsample: f32,
    pub mix: f32,
}

impl Default for BitcrushParams {
    fn default() -> Self {
        Self { bits: 10.0, downsample: 2.0, mix: 0.6 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PhaserParams {
    pub rate: f32,
    pub depth: f32,
    pub feedback: f32,
    pub stages: u8,
    pub mix: f32,
}

impl Default for PhaserParams {
    fn default() -> Self {
        Self { rate: 0.4, depth: 0.7, feedback: 0.4, stages: 6, mix: 0.5 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FlangerParams {
    pub rate: f32,
    pub depth: f32,
    pub feedback: f32,
    pub mix: f32,
}

impl Default for FlangerParams {
    fn default() -> Self {
        Self { rate: 0.25, depth: 0.6, feedback: 0.5, mix: 0.4 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GateParams {
    pub threshold: f32,
    pub attack: f32,
    pub hold: f32,
    pub release: f32,
}

impl Default for GateParams {
    fn default() -> Self {
        Self { threshold: -45.0, attack: 1.0, hold: 20.0, release: 80.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StereoParams {
    /// 0 = mono, 1 = unchanged, 2 = double width.
    pub width: f32,
    /// Frequency below which the signal is collapsed to mono.
    pub bass_mono: f32,
}

impl Default for StereoParams {
    fn default() -> Self {
        Self { width: 1.0, bass_mono: 0.0 }
    }
}

/// Every effect kind, with its parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EffectKind {
    Reverb(ReverbParams),
    Delay(DelayParams),
    Chorus(ChorusParams),
    Distortion(DistortionParams),
    Filter(FilterParams),
    Eq(EqParams),
    Compressor(CompressorParams),
    Limiter(LimiterParams),
    Bitcrush(BitcrushParams),
    Phaser(PhaserParams),
    Flanger(FlangerParams),
    Gate(GateParams),
    Stereo(StereoParams),
}

impl EffectKind {
    pub fn name(&self) -> &'static str {
        match self {
            EffectKind::Reverb(_) => "Reverb",
            EffectKind::Delay(_) => "Delay",
            EffectKind::Chorus(_) => "Chorus",
            EffectKind::Distortion(_) => "Distortion",
            EffectKind::Filter(_) => "Filter",
            EffectKind::Eq(_) => "EQ",
            EffectKind::Compressor(_) => "Compressor",
            EffectKind::Limiter(_) => "Limiter",
            EffectKind::Bitcrush(_) => "Bitcrush",
            EffectKind::Phaser(_) => "Phaser",
            EffectKind::Flanger(_) => "Flanger",
            EffectKind::Gate(_) => "Gate",
            EffectKind::Stereo(_) => "Stereo",
        }
    }

    /// The identifier the MCP server and the "add effect" menu use.
    pub fn slug(&self) -> &'static str {
        match self {
            EffectKind::Reverb(_) => "reverb",
            EffectKind::Delay(_) => "delay",
            EffectKind::Chorus(_) => "chorus",
            EffectKind::Distortion(_) => "distortion",
            EffectKind::Filter(_) => "filter",
            EffectKind::Eq(_) => "eq",
            EffectKind::Compressor(_) => "compressor",
            EffectKind::Limiter(_) => "limiter",
            EffectKind::Bitcrush(_) => "bitcrush",
            EffectKind::Phaser(_) => "phaser",
            EffectKind::Flanger(_) => "flanger",
            EffectKind::Gate(_) => "gate",
            EffectKind::Stereo(_) => "stereo",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Some(match slug {
            "reverb" => EffectKind::Reverb(Default::default()),
            "delay" => EffectKind::Delay(Default::default()),
            "chorus" => EffectKind::Chorus(Default::default()),
            "distortion" => EffectKind::Distortion(Default::default()),
            "filter" => EffectKind::Filter(Default::default()),
            "eq" => EffectKind::Eq(Default::default()),
            "compressor" => EffectKind::Compressor(Default::default()),
            "limiter" => EffectKind::Limiter(Default::default()),
            "bitcrush" => EffectKind::Bitcrush(Default::default()),
            "phaser" => EffectKind::Phaser(Default::default()),
            "flanger" => EffectKind::Flanger(Default::default()),
            "gate" => EffectKind::Gate(Default::default()),
            "stereo" => EffectKind::Stereo(Default::default()),
            _ => return None,
        })
    }

    pub const SLUGS: [&'static str; 13] = [
        "reverb",
        "delay",
        "chorus",
        "distortion",
        "filter",
        "eq",
        "compressor",
        "limiter",
        "bitcrush",
        "phaser",
        "flanger",
        "gate",
        "stereo",
    ];

    /// Named parameters, so the UI and MCP can address them uniformly.
    pub fn params(&self) -> Vec<(&'static str, f32, f32, f32)> {
        match *self {
            EffectKind::Reverb(p) => vec![
                ("size", p.size, 0.0, 1.0),
                ("damping", p.damping, 0.0, 1.0),
                ("width", p.width, 0.0, 1.0),
                ("predelay", p.predelay, 0.0, 200.0),
                ("mix", p.mix, 0.0, 1.0),
            ],
            EffectKind::Delay(p) => vec![
                ("time", p.time, 0.01, 4.0),
                ("sync", if p.sync { 1.0 } else { 0.0 }, 0.0, 1.0),
                ("feedback", p.feedback, 0.0, 0.98),
                ("ping_pong", if p.ping_pong { 1.0 } else { 0.0 }, 0.0, 1.0),
                ("damping", p.damping, 200.0, 20_000.0),
                ("mix", p.mix, 0.0, 1.0),
            ],
            EffectKind::Chorus(p) => vec![
                ("rate", p.rate, 0.01, 10.0),
                ("depth", p.depth, 0.0, 1.0),
                ("voices", p.voices as f32, 1.0, 6.0),
                ("spread", p.spread, 0.0, 1.0),
                ("mix", p.mix, 0.0, 1.0),
            ],
            EffectKind::Distortion(p) => vec![
                ("drive", p.drive, 0.0, 1.0),
                ("shape", p.shape as f32, 0.0, 2.0),
                ("tone", p.tone, 0.0, 1.0),
                ("output", p.output, 0.0, 2.0),
                ("mix", p.mix, 0.0, 1.0),
            ],
            EffectKind::Filter(p) => vec![
                ("kind", p.kind as u8 as f32, 0.0, 4.0),
                ("cutoff", p.cutoff, 20.0, 20_000.0),
                ("resonance", p.resonance, 0.0, 0.98),
                ("env_follow", p.env_follow, 0.0, 1.0),
                ("mix", p.mix, 0.0, 1.0),
            ],
            EffectKind::Eq(p) => vec![
                ("low_gain", p.low_gain, -24.0, 24.0),
                ("mid_gain", p.mid_gain, -24.0, 24.0),
                ("high_gain", p.high_gain, -24.0, 24.0),
                ("low_freq", p.low_freq, 20.0, 1_000.0),
                ("mid_freq", p.mid_freq, 100.0, 10_000.0),
                ("high_freq", p.high_freq, 1_000.0, 20_000.0),
                ("mid_q", p.mid_q, 0.1, 8.0),
            ],
            EffectKind::Compressor(p) => vec![
                ("threshold", p.threshold, -60.0, 0.0),
                ("ratio", p.ratio, 1.0, 20.0),
                ("attack", p.attack, 0.1, 200.0),
                ("release", p.release, 5.0, 2_000.0),
                ("makeup", p.makeup, -12.0, 24.0),
                ("knee", p.knee, 0.0, 24.0),
                ("mix", p.mix, 0.0, 1.0),
            ],
            EffectKind::Limiter(p) => vec![
                ("ceiling", p.ceiling, -12.0, 0.0),
                ("release", p.release, 5.0, 500.0),
            ],
            EffectKind::Bitcrush(p) => vec![
                ("bits", p.bits, 1.0, 16.0),
                ("downsample", p.downsample, 1.0, 64.0),
                ("mix", p.mix, 0.0, 1.0),
            ],
            EffectKind::Phaser(p) => vec![
                ("rate", p.rate, 0.01, 10.0),
                ("depth", p.depth, 0.0, 1.0),
                ("feedback", p.feedback, 0.0, 0.95),
                ("stages", p.stages as f32, 2.0, 12.0),
                ("mix", p.mix, 0.0, 1.0),
            ],
            EffectKind::Flanger(p) => vec![
                ("rate", p.rate, 0.01, 10.0),
                ("depth", p.depth, 0.0, 1.0),
                ("feedback", p.feedback, -0.95, 0.95),
                ("mix", p.mix, 0.0, 1.0),
            ],
            EffectKind::Gate(p) => vec![
                ("threshold", p.threshold, -90.0, 0.0),
                ("attack", p.attack, 0.1, 100.0),
                ("hold", p.hold, 0.0, 500.0),
                ("release", p.release, 1.0, 1_000.0),
            ],
            EffectKind::Stereo(p) => vec![
                ("width", p.width, 0.0, 2.0),
                ("bass_mono", p.bass_mono, 0.0, 500.0),
            ],
        }
    }

    /// Set a parameter by name. Returns false if the name is unknown.
    pub fn set_param(&mut self, name: &str, value: f32) -> bool {
        macro_rules! set {
            ($p:expr, $field:ident, $lo:expr, $hi:expr) => {{
                $p.$field = value.clamp($lo, $hi);
                true
            }};
        }
        match self {
            EffectKind::Reverb(p) => match name {
                "size" => set!(p, size, 0.0, 1.0),
                "damping" => set!(p, damping, 0.0, 1.0),
                "width" => set!(p, width, 0.0, 1.0),
                "predelay" => set!(p, predelay, 0.0, 200.0),
                "mix" => set!(p, mix, 0.0, 1.0),
                _ => false,
            },
            EffectKind::Delay(p) => match name {
                "time" => set!(p, time, 0.01, 4.0),
                "sync" => {
                    p.sync = value >= 0.5;
                    true
                }
                "feedback" => set!(p, feedback, 0.0, 0.98),
                "ping_pong" => {
                    p.ping_pong = value >= 0.5;
                    true
                }
                "damping" => set!(p, damping, 200.0, 20_000.0),
                "mix" => set!(p, mix, 0.0, 1.0),
                _ => false,
            },
            EffectKind::Chorus(p) => match name {
                "rate" => set!(p, rate, 0.01, 10.0),
                "depth" => set!(p, depth, 0.0, 1.0),
                "voices" => {
                    p.voices = value.clamp(1.0, 6.0) as u8;
                    true
                }
                "spread" => set!(p, spread, 0.0, 1.0),
                "mix" => set!(p, mix, 0.0, 1.0),
                _ => false,
            },
            EffectKind::Distortion(p) => match name {
                "drive" => set!(p, drive, 0.0, 1.0),
                "shape" => {
                    p.shape = value.clamp(0.0, 2.0) as u8;
                    true
                }
                "tone" => set!(p, tone, 0.0, 1.0),
                "output" => set!(p, output, 0.0, 2.0),
                "mix" => set!(p, mix, 0.0, 1.0),
                _ => false,
            },
            EffectKind::Filter(p) => match name {
                "kind" => {
                    p.kind = match value.clamp(0.0, 4.0) as u8 {
                        0 => FilterKind::LowPass,
                        1 => FilterKind::HighPass,
                        2 => FilterKind::BandPass,
                        3 => FilterKind::Notch,
                        _ => FilterKind::Off,
                    };
                    true
                }
                "cutoff" => set!(p, cutoff, 20.0, 20_000.0),
                "resonance" => set!(p, resonance, 0.0, 0.98),
                "env_follow" => set!(p, env_follow, 0.0, 1.0),
                "mix" => set!(p, mix, 0.0, 1.0),
                _ => false,
            },
            EffectKind::Eq(p) => match name {
                "low_gain" => set!(p, low_gain, -24.0, 24.0),
                "mid_gain" => set!(p, mid_gain, -24.0, 24.0),
                "high_gain" => set!(p, high_gain, -24.0, 24.0),
                "low_freq" => set!(p, low_freq, 20.0, 1_000.0),
                "mid_freq" => set!(p, mid_freq, 100.0, 10_000.0),
                "high_freq" => set!(p, high_freq, 1_000.0, 20_000.0),
                "mid_q" => set!(p, mid_q, 0.1, 8.0),
                _ => false,
            },
            EffectKind::Compressor(p) => match name {
                "threshold" => set!(p, threshold, -60.0, 0.0),
                "ratio" => set!(p, ratio, 1.0, 20.0),
                "attack" => set!(p, attack, 0.1, 200.0),
                "release" => set!(p, release, 5.0, 2_000.0),
                "makeup" => set!(p, makeup, -12.0, 24.0),
                "knee" => set!(p, knee, 0.0, 24.0),
                "mix" => set!(p, mix, 0.0, 1.0),
                _ => false,
            },
            EffectKind::Limiter(p) => match name {
                "ceiling" => set!(p, ceiling, -12.0, 0.0),
                "release" => set!(p, release, 5.0, 500.0),
                _ => false,
            },
            EffectKind::Bitcrush(p) => match name {
                "bits" => set!(p, bits, 1.0, 16.0),
                "downsample" => set!(p, downsample, 1.0, 64.0),
                "mix" => set!(p, mix, 0.0, 1.0),
                _ => false,
            },
            EffectKind::Phaser(p) => match name {
                "rate" => set!(p, rate, 0.01, 10.0),
                "depth" => set!(p, depth, 0.0, 1.0),
                "feedback" => set!(p, feedback, 0.0, 0.95),
                "stages" => {
                    p.stages = value.clamp(2.0, 12.0) as u8;
                    true
                }
                "mix" => set!(p, mix, 0.0, 1.0),
                _ => false,
            },
            EffectKind::Flanger(p) => match name {
                "rate" => set!(p, rate, 0.01, 10.0),
                "depth" => set!(p, depth, 0.0, 1.0),
                "feedback" => set!(p, feedback, -0.95, 0.95),
                "mix" => set!(p, mix, 0.0, 1.0),
                _ => false,
            },
            EffectKind::Gate(p) => match name {
                "threshold" => set!(p, threshold, -90.0, 0.0),
                "attack" => set!(p, attack, 0.1, 100.0),
                "hold" => set!(p, hold, 0.0, 500.0),
                "release" => set!(p, release, 1.0, 1_000.0),
                _ => false,
            },
            EffectKind::Stereo(p) => match name {
                "width" => set!(p, width, 0.0, 2.0),
                "bass_mono" => set!(p, bass_mono, 0.0, 500.0),
                _ => false,
            },
        }
    }

    fn sanitise(&mut self) {
        let params: Vec<(&'static str, f32, f32, f32)> = self.params();
        for (name, value, lo, hi) in params {
            let clamped = if value.is_finite() { value.clamp(lo, hi) } else { lo };
            if clamped != value {
                let _ = self.set_param(name, clamped);
            }
        }
    }
}

/// An effect slot: a kind plus its bypass state and wet level.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub kind: EffectKind,
    pub enabled: bool,
}

impl Effect {
    pub fn new(kind: EffectKind) -> Self {
        Self { kind, enabled: true }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        EffectKind::from_slug(slug).map(Self::new)
    }

    pub fn name(&self) -> &'static str {
        self.kind.name()
    }

    pub fn sanitise(&mut self) {
        self.kind.sanitise();
    }
}
