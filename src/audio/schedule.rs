//! Compiling the project model into a flat, time-ordered event schedule.
//!
//! Done on the control side so the audio thread only ever walks a sorted list.

use crate::model::{
    AutomationTarget, ClipSource, Curve, Id, Instrument, Mixer, Project, TimeSignature,
};

/// One note occurrence in song time.
#[derive(Debug, Clone, Copy)]
pub struct ScheduledNote {
    pub beat: f64,
    pub end: f64,
    pub channel: Id,
    pub key: u8,
    pub velocity: f32,
    pub pan: f32,
    pub fine: f32,
    /// Unique per occurrence, so repeats of the same note release correctly.
    pub uid: u32,
}

/// An automation lane flattened into song time.
#[derive(Debug, Clone)]
pub struct FlatLane {
    pub target: AutomationTarget,
    pub points: Vec<(f64, f64, Curve)>,
}

impl FlatLane {
    pub fn value_at(&self, beat: f64) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        let (first_beat, first_value, _) = self.points[0];
        if beat <= first_beat {
            return Some(first_value);
        }
        let last = *self.points.last().unwrap();
        if beat >= last.0 {
            return Some(last.1);
        }
        // Binary search: song-length lanes can hold a lot of points.
        let idx = match self
            .points
            .binary_search_by(|p| p.0.partial_cmp(&beat).unwrap_or(std::cmp::Ordering::Equal))
        {
            Ok(i) => return Some(self.points[i].1),
            Err(i) => i.saturating_sub(1),
        };
        let (b0, v0, curve) = self.points[idx];
        let (b1, v1, _) = self.points[(idx + 1).min(self.points.len() - 1)];
        let span = (b1 - b0).max(f64::EPSILON);
        let t = curve.apply((beat - b0) / span);
        Some(v0 + (v1 - v0) * t)
    }
}

/// What the transport is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    /// Loop a single pattern, the way the channel rack auditions one.
    Pattern(Id),
    /// Play the arrangement.
    Song,
}

/// The compiled, time-ordered performance.
#[derive(Debug, Clone, Default)]
pub struct Schedule {
    /// Sorted by `beat`.
    pub notes: Vec<ScheduledNote>,
    pub automation: Vec<FlatLane>,
    /// Total length in beats; the transport loops or stops here.
    pub length: f64,
}

impl Schedule {
    /// Index of the first note at or after `beat`.
    pub fn seek(&self, beat: f64) -> usize {
        self.notes.partition_point(|n| n.beat < beat)
    }
}

/// Swing an off-beat 16th later. `amount` is 0..0.9.
fn apply_swing(beat: f64, amount: f64) -> f64 {
    if amount <= 0.0 {
        return beat;
    }
    let sixteenth = beat / 0.25;
    let index = sixteenth.floor();
    let frac = sixteenth - index;
    if (index as i64) % 2 == 1 {
        (index + frac) * 0.25 + amount * 0.125
    } else {
        beat
    }
}

/// Compile a project into a schedule for the given play mode.
pub fn compile(project: &Project, mode: PlayMode) -> Schedule {
    let mut notes = Vec::new();
    let mut automation: Vec<FlatLane> = Vec::new();
    let mut uid: u32 = 0;
    let beats_per_bar = project.beats_per_bar();

    let push_pattern = |pattern_id: Id,
                            clip_start: f64,
                            clip_len: f64,
                            clip_offset: f64,
                            notes: &mut Vec<ScheduledNote>,
                            automation: &mut Vec<FlatLane>,
                            uid: &mut u32| {
        let Some(pattern) = project.pattern(pattern_id) else {
            return;
        };
        let pattern_len = pattern.length_beats(beats_per_bar).max(0.25);

        // A clip longer than its pattern repeats it, exactly like a stretched
        // pattern block in the playlist.
        let mut repeat = 0usize;
        loop {
            let repeat_origin = clip_start + repeat as f64 * pattern_len - clip_offset;
            if repeat_origin >= clip_start + clip_len {
                break;
            }
            if repeat > 1 << 20 {
                break; // backstop against a pathological clip length
            }
            for n in &pattern.notes {
                if !project.channel_audible(n.channel) {
                    continue;
                }
                // Content past the pattern's end belongs to the piano roll's
                // scratch space, not to the repeat.
                if n.start >= pattern_len - 1e-9 {
                    continue;
                }
                let start = repeat_origin + apply_swing(n.start, project.swing);
                // Swing moves the whole note, so the length is preserved.
                let end = start + n.length + n.release;
                // Trim to the clip window.
                if start >= clip_start + clip_len || end <= clip_start {
                    continue;
                }
                *uid = uid.wrapping_add(1);
                notes.push(ScheduledNote {
                    beat: start.max(clip_start),
                    end: end.min(clip_start + clip_len).max(start + 1.0 / 64.0),
                    channel: n.channel,
                    key: n.key,
                    velocity: n.velocity,
                    pan: n.pan,
                    fine: n.fine_tune,
                    uid: *uid,
                });
            }
            for lane in &pattern.automation {
                if !lane.enabled {
                    continue;
                }
                let entry = match automation.iter().position(|l| l.target == lane.target) {
                    Some(i) => &mut automation[i],
                    None => {
                        automation.push(FlatLane { target: lane.target, points: Vec::new() });
                        automation.last_mut().unwrap()
                    }
                };
                for p in &lane.points {
                    let beat = repeat_origin + p.beat;
                    if beat >= clip_start - 1e-9 && beat <= clip_start + clip_len + 1e-9 {
                        entry.points.push((beat, p.value, p.curve));
                    }
                }
            }
            repeat += 1;
        }
    };

    let length = match mode {
        PlayMode::Pattern(id) => {
            let len = project.pattern_length_beats(id);
            push_pattern(id, 0.0, len, 0.0, &mut notes, &mut automation, &mut uid);
            len
        }
        PlayMode::Song => {
            for clip in &project.playlist.clips {
                if clip.muted || !project.playlist.track_audible(clip.track) {
                    continue;
                }
                match clip.source {
                    ClipSource::Pattern(pid) => push_pattern(
                        pid,
                        clip.start,
                        clip.length,
                        clip.offset,
                        &mut notes,
                        &mut automation,
                        &mut uid,
                    ),
                    ClipSource::Automation(index) => {
                        if let Some(lane) = project.playlist.automation.get(index) {
                            if !lane.enabled {
                                continue;
                            }
                            let entry = match automation.iter().position(|l| l.target == lane.target) {
                                Some(i) => &mut automation[i],
                                None => {
                                    automation.push(FlatLane {
                                        target: lane.target,
                                        points: Vec::new(),
                                    });
                                    automation.last_mut().unwrap()
                                }
                            };
                            for p in &lane.points {
                                let beat = clip.start + p.beat - clip.offset;
                                if beat >= clip.start - 1e-9 && beat <= clip.end() + 1e-9 {
                                    entry.points.push((beat, p.value, p.curve));
                                }
                            }
                        }
                    }
                }
            }
            project.song_length_beats()
        }
    };

    notes.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap_or(std::cmp::Ordering::Equal));
    for lane in &mut automation {
        lane.points
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        // Where a repeat's first point lands on the previous repeat's last,
        // the new one wins — that is the reset the curve is asking for.
        let mut deduped: Vec<(f64, f64, Curve)> = Vec::with_capacity(lane.points.len());
        for point in lane.points.drain(..) {
            match deduped.last_mut() {
                Some(last) if (last.0 - point.0).abs() < 1e-9 => *last = point,
                _ => deduped.push(point),
            }
        }
        lane.points = deduped;
    }

    Schedule { notes, automation, length: length.max(0.25) }
}

/// Everything the audio engine needs, resolved and ready to play.
pub struct Snapshot {
    pub schedule: Schedule,
    pub channels: Vec<ChannelSnapshot>,
    pub mixer: Mixer,
    pub tempo: f64,
    pub master_volume: f32,
    pub time_signature: TimeSignature,
    pub loop_enabled: bool,
    pub loop_region: Option<(f64, f64)>,
    pub metronome: bool,
    /// Bumped every rebuild so the engine can tell snapshots apart.
    pub generation: u64,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            schedule: Schedule::default(),
            channels: Vec::new(),
            mixer: Mixer::default(),
            tempo: 130.0,
            master_volume: 0.85,
            time_signature: TimeSignature::default(),
            loop_enabled: true,
            loop_region: None,
            metronome: false,
            generation: 0,
        }
    }
}

/// A channel as the engine sees it: instrument, routing and a resolved sample.
pub struct ChannelSnapshot {
    pub id: Id,
    pub instrument: Instrument,
    pub volume: f32,
    pub pan: f32,
    pub audible: bool,
    pub mixer_track: usize,
    pub transpose: f32,
    pub effects: Vec<crate::model::Effect>,
    /// Preresolved so note-on never touches a map or the filesystem.
    pub sample: Option<std::sync::Arc<crate::audio::samples::Sample>>,
    /// Which soundfont synth slot this channel drives, if any.
    pub sf_slot: Option<usize>,
}
