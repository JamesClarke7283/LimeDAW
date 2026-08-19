//! The playlist: where patterns get arranged into a song.

use serde::{Deserialize, Serialize};

use super::{AutomationLane, Id, Rgb};

/// What a playlist clip plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClipSource {
    Pattern(Id),
    /// Index into [`Playlist::automation`].
    Automation(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: Id,
    /// Row in the playlist.
    pub track: usize,
    pub source: ClipSource,
    /// Position in beats from the song start.
    pub start: f64,
    /// How long the clip plays for, in beats. Longer than the pattern loops it.
    pub length: f64,
    /// Where in the pattern the clip starts reading, in beats.
    pub offset: f64,
    pub muted: bool,
}

impl Clip {
    pub fn end(&self) -> f64 {
        self.start + self.length
    }

    pub fn contains(&self, beat: f64) -> bool {
        beat >= self.start && beat < self.end()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaylistTrack {
    pub name: String,
    pub color: Rgb,
    pub muted: bool,
    pub solo: bool,
    pub height: f32,
}

impl PlaylistTrack {
    pub fn new(index: usize) -> Self {
        Self {
            name: format!("Track {}", index + 1),
            color: Rgb::wheel(index),
            muted: false,
            solo: false,
            height: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub tracks: Vec<PlaylistTrack>,
    pub clips: Vec<Clip>,
    /// Song-level automation lanes, referenced by `ClipSource::Automation`.
    pub automation: Vec<AutomationLane>,
    /// Loop region in beats, if set.
    pub loop_region: Option<(f64, f64)>,
}

impl Default for Playlist {
    fn default() -> Self {
        Self {
            tracks: (0..12).map(PlaylistTrack::new).collect(),
            clips: Vec::new(),
            automation: Vec::new(),
            loop_region: None,
        }
    }
}

impl Playlist {
    pub fn clip(&self, id: Id) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn clip_mut(&mut self, id: Id) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|c| c.id == id)
    }

    pub fn remove_clip(&mut self, id: Id) {
        self.clips.retain(|c| c.id != id);
    }

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.solo)
    }

    pub fn track_audible(&self, index: usize) -> bool {
        match self.tracks.get(index) {
            Some(t) => !t.muted && (!self.any_solo() || t.solo),
            None => false,
        }
    }

    /// Clips overlapping a beat range, for the sequencer to schedule.
    pub fn clips_in(&self, from: f64, to: f64) -> impl Iterator<Item = &Clip> {
        self.clips
            .iter()
            .filter(move |c| !c.muted && c.start < to && c.end() > from)
    }
}
