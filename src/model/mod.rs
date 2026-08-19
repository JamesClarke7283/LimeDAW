//! The LimeDAW project model.
//!
//! Everything in here is plain data: serialisable, cloneable and free of any
//! platform or audio-backend dependency. The audio engine compiles this model
//! into an event schedule; the UI edits it; the MCP server pokes at it.

pub mod effect;
pub mod instrument;
pub mod pattern;
pub mod playlist;

pub use effect::*;
pub use instrument::*;
pub use pattern::*;
pub use playlist::*;

use serde::{Deserialize, Serialize};

/// Ticks per quarter note. Everything musical is stored in beats (f64), but
/// this is the resolution the sequencer quantises to internally.
pub const PPQ: u32 = 960;

/// A stable identifier. Ids are never reused within a project.
pub type Id = u32;

/// An RGB colour, stored as bytes so projects diff cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b)
    }

    pub fn to_iced(self) -> iced::Color {
        iced::Color::from_rgb8(self.0, self.1, self.2)
    }

    /// The default channel colour wheel — nicked in spirit from the way FL
    /// hands every new channel a distinct hue.
    pub fn wheel(index: usize) -> Self {
        const WHEEL: [Rgb; 12] = [
            Rgb(0x8B, 0xD1, 0x3A),
            Rgb(0x4F, 0xC3, 0xF7),
            Rgb(0xFF, 0x8A, 0x65),
            Rgb(0xBA, 0x68, 0xC8),
            Rgb(0xFF, 0xD5, 0x4F),
            Rgb(0x4D, 0xB6, 0xAC),
            Rgb(0xF0, 0x6E, 0x9A),
            Rgb(0x9F, 0xA8, 0xDA),
            Rgb(0xAE, 0xD5, 0x81),
            Rgb(0xFF, 0xB7, 0x4D),
            Rgb(0x64, 0xB5, 0xF6),
            Rgb(0xE5, 0x73, 0x73),
        ];
        WHEEL[index % WHEEL.len()]
    }
}

/// A musical time signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignature {
    pub numerator: u32,
    pub denominator: u32,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

impl TimeSignature {
    /// How many quarter-note beats fit in one bar.
    pub fn beats_per_bar(&self) -> f64 {
        self.numerator as f64 * 4.0 / self.denominator as f64
    }
}

/// One instrument slot in the channel rack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: Id,
    pub name: String,
    pub color: Rgb,
    pub instrument: Instrument,
    /// Linear gain, 0.0..=2.0 (1.0 is unity).
    pub volume: f32,
    /// -1.0 hard left .. 1.0 hard right.
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    /// Index into [`Mixer::tracks`]. 0 is master.
    pub mixer_track: usize,
    /// Global transpose for the channel, in semitones.
    pub transpose: f32,
    /// The key a step-sequencer step plays.
    pub step_key: u8,
    /// Per-channel insert effects, applied before the mixer routing.
    pub effects: Vec<Effect>,
}

impl Channel {
    pub fn new(id: Id, name: impl Into<String>, instrument: Instrument, index: usize) -> Self {
        Self {
            id,
            name: name.into(),
            color: Rgb::wheel(index),
            instrument,
            volume: 0.78,
            pan: 0.0,
            muted: false,
            solo: false,
            mixer_track: 0,
            transpose: 0.0,
            step_key: 60,
            effects: Vec::new(),
        }
    }
}

/// A mixer insert track. Track 0 is always the master bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerTrack {
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub effects: Vec<Effect>,
    /// (destination track index, send amount 0..1)
    pub sends: Vec<(usize, f32)>,
}

impl MixerTrack {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            volume: 0.8,
            pan: 0.0,
            muted: false,
            solo: false,
            effects: Vec::new(),
            sends: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mixer {
    pub tracks: Vec<MixerTrack>,
}

impl Default for Mixer {
    fn default() -> Self {
        let mut tracks = vec![MixerTrack::new("Master")];
        for i in 1..=16 {
            tracks.push(MixerTrack::new(format!("Insert {i}")));
        }
        Self { tracks }
    }
}

impl Mixer {
    pub fn any_solo(&self) -> bool {
        self.tracks.iter().skip(1).any(|t| t.solo)
    }
}

/// The whole song.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Format version, so old projects can be migrated.
    #[serde(default = "default_version")]
    pub version: u32,
    pub name: String,
    pub author: String,
    pub tempo: f64,
    pub time_signature: TimeSignature,
    /// 0.0 = straight, 0.5 = heavy shuffle. Applied to off-beat 16ths.
    pub swing: f64,
    pub master_volume: f32,
    pub channels: Vec<Channel>,
    pub patterns: Vec<Pattern>,
    pub playlist: Playlist,
    pub mixer: Mixer,
    /// Path of the soundfont this project expects, if any.
    pub soundfont: Option<String>,
    next_id: Id,
}

fn default_version() -> u32 {
    1
}

impl Default for Project {
    fn default() -> Self {
        Self::empty()
    }
}

impl Project {
    pub fn empty() -> Self {
        Self {
            version: 1,
            name: "Untitled".into(),
            author: String::new(),
            tempo: 130.0,
            time_signature: TimeSignature::default(),
            swing: 0.0,
            master_volume: 0.85,
            channels: Vec::new(),
            patterns: Vec::new(),
            playlist: Playlist::default(),
            mixer: Mixer::default(),
            soundfont: None,
            next_id: 1,
        }
    }

    /// A fresh project with a usable starting kit, the way a DAW should open.
    pub fn starter() -> Self {
        let mut p = Self::empty();

        let kick = p.add_channel("Kick", Instrument::Sampler(SamplerPatch::kick()));
        let snare = p.add_channel("Snare", Instrument::Sampler(SamplerPatch::snare()));
        let hat = p.add_channel("Hat", Instrument::Sampler(SamplerPatch::hat()));
        let clap = p.add_channel("Clap", Instrument::Sampler(SamplerPatch::clap()));
        let bass = p.add_channel("Bass", Instrument::Synth(SynthPatch::bass()));
        let lead = p.add_channel("Lead", Instrument::Synth(SynthPatch::lead()));
        let keys = p.add_channel("Keys", Instrument::Synth(SynthPatch::pad()));

        // Give each channel its own mixer insert so the mixer is immediately useful.
        for (i, id) in [kick, snare, hat, clap, bass, lead, keys]
            .iter()
            .enumerate()
        {
            if let Some(c) = p.channel_mut(*id) {
                c.mixer_track = i + 1;
            }
        }
        for (i, name) in ["Kick", "Snare", "Hat", "Clap", "Bass", "Lead", "Keys"]
            .iter()
            .enumerate()
        {
            p.mixer.tracks[i + 1].name = (*name).into();
        }

        let pat = p.add_pattern("Pattern 1");
        // A four-on-the-floor starting point: enough to hear the thing work.
        for step in 0..16 {
            let beat = step as f64 * 0.25;
            if step % 4 == 0 {
                p.add_step_note(pat, kick, beat);
            }
            if step % 8 == 4 {
                p.add_step_note(pat, snare, beat);
            }
            if step % 2 == 0 {
                p.add_step_note(pat, hat, beat);
            }
        }
        p.playlist.tracks[0].name = "Pattern 1".into();
        p.add_clip(0, pat, 0.0, 4.0);
        p
    }

    pub fn next_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Beats per bar, from the time signature.
    pub fn beats_per_bar(&self) -> f64 {
        self.time_signature.beats_per_bar()
    }

    pub fn seconds_per_beat(&self) -> f64 {
        60.0 / self.tempo.max(1.0)
    }

    // ---- channels ----------------------------------------------------

    pub fn add_channel(&mut self, name: impl Into<String>, instrument: Instrument) -> Id {
        let id = self.next_id();
        let index = self.channels.len();
        self.channels
            .push(Channel::new(id, name, instrument, index));
        id
    }

    pub fn channel(&self, id: Id) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id)
    }

    pub fn channel_mut(&mut self, id: Id) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }

    pub fn channel_index(&self, id: Id) -> Option<usize> {
        self.channels.iter().position(|c| c.id == id)
    }

    pub fn remove_channel(&mut self, id: Id) {
        self.channels.retain(|c| c.id != id);
        for pat in &mut self.patterns {
            pat.notes.retain(|n| n.channel != id);
        }
    }

    pub fn any_channel_solo(&self) -> bool {
        self.channels.iter().any(|c| c.solo)
    }

    /// Whether a channel should be heard, accounting for solo state.
    pub fn channel_audible(&self, id: Id) -> bool {
        match self.channel(id) {
            Some(c) => !c.muted && (!self.any_channel_solo() || c.solo),
            None => false,
        }
    }

    // ---- patterns ----------------------------------------------------

    pub fn add_pattern(&mut self, name: impl Into<String>) -> Id {
        let id = self.next_id();
        let index = self.patterns.len();
        self.patterns.push(Pattern::new(id, name, index));
        id
    }

    pub fn pattern(&self, id: Id) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id)
    }

    pub fn pattern_mut(&mut self, id: Id) -> Option<&mut Pattern> {
        self.patterns.iter_mut().find(|p| p.id == id)
    }

    pub fn remove_pattern(&mut self, id: Id) {
        self.patterns.retain(|p| p.id != id);
        self.playlist
            .clips
            .retain(|c| c.source != ClipSource::Pattern(id));
    }

    /// Add a 1/16th note at `beat` — what a step-sequencer step actually is.
    pub fn add_step_note(&mut self, pattern: Id, channel: Id, beat: f64) -> Option<Id> {
        let key = self.channel(channel).map(|c| c.step_key).unwrap_or(60);
        let id = self.next_id();
        let pat = self.pattern_mut(pattern)?;
        pat.notes.push(Note {
            id,
            channel,
            key,
            start: beat,
            length: 0.25,
            velocity: 0.78,
            pan: 0.0,
            fine_tune: 0.0,
            release: 0.0,
        });
        Some(id)
    }

    // ---- playlist ----------------------------------------------------

    pub fn add_clip(&mut self, track: usize, pattern: Id, start: f64, length: f64) -> Id {
        let id = self.next_id();
        self.playlist.clips.push(Clip {
            id,
            track,
            source: ClipSource::Pattern(pattern),
            start,
            length,
            offset: 0.0,
            muted: false,
        });
        id
    }

    /// Song length in beats: whichever ends last, the playlist or nothing.
    pub fn song_length_beats(&self) -> f64 {
        self.playlist
            .clips
            .iter()
            .map(|c| c.start + c.length)
            .fold(0.0f64, f64::max)
            .max(self.beats_per_bar())
    }

    /// Length of the longest pattern, for pattern-mode looping.
    pub fn pattern_length_beats(&self, id: Id) -> f64 {
        self.pattern(id)
            .map(|p| p.length_beats(self.beats_per_bar()))
            .unwrap_or_else(|| self.beats_per_bar())
    }

    // ---- validation --------------------------------------------------

    /// Clamp everything into legal ranges. Called after loading a project and
    /// after the MCP server has had its way with the model.
    pub fn sanitise(&mut self) {
        self.tempo = self.tempo.clamp(20.0, 999.0);
        self.swing = self.swing.clamp(0.0, 0.9);
        self.master_volume = self.master_volume.clamp(0.0, 2.0);
        self.time_signature.numerator = self.time_signature.numerator.clamp(1, 32);
        self.time_signature.denominator = match self.time_signature.denominator {
            1 | 2 | 4 | 8 | 16 | 32 => self.time_signature.denominator,
            _ => 4,
        };
        if self.mixer.tracks.is_empty() {
            self.mixer = Mixer::default();
        }
        let track_count = self.mixer.tracks.len();
        for t in &mut self.mixer.tracks {
            t.volume = t.volume.clamp(0.0, 2.0);
            t.pan = t.pan.clamp(-1.0, 1.0);
            t.sends.retain(|(dst, _)| *dst < track_count);
            for (_, amt) in &mut t.sends {
                *amt = amt.clamp(0.0, 1.0);
            }
        }
        for c in &mut self.channels {
            c.volume = c.volume.clamp(0.0, 2.0);
            c.pan = c.pan.clamp(-1.0, 1.0);
            c.transpose = c.transpose.clamp(-48.0, 48.0);
            if c.mixer_track >= track_count {
                c.mixer_track = 0;
            }
            c.instrument.sanitise();
            for e in &mut c.effects {
                e.sanitise();
            }
        }
        for t in &mut self.mixer.tracks {
            for e in &mut t.effects {
                e.sanitise();
            }
        }
        let channel_ids: Vec<Id> = self.channels.iter().map(|c| c.id).collect();
        for p in &mut self.patterns {
            p.notes.retain(|n| channel_ids.contains(&n.channel));
            for n in &mut p.notes {
                if !n.start.is_finite() {
                    n.start = 0.0;
                }
                if !n.length.is_finite() {
                    n.length = 0.25;
                }
                n.start = n.start.max(0.0);
                n.length = n.length.clamp(1.0 / 64.0, 512.0);
                // A key outside the MIDI range would be written straight into a
                // MIDI data byte on export.
                n.key = n.key.min(127);
                n.velocity = n.velocity.clamp(0.0, 1.0);
                n.pan = n.pan.clamp(-1.0, 1.0);
                n.fine_tune = n.fine_tune.clamp(-100.0, 100.0);
                n.release = n.release.clamp(0.0, 32.0);
            }
            p.notes.sort_by(|a, b| {
                a.start
                    .partial_cmp(&b.start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            if let Some(length) = p.length_override {
                p.length_override = Some(length.clamp(0.25, 4096.0));
            }
            // `value_at` binary-searches, so lane points must be in order.
            for lane in &mut p.automation {
                for point in &mut lane.points {
                    point.beat = if point.beat.is_finite() {
                        point.beat.max(0.0)
                    } else {
                        0.0
                    };
                    point.value = if point.value.is_finite() {
                        point.value.clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                }
                lane.sort();
            }
        }
        let pattern_ids: Vec<Id> = self.patterns.iter().map(|p| p.id).collect();
        let track_rows = self.playlist.tracks.len();
        let song_automation = self.playlist.automation.len();
        self.playlist.clips.retain(|c| match c.source {
            ClipSource::Pattern(p) => pattern_ids.contains(&p) && c.track < track_rows,
            ClipSource::Automation(index) => index < song_automation && c.track < track_rows,
        });
        for clip in &mut self.playlist.clips {
            if !clip.start.is_finite() {
                clip.start = 0.0;
            }
            if !clip.length.is_finite() {
                clip.length = 4.0;
            }
            clip.start = clip.start.max(0.0);
            clip.length = clip.length.clamp(0.25, 100_000.0);
            clip.offset = clip.offset.clamp(0.0, clip.length);
        }
        if let Some((start, end)) = self.playlist.loop_region {
            let start = start.max(0.0);
            let end = end.max(start + 0.25);
            self.playlist.loop_region =
                (start.is_finite() && end.is_finite()).then_some((start, end));
        }
        for lane in &mut self.playlist.automation {
            for point in &mut lane.points {
                point.beat = if point.beat.is_finite() {
                    point.beat.max(0.0)
                } else {
                    0.0
                };
                point.value = if point.value.is_finite() {
                    point.value.clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
            lane.sort();
        }
        // Keep next_id ahead of everything that exists.
        let max = self
            .channels
            .iter()
            .map(|c| c.id)
            .chain(self.patterns.iter().map(|p| p.id))
            .chain(
                self.patterns
                    .iter()
                    .flat_map(|p| p.notes.iter().map(|n| n.id)),
            )
            .chain(self.playlist.clips.iter().map(|c| c.id))
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max + 1);
    }
}
