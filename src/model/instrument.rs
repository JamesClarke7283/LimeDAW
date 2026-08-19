//! Instrument definitions — what a channel actually makes noise with.

use serde::{Deserialize, Serialize};

/// Oscillator shapes available to the built-in synth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Waveform {
    Sine,
    Triangle,
    Saw,
    Square,
    Pulse25,
    Pulse12,
    Noise,
}

impl Waveform {
    pub const ALL: [Waveform; 7] = [
        Waveform::Sine,
        Waveform::Triangle,
        Waveform::Saw,
        Waveform::Square,
        Waveform::Pulse25,
        Waveform::Pulse12,
        Waveform::Noise,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Waveform::Sine => "Sine",
            Waveform::Triangle => "Triangle",
            Waveform::Saw => "Saw",
            Waveform::Square => "Square",
            Waveform::Pulse25 => "Pulse 25%",
            Waveform::Pulse12 => "Pulse 12%",
            Waveform::Noise => "Noise",
        }
    }
}

impl std::fmt::Display for Waveform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// An ADSR envelope. Times are in seconds, sustain is a 0..1 level.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Envelope {
    pub const fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self {
            attack,
            decay,
            sustain,
            release,
        }
    }

    pub fn sanitise(&mut self) {
        self.attack = self.attack.clamp(0.0, 20.0);
        self.decay = self.decay.clamp(0.0, 20.0);
        self.sustain = self.sustain.clamp(0.0, 1.0);
        self.release = self.release.clamp(0.001, 20.0);
    }
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new(0.002, 0.12, 0.75, 0.18)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Off,
}

impl FilterKind {
    pub const ALL: [FilterKind; 5] = [
        FilterKind::LowPass,
        FilterKind::HighPass,
        FilterKind::BandPass,
        FilterKind::Notch,
        FilterKind::Off,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FilterKind::LowPass => "Low pass",
            FilterKind::HighPass => "High pass",
            FilterKind::BandPass => "Band pass",
            FilterKind::Notch => "Notch",
            FilterKind::Off => "Off",
        }
    }
}

impl std::fmt::Display for FilterKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// One of the three oscillators in the built-in synth.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Oscillator {
    pub enabled: bool,
    pub waveform: Waveform,
    /// Pitch offset in semitones.
    pub semitones: f32,
    /// Fine detune in cents.
    pub cents: f32,
    /// Linear level, 0..1.
    pub level: f32,
    /// Number of stacked unison voices, 1..=8.
    pub unison: u8,
    /// Unison spread in cents.
    pub spread: f32,
    /// Stereo width of the unison stack, 0..1.
    pub width: f32,
    /// Phase offset at note-on, 0..1. Zero means free-running.
    pub phase: f32,
}

impl Default for Oscillator {
    fn default() -> Self {
        Self {
            enabled: true,
            waveform: Waveform::Saw,
            semitones: 0.0,
            cents: 0.0,
            level: 0.8,
            unison: 1,
            spread: 12.0,
            width: 0.5,
            phase: 0.0,
        }
    }
}

impl Oscillator {
    fn sanitise(&mut self) {
        self.semitones = self.semitones.clamp(-48.0, 48.0);
        self.cents = self.cents.clamp(-100.0, 100.0);
        self.level = self.level.clamp(0.0, 1.0);
        self.unison = self.unison.clamp(1, 8);
        self.spread = self.spread.clamp(0.0, 100.0);
        self.width = self.width.clamp(0.0, 1.0);
        self.phase = self.phase.clamp(0.0, 1.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LfoTarget {
    Pitch,
    FilterCutoff,
    Amplitude,
    PulseWidth,
    Pan,
}

impl LfoTarget {
    pub const ALL: [LfoTarget; 5] = [
        LfoTarget::Pitch,
        LfoTarget::FilterCutoff,
        LfoTarget::Amplitude,
        LfoTarget::PulseWidth,
        LfoTarget::Pan,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LfoTarget::Pitch => "Pitch",
            LfoTarget::FilterCutoff => "Cutoff",
            LfoTarget::Amplitude => "Volume",
            LfoTarget::PulseWidth => "Pulse width",
            LfoTarget::Pan => "Pan",
        }
    }
}

impl std::fmt::Display for LfoTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Lfo {
    pub enabled: bool,
    pub waveform: Waveform,
    /// Rate in Hz when `sync` is off.
    pub rate: f32,
    /// Modulation depth, 0..1.
    pub depth: f32,
    pub target: LfoTarget,
    /// Fade-in time in seconds.
    pub delay: f32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self {
            enabled: false,
            waveform: Waveform::Sine,
            rate: 5.0,
            depth: 0.2,
            target: LfoTarget::Pitch,
            delay: 0.0,
        }
    }
}

impl Lfo {
    fn sanitise(&mut self) {
        self.rate = self.rate.clamp(0.01, 60.0);
        self.depth = self.depth.clamp(0.0, 1.0);
        self.delay = self.delay.clamp(0.0, 10.0);
    }
}

/// The built-in three-oscillator subtractive synth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SynthPatch {
    pub osc: [Oscillator; 3],
    pub amp_env: Envelope,
    pub filter: FilterKind,
    /// Cutoff in Hz.
    pub cutoff: f32,
    /// Resonance, 0..1.
    pub resonance: f32,
    /// How much the filter envelope moves the cutoff, in octaves.
    pub filter_env_amount: f32,
    pub filter_env: Envelope,
    /// How much note velocity opens the filter, 0..1.
    pub filter_key_track: f32,
    pub lfo: Lfo,
    /// Portamento time in seconds. Non-zero implies mono legato.
    pub glide: f32,
    pub mono: bool,
    /// Output gain, 0..2.
    pub gain: f32,
    /// Analogue-ish drive baked into the voice, 0..1.
    pub drive: f32,
    /// How strongly velocity scales amplitude, 0..1.
    pub velocity_amount: f32,
}

impl Default for SynthPatch {
    fn default() -> Self {
        Self {
            osc: [
                Oscillator::default(),
                Oscillator {
                    enabled: false,
                    ..Oscillator::default()
                },
                Oscillator {
                    enabled: false,
                    waveform: Waveform::Sine,
                    semitones: -12.0,
                    ..Oscillator::default()
                },
            ],
            amp_env: Envelope::default(),
            filter: FilterKind::LowPass,
            cutoff: 12_000.0,
            resonance: 0.12,
            filter_env_amount: 0.0,
            filter_env: Envelope::new(0.005, 0.25, 0.4, 0.2),
            filter_key_track: 0.3,
            lfo: Lfo::default(),
            glide: 0.0,
            mono: false,
            gain: 0.8,
            drive: 0.0,
            velocity_amount: 0.8,
        }
    }
}

impl SynthPatch {
    pub fn sanitise(&mut self) {
        for o in &mut self.osc {
            o.sanitise();
        }
        self.amp_env.sanitise();
        self.filter_env.sanitise();
        self.lfo.sanitise();
        self.cutoff = self.cutoff.clamp(20.0, 20_000.0);
        self.resonance = self.resonance.clamp(0.0, 0.98);
        self.filter_env_amount = self.filter_env_amount.clamp(-8.0, 8.0);
        self.filter_key_track = self.filter_key_track.clamp(0.0, 1.0);
        self.glide = self.glide.clamp(0.0, 5.0);
        self.gain = self.gain.clamp(0.0, 2.0);
        self.drive = self.drive.clamp(0.0, 1.0);
        self.velocity_amount = self.velocity_amount.clamp(0.0, 1.0);
    }

    pub fn bass() -> Self {
        Self {
            osc: [
                Oscillator {
                    waveform: Waveform::Saw,
                    level: 0.85,
                    unison: 2,
                    spread: 8.0,
                    ..Default::default()
                },
                Oscillator {
                    enabled: true,
                    waveform: Waveform::Square,
                    semitones: -12.0,
                    level: 0.5,
                    ..Default::default()
                },
                Oscillator {
                    enabled: false,
                    ..Default::default()
                },
            ],
            amp_env: Envelope::new(0.003, 0.18, 0.65, 0.12),
            cutoff: 1_400.0,
            resonance: 0.28,
            filter_env_amount: 2.2,
            filter_env: Envelope::new(0.002, 0.22, 0.15, 0.15),
            glide: 0.0,
            mono: true,
            drive: 0.25,
            gain: 0.85,
            ..Default::default()
        }
    }

    pub fn lead() -> Self {
        Self {
            osc: [
                Oscillator {
                    waveform: Waveform::Saw,
                    level: 0.7,
                    unison: 5,
                    spread: 18.0,
                    width: 0.8,
                    ..Default::default()
                },
                Oscillator {
                    enabled: true,
                    waveform: Waveform::Saw,
                    cents: -9.0,
                    level: 0.5,
                    unison: 3,
                    spread: 22.0,
                    ..Default::default()
                },
                Oscillator {
                    enabled: false,
                    ..Default::default()
                },
            ],
            amp_env: Envelope::new(0.01, 0.3, 0.7, 0.35),
            cutoff: 6_500.0,
            resonance: 0.2,
            filter_env_amount: 1.4,
            gain: 0.62,
            ..Default::default()
        }
    }

    pub fn pad() -> Self {
        Self {
            osc: [
                Oscillator {
                    waveform: Waveform::Triangle,
                    level: 0.6,
                    unison: 3,
                    spread: 14.0,
                    ..Default::default()
                },
                Oscillator {
                    enabled: true,
                    waveform: Waveform::Saw,
                    semitones: 7.0,
                    level: 0.35,
                    unison: 2,
                    ..Default::default()
                },
                Oscillator {
                    enabled: true,
                    waveform: Waveform::Sine,
                    semitones: -12.0,
                    level: 0.4,
                    ..Default::default()
                },
            ],
            amp_env: Envelope::new(0.35, 0.6, 0.8, 0.9),
            cutoff: 4_200.0,
            resonance: 0.1,
            filter_env_amount: 0.8,
            filter_env: Envelope::new(0.5, 1.2, 0.6, 0.8),
            lfo: Lfo {
                enabled: true,
                rate: 0.35,
                depth: 0.06,
                target: LfoTarget::FilterCutoff,
                ..Default::default()
            },
            gain: 0.55,
            ..Default::default()
        }
    }

    pub fn pluck() -> Self {
        Self {
            osc: [
                Oscillator {
                    waveform: Waveform::Square,
                    level: 0.7,
                    ..Default::default()
                },
                Oscillator {
                    enabled: true,
                    waveform: Waveform::Saw,
                    cents: 7.0,
                    level: 0.4,
                    ..Default::default()
                },
                Oscillator {
                    enabled: false,
                    ..Default::default()
                },
            ],
            amp_env: Envelope::new(0.001, 0.22, 0.0, 0.18),
            cutoff: 8_000.0,
            filter_env_amount: 3.0,
            filter_env: Envelope::new(0.001, 0.16, 0.0, 0.1),
            gain: 0.7,
            ..Default::default()
        }
    }
}

/// How a synthesised drum voice is built. The sampler channel can either play
/// a loaded audio file or one of these procedural one-shots, so a fresh
/// install makes drum sounds without hunting for samples first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrumModel {
    Kick,
    Snare,
    ClosedHat,
    OpenHat,
    Clap,
    Tom,
    Rim,
    Cymbal,
    Cowbell,
}

impl DrumModel {
    pub const ALL: [DrumModel; 9] = [
        DrumModel::Kick,
        DrumModel::Snare,
        DrumModel::ClosedHat,
        DrumModel::OpenHat,
        DrumModel::Clap,
        DrumModel::Tom,
        DrumModel::Rim,
        DrumModel::Cymbal,
        DrumModel::Cowbell,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DrumModel::Kick => "Kick",
            DrumModel::Snare => "Snare",
            DrumModel::ClosedHat => "Closed hat",
            DrumModel::OpenHat => "Open hat",
            DrumModel::Clap => "Clap",
            DrumModel::Tom => "Tom",
            DrumModel::Rim => "Rim",
            DrumModel::Cymbal => "Cymbal",
            DrumModel::Cowbell => "Cowbell",
        }
    }
}

impl std::fmt::Display for DrumModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// The source a sampler channel plays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SampleSource {
    /// A procedurally generated drum hit.
    Model(DrumModel),
    /// An audio file loaded from disk. The samples themselves live in the
    /// engine's sample bank, keyed by this path.
    File { path: String },
}

/// A sampler channel: one-shot or looped playback with pitch and envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SamplerPatch {
    pub source: SampleSource,
    /// The MIDI key at which the sample plays back untransposed.
    pub root_key: u8,
    /// Extra tuning in semitones.
    pub tune: f32,
    /// Fine tuning in cents.
    pub fine: f32,
    pub reverse: bool,
    pub loop_sample: bool,
    /// Start offset into the sample, 0..1.
    pub start: f32,
    /// End point in the sample, 0..1.
    pub end: f32,
    pub envelope: Envelope,
    /// If false, the sample always plays to its end regardless of note length.
    pub gated: bool,
    pub gain: f32,
    pub filter: FilterKind,
    pub cutoff: f32,
    pub resonance: f32,
    pub velocity_amount: f32,
}

impl Default for SamplerPatch {
    fn default() -> Self {
        Self {
            source: SampleSource::Model(DrumModel::Kick),
            root_key: 60,
            tune: 0.0,
            fine: 0.0,
            reverse: false,
            loop_sample: false,
            start: 0.0,
            end: 1.0,
            envelope: Envelope::new(0.0005, 0.0, 1.0, 0.01),
            gated: false,
            gain: 0.9,
            filter: FilterKind::Off,
            cutoff: 20_000.0,
            resonance: 0.0,
            velocity_amount: 0.9,
        }
    }
}

impl SamplerPatch {
    pub fn sanitise(&mut self) {
        self.tune = self.tune.clamp(-48.0, 48.0);
        self.fine = self.fine.clamp(-100.0, 100.0);
        self.start = self.start.clamp(0.0, 1.0);
        self.end = self.end.clamp(self.start, 1.0);
        self.gain = self.gain.clamp(0.0, 2.0);
        self.cutoff = self.cutoff.clamp(20.0, 20_000.0);
        self.resonance = self.resonance.clamp(0.0, 0.98);
        self.velocity_amount = self.velocity_amount.clamp(0.0, 1.0);
        self.envelope.sanitise();
    }

    pub fn from_model(model: DrumModel) -> Self {
        Self {
            source: SampleSource::Model(model),
            ..Default::default()
        }
    }

    pub fn kick() -> Self {
        Self::from_model(DrumModel::Kick)
    }
    pub fn snare() -> Self {
        Self::from_model(DrumModel::Snare)
    }
    pub fn hat() -> Self {
        Self::from_model(DrumModel::ClosedHat)
    }
    pub fn clap() -> Self {
        Self::from_model(DrumModel::Clap)
    }
}

/// A SoundFont program: which bank and preset the channel plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SoundFontPatch {
    pub bank: u16,
    pub preset: u16,
    /// A dedicated MIDI channel is allocated per channel; drums need ch. 9.
    pub percussion: bool,
}

/// What a channel makes sound with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Instrument {
    Synth(SynthPatch),
    Sampler(SamplerPatch),
    SoundFont(SoundFontPatch),
}

impl Instrument {
    pub fn sanitise(&mut self) {
        match self {
            Instrument::Synth(p) => p.sanitise(),
            Instrument::Sampler(p) => p.sanitise(),
            Instrument::SoundFont(_) => {}
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            Instrument::Synth(_) => "Synth",
            Instrument::Sampler(_) => "Sampler",
            Instrument::SoundFont(_) => "SoundFont",
        }
    }
}

// ---------------------------------------------------------------------------
// Uniform parameter access
//
// The UI, the automation system and the MCP server all want to poke at
// instrument parameters by name rather than by field, so every patch exposes
// the same (name, value, min, max) view.
// ---------------------------------------------------------------------------

/// Names for the enumerated instrument parameters, in the order their enums
/// declare them, so an index round-trips to the same variant.
pub const WAVEFORM_NAMES: &[&str] = &[
    "Sine",
    "Triangle",
    "Saw",
    "Square",
    "Pulse 25%",
    "Pulse 12%",
    "Noise",
];
pub const FILTER_NAMES: &[&str] = &["Low pass", "High pass", "Band pass", "Notch", "Off"];
pub const LFO_TARGET_NAMES: &[&str] = &["Pitch", "Cutoff", "Volume", "Pulse width", "Pan"];

/// A named, range-bounded parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamInfo {
    pub name: &'static str,
    pub label: &'static str,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// True when the parameter only lands on whole numbers.
    pub stepped: bool,
    /// True when the parameter is an on/off switch.
    pub toggle: bool,
    /// Set when the parameter picks from a fixed list of names.
    pub choices: Option<&'static [&'static str]>,
    /// A bipolar parameter draws its knob outward from the centre.
    pub bipolar: bool,
}

impl ParamInfo {
    const fn new(name: &'static str, label: &'static str, value: f32, min: f32, max: f32) -> Self {
        Self {
            name,
            label,
            value,
            min,
            max,
            stepped: false,
            toggle: false,
            choices: None,
            bipolar: false,
        }
    }

    const fn bipolar(
        name: &'static str,
        label: &'static str,
        value: f32,
        min: f32,
        max: f32,
    ) -> Self {
        Self {
            bipolar: true,
            ..Self::new(name, label, value, min, max)
        }
    }

    const fn stepped(
        name: &'static str,
        label: &'static str,
        value: f32,
        min: f32,
        max: f32,
    ) -> Self {
        Self {
            stepped: true,
            ..Self::new(name, label, value, min, max)
        }
    }

    const fn switch(name: &'static str, label: &'static str, on: bool) -> Self {
        Self {
            toggle: true,
            stepped: true,
            ..Self::new(name, label, if on { 1.0 } else { 0.0 }, 0.0, 1.0)
        }
    }

    const fn choice(
        name: &'static str,
        label: &'static str,
        value: f32,
        choices: &'static [&'static str],
    ) -> Self {
        Self {
            stepped: true,
            choices: Some(choices),
            ..Self::new(name, label, value, 0.0, choices.len() as f32 - 1.0)
        }
    }

    /// The parameter's position in its range, 0..1.
    pub fn normalised(&self) -> f32 {
        if (self.max - self.min).abs() < f32::EPSILON {
            0.0
        } else {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }

    pub fn denormalise(&self, t: f32) -> f32 {
        self.min + (self.max - self.min) * t.clamp(0.0, 1.0)
    }
}

impl SynthPatch {
    pub fn params(&self) -> Vec<ParamInfo> {
        let mut v = Vec::with_capacity(40);
        for (i, o) in self.osc.iter().enumerate() {
            let n = i + 1;
            // Names are built from a fixed set, so the leak-free static strings
            // below stay exhaustive.
            let (enabled, wave, semis, cents, level, unison, spread, width) = match n {
                1 => (
                    "osc1_enabled",
                    "osc1_wave",
                    "osc1_semitones",
                    "osc1_cents",
                    "osc1_level",
                    "osc1_unison",
                    "osc1_spread",
                    "osc1_width",
                ),
                2 => (
                    "osc2_enabled",
                    "osc2_wave",
                    "osc2_semitones",
                    "osc2_cents",
                    "osc2_level",
                    "osc2_unison",
                    "osc2_spread",
                    "osc2_width",
                ),
                _ => (
                    "osc3_enabled",
                    "osc3_wave",
                    "osc3_semitones",
                    "osc3_cents",
                    "osc3_level",
                    "osc3_unison",
                    "osc3_spread",
                    "osc3_width",
                ),
            };
            v.push(ParamInfo::switch(enabled, "On", o.enabled));
            v.push(ParamInfo::choice(
                wave,
                "Wave",
                o.waveform as u8 as f32,
                WAVEFORM_NAMES,
            ));
            v.push(ParamInfo::bipolar(semis, "Semi", o.semitones, -48.0, 48.0));
            v.push(ParamInfo::bipolar(cents, "Cents", o.cents, -100.0, 100.0));
            v.push(ParamInfo::new(level, "Level", o.level, 0.0, 1.0));
            v.push(ParamInfo::stepped(
                unison,
                "Unison",
                o.unison as f32,
                1.0,
                8.0,
            ));
            v.push(ParamInfo::new(spread, "Spread", o.spread, 0.0, 100.0));
            v.push(ParamInfo::new(width, "Width", o.width, 0.0, 1.0));
        }
        v.push(ParamInfo::new(
            "attack",
            "Attack",
            self.amp_env.attack,
            0.0,
            8.0,
        ));
        v.push(ParamInfo::new(
            "decay",
            "Decay",
            self.amp_env.decay,
            0.0,
            8.0,
        ));
        v.push(ParamInfo::new(
            "sustain",
            "Sustain",
            self.amp_env.sustain,
            0.0,
            1.0,
        ));
        v.push(ParamInfo::new(
            "release",
            "Release",
            self.amp_env.release,
            0.001,
            8.0,
        ));
        v.push(ParamInfo::choice(
            "filter",
            "Filter",
            self.filter as u8 as f32,
            FILTER_NAMES,
        ));
        v.push(ParamInfo::new(
            "cutoff",
            "Cutoff",
            self.cutoff,
            20.0,
            20_000.0,
        ));
        v.push(ParamInfo::new(
            "resonance",
            "Reso",
            self.resonance,
            0.0,
            0.98,
        ));
        v.push(ParamInfo::bipolar(
            "filter_env_amount",
            "Env amt",
            self.filter_env_amount,
            -8.0,
            8.0,
        ));
        v.push(ParamInfo::new(
            "filter_attack",
            "F. attack",
            self.filter_env.attack,
            0.0,
            8.0,
        ));
        v.push(ParamInfo::new(
            "filter_decay",
            "F. decay",
            self.filter_env.decay,
            0.0,
            8.0,
        ));
        v.push(ParamInfo::new(
            "filter_sustain",
            "F. sustain",
            self.filter_env.sustain,
            0.0,
            1.0,
        ));
        v.push(ParamInfo::new(
            "filter_release",
            "F. release",
            self.filter_env.release,
            0.001,
            8.0,
        ));
        v.push(ParamInfo::new(
            "filter_key_track",
            "Vel>cutoff",
            self.filter_key_track,
            0.0,
            1.0,
        ));
        v.push(ParamInfo::switch("lfo_enabled", "LFO on", self.lfo.enabled));
        v.push(ParamInfo::choice(
            "lfo_wave",
            "LFO wave",
            self.lfo.waveform as u8 as f32,
            WAVEFORM_NAMES,
        ));
        v.push(ParamInfo::new(
            "lfo_rate",
            "LFO rate",
            self.lfo.rate,
            0.01,
            60.0,
        ));
        v.push(ParamInfo::new(
            "lfo_depth",
            "LFO depth",
            self.lfo.depth,
            0.0,
            1.0,
        ));
        v.push(ParamInfo::choice(
            "lfo_target",
            "LFO to",
            self.lfo.target as u8 as f32,
            LFO_TARGET_NAMES,
        ));
        v.push(ParamInfo::new(
            "lfo_delay",
            "LFO delay",
            self.lfo.delay,
            0.0,
            10.0,
        ));
        v.push(ParamInfo::new("glide", "Glide", self.glide, 0.0, 5.0));
        v.push(ParamInfo::switch("mono", "Mono", self.mono));
        v.push(ParamInfo::new("gain", "Gain", self.gain, 0.0, 2.0));
        v.push(ParamInfo::new("drive", "Drive", self.drive, 0.0, 1.0));
        v.push(ParamInfo::new(
            "velocity_amount",
            "Vel amt",
            self.velocity_amount,
            0.0,
            1.0,
        ));
        v
    }

    pub fn set_param(&mut self, name: &str, value: f32) -> bool {
        fn wave_from(v: f32) -> Waveform {
            Waveform::ALL[(v.clamp(0.0, 6.0) as usize).min(6)]
        }
        let osc_index = match name.as_bytes() {
            [b'o', b's', b'c', d, b'_', ..] if (b'1'..=b'3').contains(d) => {
                Some((d - b'1') as usize)
            }
            _ => None,
        };
        if let Some(i) = osc_index {
            let o = &mut self.osc[i];
            let suffix = &name[5..];
            let handled = match suffix {
                "enabled" => {
                    o.enabled = value >= 0.5;
                    true
                }
                "wave" => {
                    o.waveform = wave_from(value);
                    true
                }
                "semitones" => {
                    o.semitones = value.clamp(-48.0, 48.0);
                    true
                }
                "cents" => {
                    o.cents = value.clamp(-100.0, 100.0);
                    true
                }
                "level" => {
                    o.level = value.clamp(0.0, 1.0);
                    true
                }
                "unison" => {
                    o.unison = value.clamp(1.0, 8.0) as u8;
                    true
                }
                "spread" => {
                    o.spread = value.clamp(0.0, 100.0);
                    true
                }
                "width" => {
                    o.width = value.clamp(0.0, 1.0);
                    true
                }
                _ => false,
            };
            if handled {
                return true;
            }
        }
        match name {
            "attack" => self.amp_env.attack = value.clamp(0.0, 8.0),
            "decay" => self.amp_env.decay = value.clamp(0.0, 8.0),
            "sustain" => self.amp_env.sustain = value.clamp(0.0, 1.0),
            "release" => self.amp_env.release = value.clamp(0.001, 8.0),
            "filter" => {
                self.filter = FilterKind::ALL[(value.clamp(0.0, 4.0) as usize).min(4)];
            }
            "cutoff" => self.cutoff = value.clamp(20.0, 20_000.0),
            "resonance" => self.resonance = value.clamp(0.0, 0.98),
            "filter_env_amount" => self.filter_env_amount = value.clamp(-8.0, 8.0),
            "filter_attack" => self.filter_env.attack = value.clamp(0.0, 8.0),
            "filter_decay" => self.filter_env.decay = value.clamp(0.0, 8.0),
            "filter_sustain" => self.filter_env.sustain = value.clamp(0.0, 1.0),
            "filter_release" => self.filter_env.release = value.clamp(0.001, 8.0),
            "filter_key_track" => self.filter_key_track = value.clamp(0.0, 1.0),
            "lfo_enabled" => self.lfo.enabled = value >= 0.5,
            "lfo_wave" => self.lfo.waveform = wave_from(value),
            "lfo_rate" => self.lfo.rate = value.clamp(0.01, 60.0),
            "lfo_depth" => self.lfo.depth = value.clamp(0.0, 1.0),
            "lfo_target" => {
                self.lfo.target = LfoTarget::ALL[(value.clamp(0.0, 4.0) as usize).min(4)];
            }
            "lfo_delay" => self.lfo.delay = value.clamp(0.0, 10.0),
            "glide" => self.glide = value.clamp(0.0, 5.0),
            "mono" => self.mono = value >= 0.5,
            "gain" => self.gain = value.clamp(0.0, 2.0),
            "drive" => self.drive = value.clamp(0.0, 1.0),
            "velocity_amount" => self.velocity_amount = value.clamp(0.0, 1.0),
            _ => return false,
        }
        true
    }
}

impl SamplerPatch {
    pub fn params(&self) -> Vec<ParamInfo> {
        vec![
            ParamInfo::new("root_key", "Root", self.root_key as f32, 0.0, 127.0),
            ParamInfo::bipolar("tune", "Tune", self.tune, -48.0, 48.0),
            ParamInfo::bipolar("fine", "Fine", self.fine, -100.0, 100.0),
            ParamInfo::switch("reverse", "Reverse", self.reverse),
            ParamInfo::switch("loop", "Loop", self.loop_sample),
            ParamInfo::new("start", "Start", self.start, 0.0, 1.0),
            ParamInfo::new("end", "End", self.end, 0.0, 1.0),
            ParamInfo::new("attack", "Attack", self.envelope.attack, 0.0, 8.0),
            ParamInfo::new("decay", "Decay", self.envelope.decay, 0.0, 8.0),
            ParamInfo::new("sustain", "Sustain", self.envelope.sustain, 0.0, 1.0),
            ParamInfo::new("release", "Release", self.envelope.release, 0.001, 8.0),
            ParamInfo::switch("gated", "Gated", self.gated),
            ParamInfo::new("gain", "Gain", self.gain, 0.0, 2.0),
            ParamInfo::choice("filter", "Filter", self.filter as u8 as f32, FILTER_NAMES),
            ParamInfo::new("cutoff", "Cutoff", self.cutoff, 20.0, 20_000.0),
            ParamInfo::new("resonance", "Reso", self.resonance, 0.0, 0.98),
            ParamInfo::new("velocity_amount", "Vel amt", self.velocity_amount, 0.0, 1.0),
        ]
    }

    pub fn set_param(&mut self, name: &str, value: f32) -> bool {
        match name {
            "root_key" => self.root_key = value.clamp(0.0, 127.0) as u8,
            "tune" => self.tune = value.clamp(-48.0, 48.0),
            "fine" => self.fine = value.clamp(-100.0, 100.0),
            "reverse" => self.reverse = value >= 0.5,
            "loop" => self.loop_sample = value >= 0.5,
            "start" => self.start = value.clamp(0.0, 1.0),
            "end" => self.end = value.clamp(0.0, 1.0),
            "attack" => self.envelope.attack = value.clamp(0.0, 8.0),
            "decay" => self.envelope.decay = value.clamp(0.0, 8.0),
            "sustain" => self.envelope.sustain = value.clamp(0.0, 1.0),
            "release" => self.envelope.release = value.clamp(0.001, 8.0),
            "gated" => self.gated = value >= 0.5,
            "gain" => self.gain = value.clamp(0.0, 2.0),
            "filter" => self.filter = FilterKind::ALL[(value.clamp(0.0, 4.0) as usize).min(4)],
            "cutoff" => self.cutoff = value.clamp(20.0, 20_000.0),
            "resonance" => self.resonance = value.clamp(0.0, 0.98),
            "velocity_amount" => self.velocity_amount = value.clamp(0.0, 1.0),
            _ => return false,
        }
        true
    }
}

impl SoundFontPatch {
    pub fn params(&self) -> Vec<ParamInfo> {
        vec![
            ParamInfo::stepped("bank", "Bank", self.bank as f32, 0.0, 128.0),
            ParamInfo::stepped("preset", "Preset", self.preset as f32, 0.0, 127.0),
            ParamInfo::switch("percussion", "Drums", self.percussion),
        ]
    }

    pub fn set_param(&mut self, name: &str, value: f32) -> bool {
        match name {
            "bank" => self.bank = value.clamp(0.0, 128.0) as u16,
            "preset" => self.preset = value.clamp(0.0, 127.0) as u16,
            "percussion" => self.percussion = value >= 0.5,
            _ => return false,
        }
        true
    }
}

impl Instrument {
    pub fn params(&self) -> Vec<ParamInfo> {
        match self {
            Instrument::Synth(p) => p.params(),
            Instrument::Sampler(p) => p.params(),
            Instrument::SoundFont(p) => p.params(),
        }
    }

    pub fn set_param(&mut self, name: &str, value: f32) -> bool {
        match self {
            Instrument::Synth(p) => p.set_param(name, value),
            Instrument::Sampler(p) => p.set_param(name, value),
            Instrument::SoundFont(p) => p.set_param(name, value),
        }
    }

    /// Set a parameter from a normalised 0..1 value, which is what automation
    /// lanes store.
    pub fn set_param_normalised(&mut self, name: &str, t: f32) -> bool {
        let Some(info) = self.params().into_iter().find(|p| p.name == name) else {
            return false;
        };
        self.set_param(name, info.denormalise(t))
    }
}
