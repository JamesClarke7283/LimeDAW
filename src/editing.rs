//! Applying the canvas editors' actions to the project.

use iced::Task;

use crate::app::{LimeDaw, Message};
use crate::audio::EngineCmd;
use crate::audio::schedule::PlayMode;
use crate::model::*;
use crate::ui::{automation, channel_rack, piano_roll, playlist};

impl LimeDaw {
    pub(crate) fn piano_roll_action(&mut self, action: piano_roll::Action) -> Task<Message> {
        let Some(pattern_id) = self.selected_pattern_id() else {
            return Task::none();
        };
        let Some(channel_id) = self.selected_channel_id() else {
            return Task::none();
        };

        match action {
            piano_roll::Action::Seek(beat) => {
                self.engine.send(EngineCmd::Seek(beat));
            }
            piano_roll::Action::Scroll { beats, top_key } => {
                self.piano_roll.scroll_beats = beats.max(0.0);
                self.piano_roll.top_key = top_key.clamp(12.0, 127.0);
            }
            piano_roll::Action::Zoom { pixels_per_beat, key_height } => {
                self.piano_roll.pixels_per_beat = pixels_per_beat.clamp(8.0, 800.0);
                self.piano_roll.key_height = key_height.clamp(6.0, 40.0);
            }
            piano_roll::Action::Select(ids) => self.piano_roll.selection = ids,
            piano_roll::Action::BeginDrag(ids) => {
                self.checkpoint();
                self.piano_roll.selection = ids;
            }
            piano_roll::Action::Checkpoint => self.checkpoint(),
            piano_roll::Action::AddNote { key, start, length } => {
                self.checkpoint();
                let id = self.project.next_id();
                let velocity = self.settings.default_velocity;
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    pattern.notes.push(Note {
                        id,
                        channel: channel_id,
                        key,
                        start,
                        length,
                        velocity,
                        pan: 0.0,
                        fine_tune: 0.0,
                        release: 0.0,
                    });
                }
                self.piano_roll.last_created = Some(id);
                self.piano_roll.selection = vec![id];
                self.engine.resume();
                self.preview(channel_id, key);
                self.touch();
            }
            piano_roll::Action::DragNewNote { end } => {
                if let Some(id) = self.piano_roll.last_created {
                    if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                        if let Some(note) = pattern.note_mut(id) {
                            note.length = (end - note.start).max(1.0 / 64.0);
                        }
                    }
                    self.touch();
                }
            }
            piano_roll::Action::SetPositions(updates) => {
                // Audition the pitch the moment a dragged note crosses a key.
                let moved_to = updates.first().and_then(|(id, key, _)| {
                    let current = self.project.pattern(pattern_id)?.note(*id)?.key;
                    (current != *key).then_some(*key)
                });
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    for (id, key, start) in updates {
                        if let Some(note) = pattern.note_mut(id) {
                            note.key = key;
                            note.start = start.max(0.0);
                        }
                    }
                }
                if let Some(key) = moved_to {
                    self.release_all_previews();
                    self.preview(channel_id, key);
                }
                self.touch();
            }
            piano_roll::Action::SetLength(updates) => {
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    for (id, length) in updates {
                        if let Some(note) = pattern.note_mut(id) {
                            note.length = length.max(1.0 / 64.0);
                        }
                    }
                }
                self.touch();
            }
            piano_roll::Action::Delete(ids) => {
                self.checkpoint();
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    pattern.notes.retain(|n| !ids.contains(&n.id));
                }
                self.piano_roll.selection.retain(|id| !ids.contains(id));
                self.touch();
            }
            piano_roll::Action::SetVelocity(updates) => {
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    for (id, velocity) in updates {
                        if let Some(note) = pattern.note_mut(id) {
                            note.velocity = velocity.clamp(0.0, 1.0);
                        }
                    }
                }
                self.touch();
            }
            piano_roll::Action::Preview(key) => {
                self.release_all_previews();
                self.engine.resume();
                self.preview(channel_id, key);
            }
            piano_roll::Action::Release(_) => self.release_all_previews(),
            piano_roll::Action::SetTool(tool) => self.piano_roll.tool = tool,
            piano_roll::Action::ToggleVelocity => {
                self.piano_roll.show_velocity = !self.piano_roll.show_velocity;
            }
            piano_roll::Action::ToggleGhosts => {
                self.piano_roll.show_ghosts = !self.piano_roll.show_ghosts;
            }
        }
        Task::none()
    }

    pub(crate) fn steps_action(&mut self, action: channel_rack::Action) -> Task<Message> {
        let Some(pattern_id) = self.selected_pattern_id() else {
            return Task::none();
        };
        match action {
            channel_rack::Action::Seek(beat) => self.engine.send(EngineCmd::Seek(beat)),
            channel_rack::Action::SelectChannel(id) => self.selected_channel = Some(id),
            channel_rack::Action::Preview(id) => {
                let key = self.project.channel(id).map(|c| c.step_key).unwrap_or(60);
                self.engine.resume();
                self.preview(id, key);
            }
            channel_rack::Action::Paint { channel, step, on, first } => {
                // One checkpoint per drag, so undo steps back a whole gesture.
                if first {
                    self.checkpoint();
                }
                self.toggle_step(pattern_id, channel, step, Some(on));
            }
        }
        Task::none()
    }

    fn toggle_step(&mut self, pattern_id: Id, channel: Id, step: usize, force: Option<bool>) {
        let beat = step as f64 * 0.25;
        let existing = self
            .project
            .pattern(pattern_id)
            .and_then(|p| p.step_at(channel, beat));
        let want_on = force.unwrap_or(existing.is_none());

        match (existing, want_on) {
            (Some(id), false) => {
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    pattern.remove_note(id);
                }
                self.touch();
            }
            (None, true) => {
                let id = self.project.next_id();
                let key = self.project.channel(channel).map(|c| c.step_key).unwrap_or(60);
                let velocity = self.settings.default_velocity;
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    pattern.notes.push(Note {
                        id,
                        channel,
                        key,
                        start: beat,
                        length: 0.25,
                        velocity,
                        pan: 0.0,
                        fine_tune: 0.0,
                        release: 0.0,
                    });
                }
                self.selected_channel = Some(channel);
                self.engine.resume();
                self.preview_briefly(channel, key);
                self.touch();
            }
            _ => {}
        }
    }

    pub(crate) fn playlist_action(&mut self, action: playlist::Action) -> Task<Message> {
        match action {
            playlist::Action::Seek(beat) => self.engine.send(EngineCmd::Seek(beat)),
            playlist::Action::Scroll { beats, track } => {
                self.playlist.scroll_beats = beats.max(0.0);
                self.playlist.scroll_track = track.max(0.0);
            }
            playlist::Action::Zoom(ppb) => {
                self.playlist.pixels_per_beat = ppb.clamp(2.0, 240.0);
            }
            playlist::Action::Checkpoint => self.checkpoint(),
            playlist::Action::AddClip { track, start } => {
                let Some(pattern_id) = self
                    .playlist
                    .paint_pattern
                    .or_else(|| self.selected_pattern_id())
                else {
                    return Task::none();
                };
                self.checkpoint();
                let length = self.project.pattern_length_beats(pattern_id);
                let id = self.project.add_clip(track, pattern_id, start, length);
                self.playlist.selection = vec![id];
                self.touch();
            }
            playlist::Action::SetClip { id, track, start } => {
                if let Some(clip) = self.project.playlist.clip_mut(id) {
                    clip.track = track;
                    clip.start = start.max(0.0);
                }
                self.touch();
            }
            playlist::Action::SetClipLength { id, length } => {
                if let Some(clip) = self.project.playlist.clip_mut(id) {
                    clip.length = length.max(0.25);
                }
                self.touch();
            }
            playlist::Action::Delete(id) => {
                self.checkpoint();
                self.project.playlist.remove_clip(id);
                self.playlist.selection.retain(|c| *c != id);
                self.touch();
            }
            playlist::Action::Select(ids) => self.playlist.selection = ids,
            playlist::Action::BeginClipDrag { clip, pattern } => {
                self.checkpoint();
                self.playlist.selection = vec![clip];
                if let Some(pattern) = pattern {
                    self.playlist.paint_pattern = Some(pattern);
                    self.selected_pattern = Some(pattern);
                    self.needs_sync = true;
                }
            }
            playlist::Action::SelectPattern(id) => {
                self.playlist.paint_pattern = Some(id);
                self.selected_pattern = Some(id);
                self.needs_sync = true;
            }
            playlist::Action::ToggleTrackMute(index) => {
                if let Some(track) = self.project.playlist.tracks.get_mut(index) {
                    track.muted = !track.muted;
                }
                self.touch();
            }
            playlist::Action::ToggleTrackSolo(index) => {
                if let Some(track) = self.project.playlist.tracks.get_mut(index) {
                    track.solo = !track.solo;
                }
                self.touch();
            }
            playlist::Action::SetLoop { start, end } => {
                self.project.playlist.loop_region = Some((start, end.max(start + 0.25)));
                self.touch();
            }
            playlist::Action::ClearLoop => {
                self.project.playlist.loop_region = None;
                self.touch();
            }
        }
        Task::none()
    }

    pub(crate) fn automation_action(&mut self, action: automation::Action) -> Task<Message> {
        let Some(pattern_id) = self.selected_pattern_id() else {
            return Task::none();
        };
        match action {
            automation::Action::Toggle => {
                self.automation.visible = !self.automation.visible;
                if self.automation.visible {
                    self.view = crate::app::MainView::PianoRoll;
                    self.play_mode = PlayMode::Pattern(pattern_id);
                    self.engine.send(EngineCmd::SetMode(self.play_mode));
                    self.needs_sync = true;
                }
            }
            automation::Action::SelectLane(index) => self.automation.lane = index,
            automation::Action::AddLane(target) => {
                self.checkpoint();
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    if let Some(index) = pattern.automation.iter().position(|l| l.target == target) {
                        self.automation.lane = index;
                    } else {
                        pattern.automation.push(AutomationLane::new(target));
                        self.automation.lane = pattern.automation.len() - 1;
                    }
                }
                self.automation.visible = true;
                self.touch();
            }
            automation::Action::RemoveLane(index) => {
                self.checkpoint();
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    if index < pattern.automation.len() {
                        pattern.automation.remove(index);
                    }
                }
                self.automation.lane = self.automation.lane.saturating_sub(1);
                self.touch();
            }
            automation::Action::Checkpoint => self.checkpoint(),
            automation::Action::AddPoint { beat, value } => {
                self.checkpoint();
                let lane_index = self.automation.lane;
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    if let Some(lane) = pattern.automation.get_mut(lane_index) {
                        lane.points.push(AutomationPoint { beat, value, curve: Curve::Linear });
                        lane.sort();
                    }
                }
                self.touch();
            }
            automation::Action::MovePoint { index, beat, value } => {
                // Deliberately not sorted here: the drag is holding this index.
                let lane_index = self.automation.lane;
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    if let Some(lane) = pattern.automation.get_mut(lane_index) {
                        if let Some(point) = lane.points.get_mut(index) {
                            point.beat = beat.max(0.0);
                            point.value = value.clamp(0.0, 1.0);
                        }
                    }
                }
                self.touch();
            }
            automation::Action::SortLane => {
                let lane_index = self.automation.lane;
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    if let Some(lane) = pattern.automation.get_mut(lane_index) {
                        lane.sort();
                    }
                }
                self.touch();
            }
            automation::Action::RemovePoint(index) => {
                self.checkpoint();
                let lane_index = self.automation.lane;
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    if let Some(lane) = pattern.automation.get_mut(lane_index) {
                        if index < lane.points.len() {
                            lane.points.remove(index);
                        }
                    }
                }
                self.touch();
            }
            automation::Action::SetCurve { index, curve } => {
                let lane_index = self.automation.lane;
                if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                    if let Some(lane) = pattern.automation.get_mut(lane_index) {
                        if let Some(point) = lane.points.get_mut(index) {
                            point.curve = curve;
                        }
                    }
                }
                self.touch();
            }
            automation::Action::Seek(beat) => self.engine.send(EngineCmd::Seek(beat)),
        }
        Task::none()
    }
}
