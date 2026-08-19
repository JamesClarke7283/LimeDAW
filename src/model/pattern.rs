//! Patterns: the clips of musical data that the playlist arranges.

use serde::{Deserialize, Serialize};

use super::{Id, Rgb};

/// A single note event, positioned in beats relative to its pattern's start.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: Id,
    /// The channel this note plays on.
    pub channel: Id,
    /// MIDI key, 0..=127.
    pub key: u8,
    /// Start position in beats.
    pub start: f64,
    /// Length in beats.
    pub length: f64,
    /// 0..1.
    pub velocity: f32,
    /// -1..1, offsets the channel pan.
    pub pan: f32,
    /// Cents.
    pub fine_tune: f32,
    /// Extra release time in beats, for per-note tails.
    pub release: f64,
}

impl Note {
    pub fn end(&self) -> f64 {
        self.start + self.length
    }

    pub fn contains(&self, beat: f64) -> bool {
        beat >= self.start && beat < self.end()
    }
}

/// What an automation point does between itself and the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Curve {
    Linear,
    Hold,
    Smooth,
    EaseIn,
    EaseOut,
}

impl Curve {
    pub const ALL: [Curve; 5] = [Curve::Linear, Curve::Hold, Curve::Smooth, Curve::EaseIn, Curve::EaseOut];

    pub fn label(self) -> &'static str {
        match self {
            Curve::Linear => "Linear",
            Curve::Hold => "Hold",
            Curve::Smooth => "Smooth",
            Curve::EaseIn => "Ease in",
            Curve::EaseOut => "Ease out",
        }
    }

    /// Shape a 0..1 interpolation factor.
    pub fn apply(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Linear => t,
            Curve::Hold => 0.0,
            Curve::Smooth => t * t * (3.0 - 2.0 * t),
            Curve::EaseIn => t * t,
            Curve::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    /// Position in beats.
    pub beat: f64,
    /// Normalised 0..1 value.
    pub value: f64,
    pub curve: Curve,
}

/// What an automation lane controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutomationTarget {
    Tempo,
    MasterVolume,
    ChannelVolume(Id),
    ChannelPan(Id),
    MixerVolume(usize),
    MixerPan(usize),
    /// A named parameter on an effect in a mixer track's chain.
    MixerEffect { track: usize, slot: usize, param: ParamRef },
    /// A named parameter on an effect in a channel's chain.
    ChannelEffect { channel: Id, slot: usize, param: ParamRef },
    /// A named parameter on a channel's instrument.
    Instrument { channel: Id, param: ParamRef },
}

/// A parameter name stored as a small fixed buffer so the whole target stays
/// `Copy` — automation targets get compared and copied a lot in the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamRef {
    bytes: [u8; 20],
    len: u8,
}

impl ParamRef {
    pub fn new(name: &str) -> Self {
        let mut bytes = [0u8; 20];
        let src = name.as_bytes();
        let len = src.len().min(20);
        bytes[..len].copy_from_slice(&src[..len]);
        Self { bytes, len: len as u8 }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

impl std::fmt::Display for ParamRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AutomationTarget {
    pub fn label(&self) -> String {
        match self {
            AutomationTarget::Tempo => "Tempo".into(),
            AutomationTarget::MasterVolume => "Master volume".into(),
            AutomationTarget::ChannelVolume(id) => format!("Channel {id} volume"),
            AutomationTarget::ChannelPan(id) => format!("Channel {id} pan"),
            AutomationTarget::MixerVolume(t) => format!("Insert {t} volume"),
            AutomationTarget::MixerPan(t) => format!("Insert {t} pan"),
            AutomationTarget::MixerEffect { track, slot, param } => {
                format!("Insert {track} fx {slot} {param}")
            }
            AutomationTarget::ChannelEffect { channel, slot, param } => {
                format!("Channel {channel} fx {slot} {param}")
            }
            AutomationTarget::Instrument { channel, param } => {
                format!("Channel {channel} {param}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    pub target: AutomationTarget,
    pub points: Vec<AutomationPoint>,
    pub enabled: bool,
}

impl AutomationLane {
    pub fn new(target: AutomationTarget) -> Self {
        Self {
            target,
            points: vec![
                AutomationPoint { beat: 0.0, value: 0.5, curve: Curve::Linear },
            ],
            enabled: true,
        }
    }

    /// Evaluate the lane at `beat`, or `None` if it has no points.
    pub fn value_at(&self, beat: f64) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        let first = &self.points[0];
        if beat <= first.beat {
            return Some(first.value);
        }
        let last = self.points.last().unwrap();
        if beat >= last.beat {
            return Some(last.value);
        }
        // Points are kept sorted, so a linear scan of a handful is fine.
        for w in self.points.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if beat >= a.beat && beat < b.beat {
                let span = (b.beat - a.beat).max(f64::EPSILON);
                let t = a.curve.apply((beat - a.beat) / span);
                return Some(a.value + (b.value - a.value) * t);
            }
        }
        Some(last.value)
    }

    pub fn sort(&mut self) {
        self.points
            .sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap_or(std::cmp::Ordering::Equal));
    }
}

/// A pattern holds notes for any number of channels, plus automation lanes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub id: Id,
    pub name: String,
    pub color: Rgb,
    pub notes: Vec<Note>,
    pub automation: Vec<AutomationLane>,
    /// Explicit length in beats. `None` means "as long as the content needs".
    pub length_override: Option<f64>,
}

impl Pattern {
    pub fn new(id: Id, name: impl Into<String>, index: usize) -> Self {
        Self {
            id,
            name: name.into(),
            color: Rgb::wheel(index + 3),
            notes: Vec::new(),
            automation: Vec::new(),
            length_override: None,
        }
    }

    /// Length in beats, rounded up to whole bars when derived from content.
    pub fn length_beats(&self, beats_per_bar: f64) -> f64 {
        if let Some(len) = self.length_override {
            return len.max(0.25);
        }
        let content = self
            .notes
            .iter()
            .map(|n| n.end())
            .chain(self.automation.iter().flat_map(|l| l.points.iter().map(|p| p.beat)))
            .fold(0.0f64, f64::max);
        if content <= 0.0 {
            beats_per_bar
        } else {
            (content / beats_per_bar).ceil().max(1.0) * beats_per_bar
        }
    }

    pub fn note(&self, id: Id) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    pub fn note_mut(&mut self, id: Id) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    pub fn remove_note(&mut self, id: Id) {
        self.notes.retain(|n| n.id != id);
    }

    /// Notes belonging to one channel, in time order.
    pub fn channel_notes(&self, channel: Id) -> impl Iterator<Item = &Note> {
        self.notes.iter().filter(move |n| n.channel == channel)
    }

    /// Is there a 1/16 step at this position for this channel?
    pub fn step_at(&self, channel: Id, beat: f64) -> Option<Id> {
        self.notes
            .iter()
            .find(|n| n.channel == channel && (n.start - beat).abs() < 1e-6)
            .map(|n| n.id)
    }

    pub fn lane_mut(&mut self, target: AutomationTarget) -> &mut AutomationLane {
        if let Some(i) = self.automation.iter().position(|l| l.target == target) {
            &mut self.automation[i]
        } else {
            self.automation.push(AutomationLane::new(target));
            self.automation.last_mut().unwrap()
        }
    }
}
