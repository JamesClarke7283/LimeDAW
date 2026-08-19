//! Executing MCP tool calls against the running application.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::app::{FxHost, LimeDaw, MainView, NewChannelKind, SynthPreset};
use crate::audio::EngineCmd;
use crate::audio::schedule::PlayMode;
use crate::model::{ParamKind, *};
use crate::theme::ThemeChoice;

/// Small helpers for reading arguments without a mountain of match arms.
trait Args {
    fn f64_of(&self, key: &str) -> Option<f64>;
    fn f32_of(&self, key: &str) -> Option<f32>;
    fn u32_of(&self, key: &str) -> Option<u32>;
    fn usize_of(&self, key: &str) -> Option<usize>;
    fn str_of(&self, key: &str) -> Option<String>;
    fn bool_of(&self, key: &str) -> Option<bool>;
}

impl Args for Value {
    fn f64_of(&self, key: &str) -> Option<f64> {
        let value = match self.get(key) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => s.parse().ok(),
            _ => None,
        }?;
        // NaN and infinity survive `clamp`, so they must never get in.
        value.is_finite().then_some(value)
    }
    fn f32_of(&self, key: &str) -> Option<f32> {
        self.f64_of(key).map(|v| v as f32)
    }
    fn u32_of(&self, key: &str) -> Option<u32> {
        self.f64_of(key).map(|v| v.max(0.0) as u32)
    }
    fn usize_of(&self, key: &str) -> Option<usize> {
        self.f64_of(key).map(|v| v.max(0.0) as usize)
    }
    fn str_of(&self, key: &str) -> Option<String> {
        self.get(key).and_then(Value::as_str).map(str::to_string)
    }
    fn bool_of(&self, key: &str) -> Option<bool> {
        match self.get(key) {
            Some(Value::Bool(b)) => Some(*b),
            Some(Value::Number(n)) => n.as_f64().map(|v| v >= 0.5),
            _ => None,
        }
    }
}

fn missing(name: &str) -> String {
    format!("missing required argument: {name}")
}

/// Parse `#RRGGBB` into the project's colour type.
fn parse_color(text: &str) -> Option<Rgb> {
    let hex = text.trim().trim_start_matches('#');
    // Slicing a string by bytes panics on a multi-byte character, so check
    // before indexing rather than after.
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Rgb(r, g, b))
}

fn color_to_hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c.0, c.1, c.2)
}

/// The most playlist rows a project may have. Rows are cheap, but a tool call
/// asking for row four billion should be an error, not an allocation.
const MAX_PLAYLIST_TRACKS: usize = 256;

/// Grow the playlist to include `track`, or refuse if that is unreasonable.
fn ensure_playlist_track(playlist: &mut Playlist, track: usize) -> Result<(), String> {
    if track >= MAX_PLAYLIST_TRACKS {
        return Err(format!(
            "playlist row {track} is out of range (0..{MAX_PLAYLIST_TRACKS})"
        ));
    }
    while playlist.tracks.len() <= track {
        let index = playlist.tracks.len();
        playlist.tracks.push(PlaylistTrack::new(index));
    }
    Ok(())
}

fn curve_from(name: &str) -> Curve {
    match name {
        "hold" => Curve::Hold,
        "smooth" => Curve::Smooth,
        "ease_in" => Curve::EaseIn,
        "ease_out" => Curve::EaseOut,
        _ => Curve::Linear,
    }
}

impl LimeDaw {
    /// Run one MCP tool. Errors come back as plain messages for the client.
    pub fn handle_mcp_tool(&mut self, tool: &str, args: &Value) -> Result<Value, String> {
        let result = self.dispatch(tool, args)?;
        // Anything a tool changed needs to reach the audio engine.
        if self.needs_sync {
            self.sync_engine();
        }
        Ok(result)
    }

    fn dispatch(&mut self, tool: &str, args: &Value) -> Result<Value, String> {
        match tool {
            // ---- session ------------------------------------------------
            "get_state" => Ok(self.state_summary()),
            "get_project" => serde_json::to_value(&self.project).map_err(|e| e.to_string()),
            "set_project" => {
                let raw = args.get("project").ok_or_else(|| missing("project"))?;
                let project: Project = serde_json::from_value(raw.clone())
                    .map_err(|e| format!("invalid project: {e}"))?;
                self.checkpoint();
                self.load_project(project, self.project_path.clone());
                self.dirty = true;
                Ok(json!({ "ok": true, "name": self.project.name }))
            }
            "new_project" => {
                self.checkpoint();
                let empty = args.bool_of("empty").unwrap_or(false);
                let mut project = if empty {
                    Project::empty()
                } else {
                    Project::starter()
                };
                if let Some(name) = args.str_of("name") {
                    project.name = name;
                }
                self.load_project(project, None);
                Ok(json!({ "ok": true, "name": self.project.name }))
            }
            "list_projects" => {
                let projects: Vec<Value> = crate::paths::discover_projects()
                    .into_iter()
                    .map(|p| {
                        json!({
                            "name": p.file_stem().map(|n| n.to_string_lossy().to_string()),
                            "path": p.to_string_lossy(),
                        })
                    })
                    .collect();
                Ok(
                    json!({ "directory": crate::paths::projects_dir().to_string_lossy(), "projects": projects }),
                )
            }
            "open_project" => {
                let raw = args.str_of("path").ok_or_else(|| missing("path"))?;
                let path = resolve_project_path(&raw);
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("could not read {}: {e}", path.display()))?;
                let project = crate::persist::project_from_json(&text)
                    .map_err(|e| format!("could not parse {}: {e}", path.display()))?;
                self.checkpoint();
                self.load_project(project, Some(path.clone()));
                self.settings
                    .remember_project(path.to_string_lossy().as_ref());
                let _ = self.settings.save();
                Ok(json!({ "ok": true, "path": path.to_string_lossy(), "name": self.project.name }))
            }
            "save_project" => {
                let path = match args.str_of("path") {
                    Some(raw) => resolve_project_path(&raw),
                    None => self.project_path.clone().unwrap_or_else(|| {
                        crate::paths::projects_dir().join(format!(
                            "{}.lime",
                            crate::actions::safe_name(&self.project.name)
                        ))
                    }),
                };
                self.project.sanitise();
                let json_text =
                    crate::persist::project_to_json(&self.project).map_err(|e| e.to_string())?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(&path, json_text)
                    .map_err(|e| format!("could not write {}: {e}", path.display()))?;
                self.project_path = Some(path.clone());
                self.dirty = false;
                self.settings
                    .remember_project(path.to_string_lossy().as_ref());
                let _ = self.settings.save();
                Ok(json!({ "ok": true, "path": path.to_string_lossy() }))
            }
            "export_wav" => self.export_wav(args),

            // ---- transport ----------------------------------------------
            "play" => {
                if let Some(position) = args.f64_of("position") {
                    self.engine.send(EngineCmd::Seek(position.max(0.0)));
                }
                self.engine.send(EngineCmd::Play);
                Ok(json!({ "playing": true }))
            }
            "stop" => {
                self.engine.send(EngineCmd::Stop);
                Ok(json!({ "playing": false, "position": 0.0 }))
            }
            "pause" => {
                self.engine.send(EngineCmd::Pause);
                Ok(json!({ "playing": false }))
            }
            "set_position" => {
                let beat = args.f64_of("beat").ok_or_else(|| missing("beat"))?;
                self.engine.send(EngineCmd::Seek(beat.max(0.0)));
                Ok(json!({ "position": beat.max(0.0) }))
            }
            "set_tempo" => {
                let bpm = args.f64_of("bpm").ok_or_else(|| missing("bpm"))?;
                self.project.tempo = bpm.clamp(20.0, 999.0);
                self.tempo_text = format!("{:.2}", self.project.tempo);
                self.engine.send(EngineCmd::SetTempo(self.project.tempo));
                self.touch_schedule();
                Ok(json!({ "tempo": self.project.tempo }))
            }
            "set_time_signature" => {
                let numerator = args
                    .u32_of("numerator")
                    .ok_or_else(|| missing("numerator"))?;
                let denominator = args
                    .u32_of("denominator")
                    .ok_or_else(|| missing("denominator"))?;
                self.project.time_signature = TimeSignature {
                    numerator,
                    denominator,
                };
                self.project.sanitise();
                self.touch_schedule();
                Ok(json!({
                    "numerator": self.project.time_signature.numerator,
                    "denominator": self.project.time_signature.denominator,
                }))
            }
            "set_swing" => {
                let amount = args.f64_of("amount").ok_or_else(|| missing("amount"))?;
                self.project.swing = amount.clamp(0.0, 0.9);
                self.touch_schedule();
                Ok(json!({ "swing": self.project.swing }))
            }
            "set_play_mode" => {
                let mode = args.str_of("mode").ok_or_else(|| missing("mode"))?;
                self.play_mode = match mode.as_str() {
                    "song" => PlayMode::Song,
                    _ => PlayMode::Pattern(
                        self.selected_pattern_id()
                            .ok_or("there is no pattern to play")?,
                    ),
                };
                self.engine.send(EngineCmd::SetMode(self.play_mode));
                self.needs_sync = true;
                Ok(json!({ "mode": mode }))
            }
            "set_loop" => {
                if args.bool_of("clear").unwrap_or(false) {
                    self.project.playlist.loop_region = None;
                } else {
                    let start = args.f64_of("start").unwrap_or(0.0).max(0.0);
                    let end = args
                        .f64_of("end")
                        .unwrap_or_else(|| self.project.song_length_beats())
                        .max(start + 0.25);
                    self.project.playlist.loop_region = Some((start, end));
                }
                if let Some(enabled) = args.bool_of("enabled") {
                    self.engine.send(EngineCmd::SetLoopEnabled(enabled));
                }
                self.touch_schedule();
                Ok(json!({ "loop": self.project.playlist.loop_region }))
            }
            "set_metronome" => {
                let enabled = args.bool_of("enabled").ok_or_else(|| missing("enabled"))?;
                self.settings.metronome = enabled;
                self.engine.send(EngineCmd::SetMetronome(enabled));
                let _ = self.settings.save();
                Ok(json!({ "metronome": enabled }))
            }
            "set_master_volume" => {
                let volume = args.f32_of("volume").ok_or_else(|| missing("volume"))?;
                self.project.master_volume = volume.clamp(0.0, 2.0);
                self.touch_schedule();
                Ok(json!({ "master_volume": self.project.master_volume }))
            }

            // ---- channels ------------------------------------------------
            "list_channels" => Ok(json!({
                "channels": self.project.channels.iter().map(|c| self.channel_json(c)).collect::<Vec<_>>()
            })),
            "add_channel" => self.add_channel_tool(args),
            "remove_channel" => {
                let id = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
                if self.project.channel(id).is_none() {
                    return Err(format!("no channel with id {id}"));
                }
                self.checkpoint();
                self.project.remove_channel(id);
                if self.selected_channel == Some(id) {
                    self.selected_channel = self.project.channels.first().map(|c| c.id);
                }
                self.touch_schedule();
                Ok(json!({ "removed": id }))
            }
            "set_channel" => self.set_channel_tool(args),
            "set_instrument" => self.set_instrument_tool(args),
            "get_instrument_params" => {
                let id = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
                let channel = self.project.channel(id).ok_or("no such channel")?;
                Ok(json!({
                    "channel": id,
                    "kind": channel.instrument.kind_label(),
                    "params": channel.instrument.params().iter().map(|p| json!({
                        "name": p.name,
                        "label": p.label,
                        "value": p.value,
                        "min": p.min,
                        "max": p.max,
                        "stepped": p.stepped,
                    })).collect::<Vec<_>>()
                }))
            }
            "set_instrument_param" => {
                let id = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
                let param = args.str_of("param").ok_or_else(|| missing("param"))?;
                let channel = self.project.channel_mut(id).ok_or("no such channel")?;
                let applied = if let Some(t) = args.f32_of("normalised") {
                    channel.instrument.set_param_normalised(&param, t)
                } else {
                    let value = args.f32_of("value").ok_or("provide value or normalised")?;
                    channel.instrument.set_param(&param, value)
                };
                if !applied {
                    return Err(format!("unknown parameter: {param}"));
                }
                let value = self
                    .project
                    .channel(id)
                    .and_then(|c| c.instrument.params().into_iter().find(|p| p.name == param))
                    .map(|p| p.value)
                    .unwrap_or_default();
                self.touch_schedule();
                Ok(json!({ "channel": id, "param": param, "value": value }))
            }
            "preview_note" => {
                let id = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
                let key = args.u32_of("key").ok_or_else(|| missing("key"))?.min(127) as u8;
                let velocity = args.f32_of("velocity").unwrap_or(0.8).clamp(0.0, 1.0);
                if self.project.channel(id).is_none() {
                    return Err(format!("no channel with id {id}"));
                }
                // Nothing will send a matching note-off, so give it a deadline.
                self.engine.send(EngineCmd::NoteOn {
                    channel: id,
                    key,
                    velocity,
                });
                self.hold_preview(id, key);
                Ok(json!({ "channel": id, "key": key }))
            }

            // ---- patterns -------------------------------------------------
            "list_patterns" => {
                let beats_per_bar = self.project.beats_per_bar();
                Ok(json!({
                    "selected": self.selected_pattern_id(),
                    "patterns": self.project.patterns.iter().map(|p| json!({
                        "id": p.id,
                        "name": p.name,
                        "color": color_to_hex(p.color),
                        "length_beats": p.length_beats(beats_per_bar),
                        "fixed_length": p.length_override,
                        "notes": p.notes.len(),
                        "automation_lanes": p.automation.len(),
                    })).collect::<Vec<_>>()
                }))
            }
            "add_pattern" => {
                self.checkpoint();
                let n = self.project.patterns.len() + 1;
                let name = args
                    .str_of("name")
                    .unwrap_or_else(|| format!("Pattern {n}"));
                let id = self.project.add_pattern(name.clone());
                if let Some(length) = args.f64_of("length_beats")
                    && let Some(pattern) = self.project.pattern_mut(id)
                {
                    pattern.length_override = Some(length.max(0.25));
                }
                self.selected_pattern = Some(id);
                if matches!(self.play_mode, PlayMode::Pattern(_)) {
                    self.play_mode = PlayMode::Pattern(id);
                    self.engine.send(EngineCmd::SetMode(self.play_mode));
                }
                self.touch_schedule();
                Ok(json!({ "id": id, "name": name }))
            }
            "remove_pattern" => {
                let id = args.u32_of("pattern").ok_or_else(|| missing("pattern"))?;
                if self.project.pattern(id).is_none() {
                    return Err(format!("no pattern with id {id}"));
                }
                self.checkpoint();
                self.project.remove_pattern(id);
                if self.selected_pattern == Some(id) {
                    self.selected_pattern = self.project.patterns.first().map(|p| p.id);
                }
                self.touch_schedule();
                Ok(json!({ "removed": id }))
            }
            "set_pattern" => {
                let id = args.u32_of("pattern").ok_or_else(|| missing("pattern"))?;
                let colour = args.str_of("color").and_then(|c| parse_color(&c));
                if self.project.pattern(id).is_none() {
                    return Err(format!("no pattern with id {id}"));
                }
                self.checkpoint();
                let pattern = self.project.pattern_mut(id).ok_or("no such pattern")?;
                if let Some(name) = args.str_of("name") {
                    pattern.name = name;
                }
                if let Some(colour) = colour {
                    pattern.color = colour;
                }
                if args.bool_of("auto_length").unwrap_or(false) {
                    pattern.length_override = None;
                } else if let Some(length) = args.f64_of("length_beats") {
                    pattern.length_override = Some(length.max(0.25));
                }
                self.touch_schedule();
                Ok(json!({ "ok": true, "pattern": id }))
            }
            "select_pattern" => {
                let id = args.u32_of("pattern").ok_or_else(|| missing("pattern"))?;
                if self.project.pattern(id).is_none() {
                    return Err(format!("no pattern with id {id}"));
                }
                self.selected_pattern = Some(id);
                if matches!(self.play_mode, PlayMode::Pattern(_)) {
                    self.play_mode = PlayMode::Pattern(id);
                    self.engine.send(EngineCmd::SetMode(self.play_mode));
                }
                self.needs_sync = true;
                Ok(json!({ "selected": id }))
            }
            "clear_pattern" => {
                let id = self.pattern_arg(args)?;
                let channel = args.u32_of("channel");
                self.checkpoint();
                let pattern = self.project.pattern_mut(id).ok_or("no such pattern")?;
                let before = pattern.notes.len();
                match channel {
                    Some(c) => pattern.notes.retain(|n| n.channel != c),
                    None => pattern.notes.clear(),
                }
                let removed = before - pattern.notes.len();
                self.touch_schedule();
                Ok(json!({ "pattern": id, "removed": removed }))
            }
            "list_notes" => {
                let id = self.pattern_arg(args)?;
                let channel = args.u32_of("channel");
                let pattern = self.project.pattern(id).ok_or("no such pattern")?;
                let notes: Vec<Value> = pattern
                    .notes
                    .iter()
                    .filter(|n| channel.map(|c| n.channel == c).unwrap_or(true))
                    .map(|n| {
                        json!({
                            "id": n.id,
                            "channel": n.channel,
                            "key": n.key,
                            "name": crate::ui::note_name(n.key),
                            "start": n.start,
                            "length": n.length,
                            "velocity": n.velocity,
                            "pan": n.pan,
                            "fine_tune": n.fine_tune,
                        })
                    })
                    .collect();
                Ok(json!({ "pattern": id, "count": notes.len(), "notes": notes }))
            }
            "add_note" => {
                let pattern_id = self.pattern_arg(args)?;
                let channel = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
                let note = self.build_note(args, channel, pattern_id)?;
                self.checkpoint();
                let id = note.id;
                self.project
                    .pattern_mut(pattern_id)
                    .ok_or("no such pattern")?
                    .notes
                    .push(note);
                self.touch_schedule();
                Ok(json!({ "id": id, "pattern": pattern_id }))
            }
            "add_notes" => {
                let pattern_id = self.pattern_arg(args)?;
                let default_channel = args.u32_of("channel");
                let items = args
                    .get("notes")
                    .and_then(Value::as_array)
                    .ok_or_else(|| missing("notes"))?
                    .clone();
                // Build every note first: a bad entry halfway through must not
                // leave the pattern half written.
                let mut prepared = Vec::with_capacity(items.len());
                for item in &items {
                    let channel = item
                        .u32_of("channel")
                        .or(default_channel)
                        .ok_or("each note needs a channel, or pass a default channel")?;
                    prepared.push(self.build_note(item, channel, pattern_id)?);
                }
                self.checkpoint();
                let added: Vec<Id> = prepared.iter().map(|n| n.id).collect();
                self.project
                    .pattern_mut(pattern_id)
                    .ok_or("no such pattern")?
                    .notes
                    .extend(prepared);
                self.touch_schedule();
                Ok(json!({ "pattern": pattern_id, "added": added.len(), "ids": added }))
            }
            "update_note" => {
                let pattern_id = self.pattern_arg(args)?;
                let note_id = args.u32_of("note").ok_or_else(|| missing("note"))?;
                let channels: Vec<Id> = self.project.channels.iter().map(|c| c.id).collect();
                if self
                    .project
                    .pattern(pattern_id)
                    .and_then(|p| p.note(note_id))
                    .is_none()
                {
                    return Err(format!("no note with id {note_id}"));
                }
                self.checkpoint();
                let pattern = self
                    .project
                    .pattern_mut(pattern_id)
                    .ok_or("no such pattern")?;
                let note = pattern.note_mut(note_id).ok_or("no such note")?;
                if let Some(key) = args.u32_of("key") {
                    note.key = key.min(127) as u8;
                }
                if let Some(start) = args.f64_of("start") {
                    note.start = start.max(0.0);
                }
                if let Some(length) = args.f64_of("length") {
                    note.length = length.clamp(1.0 / 64.0, 512.0);
                }
                if let Some(velocity) = args.f32_of("velocity") {
                    note.velocity = velocity.clamp(0.0, 1.0);
                }
                if let Some(pan) = args.f32_of("pan") {
                    note.pan = pan.clamp(-1.0, 1.0);
                }
                if let Some(fine) = args.f32_of("fine_tune") {
                    note.fine_tune = fine.clamp(-100.0, 100.0);
                }
                if let Some(channel) = args.u32_of("channel") {
                    if !channels.contains(&channel) {
                        return Err(format!("no channel with id {channel}"));
                    }
                    note.channel = channel;
                }
                self.touch_schedule();
                Ok(json!({ "ok": true, "note": note_id }))
            }
            "remove_note" => {
                let pattern_id = self.pattern_arg(args)?;
                let note_id = args.u32_of("note").ok_or_else(|| missing("note"))?;
                self.checkpoint();
                let pattern = self
                    .project
                    .pattern_mut(pattern_id)
                    .ok_or("no such pattern")?;
                let before = pattern.notes.len();
                pattern.remove_note(note_id);
                if pattern.notes.len() == before {
                    return Err(format!("no note with id {note_id}"));
                }
                self.touch_schedule();
                Ok(json!({ "removed": note_id }))
            }
            "set_steps" => self.set_steps_tool(args),
            "transpose_notes" => {
                let pattern_id = self.pattern_arg(args)?;
                let semitones = args
                    .f64_of("semitones")
                    .ok_or_else(|| missing("semitones"))? as i32;
                let channel = args.u32_of("channel");
                self.checkpoint();
                let pattern = self
                    .project
                    .pattern_mut(pattern_id)
                    .ok_or("no such pattern")?;
                let mut moved = 0;
                for note in &mut pattern.notes {
                    if channel.map(|c| note.channel == c).unwrap_or(true) {
                        note.key = (note.key as i32 + semitones).clamp(0, 127) as u8;
                        moved += 1;
                    }
                }
                self.touch_schedule();
                Ok(json!({ "moved": moved }))
            }
            "quantize_notes" => {
                let pattern_id = self.pattern_arg(args)?;
                let grid = args
                    .f64_of("grid")
                    .ok_or_else(|| missing("grid"))?
                    .max(1.0 / 64.0);
                let strength = args.f64_of("strength").unwrap_or(1.0).clamp(0.0, 1.0);
                let channel = args.u32_of("channel");
                self.checkpoint();
                let pattern = self
                    .project
                    .pattern_mut(pattern_id)
                    .ok_or("no such pattern")?;
                let mut moved = 0;
                for note in &mut pattern.notes {
                    if channel.map(|c| note.channel == c).unwrap_or(true) {
                        let target = (note.start / grid).round() * grid;
                        note.start = (note.start + (target - note.start) * strength).max(0.0);
                        moved += 1;
                    }
                }
                self.touch_schedule();
                Ok(json!({ "quantized": moved, "grid": grid }))
            }
            "humanize_notes" => {
                let pattern_id = self.pattern_arg(args)?;
                let timing = args.f64_of("timing").unwrap_or(0.01).clamp(0.0, 0.5);
                let velocity = args.f32_of("velocity").unwrap_or(0.1).clamp(0.0, 1.0);
                let channel = args.u32_of("channel");
                self.checkpoint();
                let pattern = self
                    .project
                    .pattern_mut(pattern_id)
                    .ok_or("no such pattern")?;
                let mut touched = 0;
                for (index, note) in pattern.notes.iter_mut().enumerate() {
                    if !channel.map(|c| note.channel == c).unwrap_or(true) {
                        continue;
                    }
                    // Deterministic jitter: the same call always gives the same
                    // feel, so results are reproducible.
                    let seed = (note.id as u64).wrapping_mul(6_364_136_223_846_793_005)
                        ^ (index as u64).wrapping_mul(1_442_695_040_888_963_407);
                    let a = ((seed >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0;
                    let b = ((seed.rotate_left(17) >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0;
                    note.start = (note.start + a * timing).max(0.0);
                    note.velocity = (note.velocity + b as f32 * velocity).clamp(0.05, 1.0);
                    touched += 1;
                }
                self.touch_schedule();
                Ok(json!({ "humanized": touched }))
            }

            // ---- playlist -------------------------------------------------
            "list_clips" => Ok(json!({
                "tracks": self.project.playlist.tracks.iter().enumerate().map(|(i, t)| json!({
                    "index": i, "name": t.name, "muted": t.muted, "solo": t.solo,
                })).collect::<Vec<_>>(),
                "clips": self.project.playlist.clips.iter().map(|c| json!({
                    "id": c.id,
                    "track": c.track,
                    "pattern": match c.source { ClipSource::Pattern(p) => Some(p), _ => None },
                    "start": c.start,
                    "length": c.length,
                    "muted": c.muted,
                })).collect::<Vec<_>>(),
                "song_length_beats": self.project.song_length_beats(),
            })),
            "add_clip" => {
                let pattern = args.u32_of("pattern").ok_or_else(|| missing("pattern"))?;
                let track = args.usize_of("track").ok_or_else(|| missing("track"))?;
                let start = args
                    .f64_of("start")
                    .ok_or_else(|| missing("start"))?
                    .max(0.0);
                if self.project.pattern(pattern).is_none() {
                    return Err(format!("no pattern with id {pattern}"));
                }
                ensure_playlist_track(&mut self.project.playlist, track)?;
                let length = args
                    .f64_of("length")
                    .unwrap_or_else(|| self.project.pattern_length_beats(pattern))
                    .max(0.25);
                self.checkpoint();
                let id = self.project.add_clip(track, pattern, start, length);
                self.touch_schedule();
                Ok(json!({ "id": id, "track": track, "start": start, "length": length }))
            }
            "remove_clip" => {
                let id = args.u32_of("clip").ok_or_else(|| missing("clip"))?;
                self.checkpoint();
                let before = self.project.playlist.clips.len();
                self.project.playlist.remove_clip(id);
                if self.project.playlist.clips.len() == before {
                    return Err(format!("no clip with id {id}"));
                }
                self.touch_schedule();
                Ok(json!({ "removed": id }))
            }
            "move_clip" => {
                let id = args.u32_of("clip").ok_or_else(|| missing("clip"))?;
                let track_count = self.project.playlist.tracks.len();
                if self.project.playlist.clip(id).is_none() {
                    return Err(format!("no clip with id {id}"));
                }
                self.checkpoint();
                let clip = self.project.playlist.clip_mut(id).ok_or("no such clip")?;
                if let Some(track) = args.usize_of("track") {
                    clip.track = track.min(track_count.saturating_sub(1));
                }
                if let Some(start) = args.f64_of("start") {
                    clip.start = start.max(0.0);
                }
                if let Some(length) = args.f64_of("length") {
                    clip.length = length.max(0.25);
                }
                if let Some(muted) = args.bool_of("muted") {
                    clip.muted = muted;
                }
                self.touch_schedule();
                Ok(json!({ "ok": true, "clip": id }))
            }
            "set_playlist_track" => {
                let index = args.usize_of("track").ok_or_else(|| missing("track"))?;
                let colour = args.str_of("color").and_then(|c| parse_color(&c));
                ensure_playlist_track(&mut self.project.playlist, index)?;
                self.checkpoint();
                let track = &mut self.project.playlist.tracks[index];
                if let Some(name) = args.str_of("name") {
                    track.name = name;
                }
                if let Some(muted) = args.bool_of("muted") {
                    track.muted = muted;
                }
                if let Some(solo) = args.bool_of("solo") {
                    track.solo = solo;
                }
                if let Some(colour) = colour {
                    track.color = colour;
                }
                self.touch_schedule();
                Ok(json!({ "ok": true, "track": index }))
            }

            // ---- mixer ----------------------------------------------------
            "get_mixer" => Ok(json!({
                "tracks": self.project.mixer.tracks.iter().enumerate().map(|(i, t)| {
                    let (left, right) = self.engine.status().meter(i);
                    json!({
                        "index": i,
                        "name": t.name,
                        "volume": t.volume,
                        "pan": t.pan,
                        "muted": t.muted,
                        "solo": t.solo,
                        "peak_left": left,
                        "peak_right": right,
                        "effects": t.effects.iter().map(|e| e.name()).collect::<Vec<_>>(),
                        "sends": t.sends,
                    })
                }).collect::<Vec<_>>()
            })),
            "set_mixer_track" => {
                let index = args.usize_of("track").ok_or_else(|| missing("track"))?;
                if index >= self.project.mixer.tracks.len() {
                    return Err("no such mixer track".into());
                }
                self.checkpoint();
                let track = &mut self.project.mixer.tracks[index];
                if let Some(name) = args.str_of("name") {
                    track.name = name;
                }
                if let Some(volume) = args.f32_of("volume") {
                    track.volume = volume.clamp(0.0, 2.0);
                }
                if let Some(pan) = args.f32_of("pan") {
                    track.pan = pan.clamp(-1.0, 1.0);
                }
                if let Some(muted) = args.bool_of("muted") {
                    track.muted = muted;
                }
                if let Some(solo) = args.bool_of("solo") {
                    track.solo = solo;
                }
                self.touch_schedule();
                Ok(json!({ "ok": true, "track": index }))
            }
            "add_effect" => {
                let slug = args.str_of("kind").ok_or_else(|| missing("kind"))?;
                let effect = Effect::from_slug(&slug).ok_or_else(|| {
                    format!("unknown effect: {slug}. Try one of {:?}", EffectKind::SLUGS)
                })?;
                let host = self.fx_host(args)?;
                self.checkpoint();
                let position = args.usize_of("position");
                let chain = self.effect_chain_mut(host).ok_or("no such effect chain")?;
                let slot = match position {
                    Some(p) if p <= chain.len() => {
                        chain.insert(p, effect);
                        p
                    }
                    _ => {
                        chain.push(effect);
                        chain.len() - 1
                    }
                };
                self.touch_schedule();
                Ok(json!({ "slot": slot, "kind": slug }))
            }
            "remove_effect" => {
                let slot = args.usize_of("slot").ok_or_else(|| missing("slot"))?;
                let host = self.fx_host(args)?;
                self.checkpoint();
                let chain = self.effect_chain_mut(host).ok_or("no such effect chain")?;
                if slot >= chain.len() {
                    return Err(format!("no effect in slot {slot}"));
                }
                let removed = chain.remove(slot);
                self.touch_schedule();
                Ok(json!({ "removed": removed.name(), "slot": slot }))
            }
            "list_effects" => {
                let host = self.fx_host(args)?;
                let chain = self.effect_chain(host).ok_or("no such effect chain")?;
                Ok(json!({
                    "effects": chain.iter().enumerate().map(|(slot, effect)| json!({
                        "slot": slot,
                        "kind": effect.kind.slug(),
                        "name": effect.name(),
                        "enabled": effect.enabled,
                        "params": effect.kind.params().iter().map(|(name, value, min, max)| json!({
                            "name": name, "value": value, "min": min, "max": max,
                        })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>()
                }))
            }
            "set_effect_param" => {
                let slot = args.usize_of("slot").ok_or_else(|| missing("slot"))?;
                let host = self.fx_host(args)?;
                let param = args.str_of("param");
                let enabled = args.bool_of("enabled");
                let value = args.f32_of("value");
                let normalised = args.f32_of("normalised");
                let chain = self.effect_chain_mut(host).ok_or("no such effect chain")?;
                let effect = chain.get_mut(slot).ok_or("no effect in that slot")?;
                if let Some(enabled) = enabled {
                    effect.enabled = enabled;
                }
                if let Some(param) = &param {
                    let resolved = match (value, normalised) {
                        (Some(v), _) => v,
                        (None, Some(t)) => {
                            let (_, _, min, max) = effect
                                .kind
                                .params()
                                .into_iter()
                                .find(|(n, _, _, _)| n == param)
                                .ok_or_else(|| format!("unknown parameter: {param}"))?;
                            min + (max - min) * t.clamp(0.0, 1.0)
                        }
                        (None, None) => return Err("provide value or normalised".into()),
                    };
                    if !effect.kind.set_param(param, resolved) {
                        return Err(format!("unknown parameter: {param}"));
                    }
                }
                self.touch_schedule();
                Ok(json!({ "ok": true, "slot": slot }))
            }
            "move_effect" => {
                let slot = args.usize_of("slot").ok_or_else(|| missing("slot"))?;
                let to = args.usize_of("to").ok_or_else(|| missing("to"))?;
                let host = self.fx_host(args)?;
                let len = self.effect_chain(host).map(Vec::len).unwrap_or(0);
                if slot >= len {
                    return Err(format!("no effect in slot {slot}"));
                }
                if to >= len {
                    return Err(format!("slot {to} is past the end of the chain"));
                }
                self.checkpoint();
                if let Some(chain) = self.effect_chain_mut(host) {
                    let effect = chain.remove(slot);
                    chain.insert(to, effect);
                }
                self.touch_schedule();
                Ok(json!({ "ok": true, "from": slot, "to": to }))
            }
            "apply_effect_preset" => {
                let slot = args.usize_of("slot").ok_or_else(|| missing("slot"))?;
                let preset = args.str_of("preset").ok_or_else(|| missing("preset"))?;
                let host = self.fx_host(args)?;
                if self.effect_chain(host).and_then(|c| c.get(slot)).is_none() {
                    return Err(format!("no effect in slot {slot}"));
                }
                self.checkpoint();
                let applied = self
                    .effect_chain_mut(host)
                    .and_then(|c| c.get_mut(slot))
                    .map(|e| e.kind.apply_preset(&preset))
                    .unwrap_or(false);
                if !applied {
                    let available: Vec<&str> = self
                        .effect_chain(host)
                        .and_then(|c| c.get(slot))
                        .map(|e| e.kind.presets().iter().map(|(n, _)| *n).collect())
                        .unwrap_or_default();
                    return Err(format!(
                        "no preset called {preset}. Available: {}",
                        available.join(", ")
                    ));
                }
                self.touch_schedule();
                Ok(self.effect_json(host, slot))
            }
            "set_effect_params" => {
                let slot = args.usize_of("slot").ok_or_else(|| missing("slot"))?;
                let host = self.fx_host(args)?;
                if self.effect_chain(host).and_then(|c| c.get(slot)).is_none() {
                    return Err(format!("no effect in slot {slot}"));
                }
                // Resolve every name before touching anything, so one typo does
                // not leave the effect half set.
                let mut resolved: Vec<(String, f32)> = Vec::new();
                let ranges: Vec<(String, f32, f32)> = self
                    .effect_chain(host)
                    .and_then(|c| c.get(slot))
                    .map(|e| {
                        e.kind
                            .params()
                            .into_iter()
                            .map(|(n, _, lo, hi)| (n.to_string(), lo, hi))
                            .collect()
                    })
                    .unwrap_or_default();

                if let Some(Value::Object(direct)) = args.get("params") {
                    for (name, value) in direct {
                        let value = value
                            .as_f64()
                            .filter(|v| v.is_finite())
                            .ok_or_else(|| format!("{name} needs a finite number"))?;
                        if !ranges.iter().any(|(n, _, _)| n == name) {
                            return Err(format!("unknown parameter: {name}"));
                        }
                        resolved.push((name.clone(), value as f32));
                    }
                }
                if let Some(Value::Object(normalised)) = args.get("normalised") {
                    for (name, value) in normalised {
                        let t = value
                            .as_f64()
                            .filter(|v| v.is_finite())
                            .ok_or_else(|| format!("{name} needs a finite number"))?
                            as f32;
                        let (_, lo, hi) = ranges
                            .iter()
                            .find(|(n, _, _)| n == name)
                            .ok_or_else(|| format!("unknown parameter: {name}"))?;
                        resolved.push((name.clone(), lo + (hi - lo) * t.clamp(0.0, 1.0)));
                    }
                }

                self.checkpoint();
                if let Some(effect) = self.effect_chain_mut(host).and_then(|c| c.get_mut(slot)) {
                    if let Some(enabled) = args.bool_of("enabled") {
                        effect.enabled = enabled;
                    }
                    for (name, value) in &resolved {
                        let _ = effect.kind.set_param(name, *value);
                    }
                }
                self.touch_schedule();
                Ok(self.effect_json(host, slot))
            }
            "clear_effects" => {
                let host = self.fx_host(args)?;
                self.checkpoint();
                let removed = match self.effect_chain_mut(host) {
                    Some(chain) => {
                        let n = chain.len();
                        chain.clear();
                        n
                    }
                    None => 0,
                };
                self.selected_effect = None;
                self.touch_schedule();
                Ok(json!({ "removed": removed }))
            }
            "copy_effects" => {
                let source = match (args.u32_of("from_channel"), args.usize_of("from_track")) {
                    (Some(channel), _) => {
                        if self.project.channel(channel).is_none() {
                            return Err(format!("no channel with id {channel}"));
                        }
                        FxHost::Channel(channel)
                    }
                    (None, Some(track)) => {
                        if track >= self.project.mixer.tracks.len() {
                            return Err(format!("no mixer track {track}"));
                        }
                        FxHost::MixerTrack(track)
                    }
                    (None, None) => return Err(missing("from_track or from_channel")),
                };
                let destination = match (args.u32_of("to_channel"), args.usize_of("to_track")) {
                    (Some(channel), _) => {
                        if self.project.channel(channel).is_none() {
                            return Err(format!("no channel with id {channel}"));
                        }
                        FxHost::Channel(channel)
                    }
                    (None, Some(track)) => {
                        if track >= self.project.mixer.tracks.len() {
                            return Err(format!("no mixer track {track}"));
                        }
                        FxHost::MixerTrack(track)
                    }
                    (None, None) => return Err(missing("to_track or to_channel")),
                };
                let effects = self.effect_chain(source).cloned().unwrap_or_default();
                self.checkpoint();
                if let Some(chain) = self.effect_chain_mut(destination) {
                    if !args.bool_of("append").unwrap_or(false) {
                        chain.clear();
                    }
                    chain.extend(effects.iter().copied());
                }
                self.touch_schedule();
                Ok(json!({ "copied": effects.len() }))
            }
            "list_effect_kinds" => {
                let only = args.str_of("kind");
                let kinds: Vec<Value> = EffectKind::SLUGS
                    .iter()
                    .filter(|slug| only.as_deref().map(|k| k == **slug).unwrap_or(true))
                    .filter_map(|slug| {
                        let kind = EffectKind::from_slug(slug)?;
                        Some(json!({
                            "kind": slug,
                            "name": kind.name(),
                            "category": kind.category().label(),
                            "description": kind.description(),
                            "params": kind.params().iter().map(|(name, value, min, max)| json!({
                                "name": name,
                                "default": value,
                                "min": min,
                                "max": max,
                                "control": match kind.param_kind(name) {
                                    ParamKind::Toggle => "toggle",
                                    ParamKind::Stepped => "stepped",
                                    ParamKind::Choice(_) => "choice",
                                    ParamKind::Continuous => "continuous",
                                },
                                "choices": match kind.param_kind(name) {
                                    ParamKind::Choice(options) => Some(options.to_vec()),
                                    _ => None,
                                },
                            })).collect::<Vec<_>>(),
                            "presets": kind.presets().iter().map(|(n, _)| *n).collect::<Vec<_>>(),
                        }))
                    })
                    .collect();
                if kinds.is_empty() {
                    return Err(format!(
                        "unknown effect. Available: {}",
                        EffectKind::SLUGS.join(", ")
                    ));
                }
                Ok(json!({ "effects": kinds }))
            }
            "set_send" => {
                let from = args.usize_of("track").ok_or_else(|| missing("track"))?;
                let to = args
                    .usize_of("destination")
                    .ok_or_else(|| missing("destination"))?;
                let amount = args.f32_of("amount").ok_or_else(|| missing("amount"))?;
                let count = self.project.mixer.tracks.len();
                if from >= count || to >= count {
                    return Err("mixer track out of range".into());
                }
                if from == to {
                    return Err("a track cannot send to itself".into());
                }
                self.checkpoint();
                let track = &mut self.project.mixer.tracks[from];
                track.sends.retain(|(dst, _)| *dst != to);
                if amount > 0.001 {
                    track.sends.push((to, amount.clamp(0.0, 1.0)));
                }
                self.touch_schedule();
                Ok(json!({ "ok": true, "from": from, "to": to, "amount": amount }))
            }

            "set_instrument_params" => {
                let id = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
                let channel = self.project.channel(id).ok_or("no such channel")?;
                let ranges: Vec<(String, f32, f32)> = channel
                    .instrument
                    .params()
                    .into_iter()
                    .map(|p| (p.name.to_string(), p.min, p.max))
                    .collect();

                let mut resolved: Vec<(String, f32)> = Vec::new();
                if let Some(Value::Object(direct)) = args.get("params") {
                    for (name, value) in direct {
                        let value = value
                            .as_f64()
                            .filter(|v| v.is_finite())
                            .ok_or_else(|| format!("{name} needs a finite number"))?;
                        if !ranges.iter().any(|(n, _, _)| n == name) {
                            return Err(format!("unknown parameter: {name}"));
                        }
                        resolved.push((name.clone(), value as f32));
                    }
                }
                if let Some(Value::Object(normalised)) = args.get("normalised") {
                    for (name, value) in normalised {
                        let t = value
                            .as_f64()
                            .filter(|v| v.is_finite())
                            .ok_or_else(|| format!("{name} needs a finite number"))?
                            as f32;
                        let (_, lo, hi) = ranges
                            .iter()
                            .find(|(n, _, _)| n == name)
                            .ok_or_else(|| format!("unknown parameter: {name}"))?;
                        resolved.push((name.clone(), lo + (hi - lo) * t.clamp(0.0, 1.0)));
                    }
                }

                self.checkpoint();
                if let Some(channel) = self.project.channel_mut(id) {
                    for (name, value) in &resolved {
                        let _ = channel.instrument.set_param(name, *value);
                    }
                }
                self.touch_schedule();
                Ok(json!({ "channel": id, "set": resolved.len() }))
            }
            "clone_channel" => {
                let id = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
                let source = self.project.channel(id).cloned().ok_or("no such channel")?;
                self.checkpoint();
                let new_id = self.project.next_id();
                let mut clone = source.clone();
                clone.id = new_id;
                clone.name = args
                    .str_of("name")
                    .unwrap_or_else(|| format!("{} copy", source.name));
                let index = self.project.channel_index(id).unwrap_or(0);
                self.project.channels.insert(index + 1, clone);

                if args.bool_of("with_notes").unwrap_or(false) {
                    let patterns: Vec<Id> = self.project.patterns.iter().map(|p| p.id).collect();
                    for pattern_id in patterns {
                        let copies: Vec<Note> = self
                            .project
                            .pattern(pattern_id)
                            .map(|p| p.channel_notes(id).copied().collect())
                            .unwrap_or_default();
                        let mut renumbered = Vec::with_capacity(copies.len());
                        for mut note in copies {
                            note.id = self.project.next_id();
                            note.channel = new_id;
                            renumbered.push(note);
                        }
                        if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                            pattern.notes.extend(renumbered);
                        }
                    }
                }
                self.selected_channel = Some(new_id);
                self.touch_schedule();
                let summary = self
                    .project
                    .channel(new_id)
                    .map(|c| self.channel_json(c))
                    .unwrap_or(json!({ "id": new_id }));
                Ok(summary)
            }
            "move_channel" => {
                let id = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
                let to = args.usize_of("to").ok_or_else(|| missing("to"))?;
                let from = self
                    .project
                    .channel_index(id)
                    .ok_or_else(|| format!("no channel with id {id}"))?;
                let to = to.min(self.project.channels.len().saturating_sub(1));
                self.checkpoint();
                let channel = self.project.channels.remove(from);
                self.project.channels.insert(to, channel);
                self.touch_schedule();
                Ok(json!({ "channel": id, "from": from, "to": to }))
            }
            "clone_pattern" => {
                let id = self.pattern_arg(args)?;
                let source = self.project.pattern(id).cloned().ok_or("no such pattern")?;
                self.checkpoint();
                let new_id = self.project.next_id();
                let mut clone = source.clone();
                clone.id = new_id;
                clone.name = args
                    .str_of("name")
                    .unwrap_or_else(|| format!("{} copy", source.name));
                for note in &mut clone.notes {
                    note.id = self.project.next_id();
                }
                self.project.patterns.push(clone);
                self.selected_pattern = Some(new_id);
                self.touch_schedule();
                Ok(json!({ "id": new_id, "notes": source.notes.len() }))
            }
            "copy_notes" => {
                let from_pattern = self.pattern_arg(args)?;
                let to_pattern = match args.u32_of("to_pattern") {
                    Some(id) if self.project.pattern(id).is_some() => id,
                    Some(id) => return Err(format!("no pattern with id {id}")),
                    None => from_pattern,
                };
                let channel = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
                if self.project.channel(channel).is_none() {
                    return Err(format!("no channel with id {channel}"));
                }
                let to_channel = match args.u32_of("to_channel") {
                    Some(id) if self.project.channel(id).is_some() => id,
                    Some(id) => return Err(format!("no channel with id {id}")),
                    None => channel,
                };
                let offset = args.f64_of("offset").unwrap_or(0.0);
                let semitones = args.f64_of("semitones").unwrap_or(0.0) as i32;
                let scale = args.f32_of("velocity_scale").unwrap_or(1.0).clamp(0.0, 4.0);

                let source: Vec<Note> = self
                    .project
                    .pattern(from_pattern)
                    .map(|p| p.channel_notes(channel).copied().collect())
                    .unwrap_or_default();
                if source.is_empty() {
                    return Err("that channel has no notes in the source pattern".into());
                }
                self.checkpoint();
                let mut copies = Vec::with_capacity(source.len());
                for mut note in source {
                    note.id = self.project.next_id();
                    note.channel = to_channel;
                    note.start = (note.start + offset).max(0.0);
                    note.key = (note.key as i32 + semitones).clamp(0, 127) as u8;
                    note.velocity = (note.velocity * scale).clamp(0.0, 1.0);
                    copies.push(note);
                }
                let count = copies.len();
                if let Some(pattern) = self.project.pattern_mut(to_pattern) {
                    pattern.notes.extend(copies);
                }
                self.touch_schedule();
                Ok(json!({ "copied": count, "to_pattern": to_pattern, "to_channel": to_channel }))
            }
            "set_grid" => {
                let beats = args.f64_of("beats").ok_or_else(|| missing("beats"))?;
                self.settings.grid_snap = if beats <= 0.0 {
                    0.0
                } else {
                    beats.clamp(1.0 / 64.0, 8.0)
                };
                let _ = self.settings.save();
                Ok(json!({ "grid": self.settings.grid_snap }))
            }
            "set_recording" => {
                let enabled = args.bool_of("enabled").ok_or_else(|| missing("enabled"))?;
                self.recording = enabled;
                if !enabled {
                    self.release_all_previews();
                }
                Ok(json!({ "recording": enabled }))
            }
            "get_meters" => {
                let status = self.engine.status();
                Ok(json!({
                    "voices": status.voices(),
                    "cpu_load": status.cpu_load(),
                    "sample_rate": status.sample_rate(),
                    "playing": status.playing(),
                    "tracks": self.project.mixer.tracks.iter().enumerate().map(|(i, t)| {
                        let (left, right) = status.meter(i);
                        json!({
                            "index": i,
                            "name": t.name,
                            "peak_left": left,
                            "peak_right": right,
                            "peak_db": crate::audio::dsp::gain_to_db(left.max(right)),
                        })
                    }).collect::<Vec<_>>(),
                }))
            }

            // ---- automation -------------------------------------------------
            "add_automation" => self.add_automation_tool(args),
            "list_automation" => {
                let pattern_id = self.pattern_arg(args)?;
                let pattern = self.project.pattern(pattern_id).ok_or("no such pattern")?;
                Ok(json!({
                    "pattern": pattern_id,
                    "lanes": pattern.automation.iter().enumerate().map(|(i, lane)| json!({
                        "index": i,
                        "target": lane.target.label(),
                        "enabled": lane.enabled,
                        "points": lane.points.iter().map(|p| json!({
                            "beat": p.beat, "value": p.value, "curve": format!("{:?}", p.curve).to_lowercase(),
                        })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>()
                }))
            }
            "clear_automation" => {
                let pattern_id = self.pattern_arg(args)?;
                self.checkpoint();
                let pattern = self
                    .project
                    .pattern_mut(pattern_id)
                    .ok_or("no such pattern")?;
                if args.bool_of("all").unwrap_or(false) {
                    let count = pattern.automation.len();
                    pattern.automation.clear();
                    self.touch_schedule();
                    return Ok(json!({ "removed": count }));
                }
                let index = args.usize_of("index").ok_or("pass index, or all:true")?;
                if index >= pattern.automation.len() {
                    return Err("no lane at that index".into());
                }
                pattern.automation.remove(index);
                self.touch_schedule();
                Ok(json!({ "removed": 1 }))
            }

            // ---- soundfont and samples --------------------------------------
            "load_soundfont" => {
                let path = PathBuf::from(args.str_of("path").ok_or_else(|| missing("path"))?);
                self.apply_soundfont(path.clone())?;
                Ok(json!({
                    "path": path.to_string_lossy(),
                    "presets": self.soundfont_presets.len(),
                }))
            }
            "list_soundfont_presets" => {
                if self.soundfont.is_none() {
                    return Err("no soundfont is loaded — call load_soundfont first".into());
                }
                let filter = args.str_of("filter").unwrap_or_default().to_lowercase();
                let presets: Vec<Value> = self
                    .soundfont_presets
                    .iter()
                    .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter))
                    .map(|p| json!({ "bank": p.bank, "program": p.program, "name": p.name }))
                    .collect();
                Ok(json!({ "count": presets.len(), "presets": presets }))
            }
            "export_midi" => {
                #[cfg(target_arch = "wasm32")]
                {
                    Err("MIDI export is only available in the desktop app".to_string())
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let pattern_id = self.pattern_arg(args)?;
                    let pattern = self.project.pattern(pattern_id).ok_or("no such pattern")?;
                    let bytes = crate::persist::pattern_to_midi(&self.project, pattern);
                    let path = match args.str_of("path") {
                        Some(raw) => {
                            let p = PathBuf::from(&raw);
                            if p.is_absolute() {
                                p
                            } else {
                                crate::paths::renders_dir().join(raw)
                            }
                        }
                        None => crate::paths::renders_dir()
                            .join(format!("{}.mid", crate::actions::safe_name(&pattern.name))),
                    };
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&path, &bytes)
                        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
                    Ok(json!({ "path": path.to_string_lossy(), "bytes": bytes.len() }))
                }
            }
            "import_midi" => {
                #[cfg(target_arch = "wasm32")]
                {
                    Err("MIDI import is only available in the desktop app".to_string())
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let raw = args.str_of("path").ok_or_else(|| missing("path"))?;
                    let path = PathBuf::from(&raw);
                    let path = if path.is_absolute() {
                        path
                    } else {
                        crate::paths::projects_dir().join(raw)
                    };
                    let bytes = std::fs::read(&path)
                        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
                    let pattern_id = self.pattern_arg(args)?;
                    let channel = match args.u32_of("channel") {
                        Some(c) => c,
                        None => self
                            .selected_channel_id()
                            .ok_or("there are no channels — call add_channel first")?,
                    };
                    if self.project.channel(channel).is_none() {
                        return Err(format!("no channel with id {channel}"));
                    }
                    let mut next = self.project.next_id();
                    let (notes, tempo) = crate::persist::midi_to_notes(&bytes, channel, &mut next)
                        .map_err(|e| e.to_string())?;
                    self.checkpoint();
                    if args.bool_of("use_tempo").unwrap_or(true)
                        && let Some(tempo) = tempo
                    {
                        self.project.tempo = tempo.clamp(20.0, 999.0);
                        self.tempo_text = format!("{:.2}", self.project.tempo);
                    }
                    let replace = args.bool_of("replace").unwrap_or(false);
                    let count = notes.len();
                    // Re-key against the live counter so ids stay unique.
                    let mut placed = Vec::with_capacity(count);
                    for mut note in notes {
                        note.id = self.project.next_id();
                        placed.push(note);
                    }
                    if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                        if replace {
                            pattern.notes.retain(|n| n.channel != channel);
                        }
                        pattern.notes.extend(placed);
                    }
                    self.touch_schedule();
                    Ok(json!({
                        "pattern": pattern_id,
                        "channel": channel,
                        "notes": count,
                        "tempo": tempo,
                    }))
                }
            }
            "list_samples" => Ok(json!({
                "built_in": DrumModel::ALL.iter().map(|m| m.label()).collect::<Vec<_>>(),
                "directory": crate::paths::samples_dir().to_string_lossy(),
                "files": crate::paths::discover_samples().iter()
                    .map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
            })),

            // ---- interface ---------------------------------------------------
            "set_theme" => {
                let theme = args.str_of("theme").ok_or_else(|| missing("theme"))?;
                let choice = match theme.as_str() {
                    "light" => ThemeChoice::Light,
                    "dark" => ThemeChoice::Dark,
                    _ => ThemeChoice::System,
                };
                self.settings.theme = choice;
                self.theme = crate::theme::resolve(choice, self.system_dark);
                let _ = self.settings.save();
                Ok(json!({ "theme": theme }))
            }
            "set_view" => {
                let view = args.str_of("view").ok_or_else(|| missing("view"))?;
                let target =
                    MainView::from_slug(&view).ok_or_else(|| format!("unknown view: {view}"))?;
                self.view = target;
                Ok(json!({ "view": view }))
            }
            "undo" => {
                if self.undo_stack_len() == 0 {
                    return Err("nothing to undo".into());
                }
                self.apply_undo();
                Ok(json!({ "ok": true }))
            }
            "redo" => {
                if !self.apply_redo() {
                    return Err("nothing to redo".into());
                }
                Ok(json!({ "ok": true }))
            }

            other => Err(format!(
                "unknown tool: {other}. Call tools/list to see what is available."
            )),
        }
    }

    // -----------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------

    fn pattern_arg(&self, args: &Value) -> Result<Id, String> {
        match args.u32_of("pattern") {
            Some(id) => {
                if self.project.pattern(id).is_some() {
                    Ok(id)
                } else {
                    Err(format!("no pattern with id {id}"))
                }
            }
            None => self
                .selected_pattern_id()
                .ok_or_else(|| "there are no patterns — call add_pattern first".to_string()),
        }
    }

    fn fx_host(&self, args: &Value) -> Result<FxHost, String> {
        if let Some(channel) = args.u32_of("channel") {
            if self.project.channel(channel).is_none() {
                return Err(format!("no channel with id {channel}"));
            }
            return Ok(FxHost::Channel(channel));
        }
        let track = args.usize_of("track").unwrap_or(0);
        if track >= self.project.mixer.tracks.len() {
            return Err(format!("no mixer track {track}"));
        }
        Ok(FxHost::MixerTrack(track))
    }

    fn build_note(&mut self, args: &Value, channel: Id, _pattern: Id) -> Result<Note, String> {
        if self.project.channel(channel).is_none() {
            return Err(format!("no channel with id {channel}"));
        }
        let key = args.u32_of("key").ok_or_else(|| missing("key"))?.min(127) as u8;
        let start = args
            .f64_of("start")
            .ok_or_else(|| missing("start"))?
            .max(0.0);
        let length = args
            .f64_of("length")
            .unwrap_or(0.25)
            .clamp(1.0 / 64.0, 512.0);
        let velocity = args.f32_of("velocity").unwrap_or(0.78).clamp(0.0, 1.0);
        let pan = args.f32_of("pan").unwrap_or(0.0).clamp(-1.0, 1.0);
        let fine_tune = args.f32_of("fine_tune").unwrap_or(0.0).clamp(-100.0, 100.0);
        Ok(Note {
            id: self.project.next_id(),
            channel,
            key,
            start,
            length,
            velocity,
            pan,
            fine_tune,
            release: 0.0,
        })
    }

    /// One effect, described the way `list_effects` describes it.
    fn effect_json(&self, host: FxHost, slot: usize) -> Value {
        let Some(effect) = self.effect_chain(host).and_then(|c| c.get(slot)) else {
            return json!({ "slot": slot });
        };
        json!({
            "slot": slot,
            "kind": effect.kind.slug(),
            "name": effect.name(),
            "enabled": effect.enabled,
            "params": effect.kind.params().iter().map(|(name, value, min, max)| json!({
                "name": name, "value": value, "min": min, "max": max,
            })).collect::<Vec<_>>(),
        })
    }

    fn channel_json(&self, channel: &Channel) -> Value {
        let instrument = match &channel.instrument {
            Instrument::Synth(_) => json!({ "kind": "synth" }),
            Instrument::Sampler(p) => match &p.source {
                SampleSource::Model(m) => json!({ "kind": "sampler", "drum": m.label() }),
                SampleSource::File { path } => json!({ "kind": "sampler", "sample": path }),
            },
            Instrument::SoundFont(p) => {
                json!({ "kind": "soundfont", "bank": p.bank, "program": p.preset })
            }
        };
        json!({
            "id": channel.id,
            "name": channel.name,
            "color": color_to_hex(channel.color),
            "instrument": instrument,
            "volume": channel.volume,
            "pan": channel.pan,
            "muted": channel.muted,
            "solo": channel.solo,
            "transpose": channel.transpose,
            "mixer_track": channel.mixer_track,
            "step_key": channel.step_key,
            "effects": channel.effects.iter().map(|e| e.name()).collect::<Vec<_>>(),
        })
    }

    fn state_summary(&self) -> Value {
        let status = self.engine.status();
        let beats_per_bar = self.project.beats_per_bar();
        json!({
            "project": {
                "name": self.project.name,
                "path": self.project_path.as_ref().map(|p| p.to_string_lossy().to_string()),
                "unsaved_changes": self.dirty,
                "tempo": self.project.tempo,
                "time_signature": [self.project.time_signature.numerator, self.project.time_signature.denominator],
                "swing": self.project.swing,
                "master_volume": self.project.master_volume,
                "song_length_beats": self.project.song_length_beats(),
            },
            "transport": {
                "playing": status.playing(),
                "position_beats": status.position(),
                "position": crate::ui::format_position(status.position(), beats_per_bar),
                "mode": match self.play_mode { PlayMode::Song => "song", PlayMode::Pattern(_) => "pattern" },
                "loop": self.project.playlist.loop_region,
                "metronome": self.settings.metronome,
            },
            "engine": {
                "sample_rate": self.engine.sample_rate(),
                "voices": status.voices(),
                "silent": self.engine.silent,
            },
            "selection": {
                "channel": self.selected_channel_id(),
                "pattern": self.selected_pattern_id(),
                "mixer_track": self.selected_mixer_track,
                "view": self.view.slug(),
            },
            "channels": self.project.channels.iter().map(|c| self.channel_json(c)).collect::<Vec<_>>(),
            "patterns": self.project.patterns.iter().map(|p| json!({
                "id": p.id,
                "name": p.name,
                "length_beats": p.length_beats(beats_per_bar),
                "notes": p.notes.len(),
            })).collect::<Vec<_>>(),
            "playlist_clips": self.project.playlist.clips.len(),
            "soundfont": self.soundfont_path.as_ref().map(|p| p.to_string_lossy().to_string()),
        })
    }

    fn add_channel_tool(&mut self, args: &Value) -> Result<Value, String> {
        let kind = args.str_of("instrument").unwrap_or_else(|| "synth".into());
        self.checkpoint();
        let new_kind = match kind.as_str() {
            "sampler" => {
                let drum = args.str_of("drum").and_then(|name| {
                    DrumModel::ALL
                        .into_iter()
                        .find(|m| m.label().eq_ignore_ascii_case(&name))
                });
                match drum {
                    Some(model) => NewChannelKind::Drum(model),
                    None => NewChannelKind::EmptySampler,
                }
            }
            "soundfont" => NewChannelKind::SoundFont,
            _ => NewChannelKind::Synth(SynthPreset::from_name(
                &args.str_of("preset").unwrap_or_default(),
            )),
        };
        let id = self.add_channel_of(new_kind);

        if let Some(name) = args.str_of("name")
            && let Some(channel) = self.project.channel_mut(id)
        {
            channel.name = name;
        }
        if kind == "soundfont" {
            let bank = args.u32_of("bank").unwrap_or(0) as u16;
            let program = args.u32_of("program").unwrap_or(0) as u16;
            if let Some(channel) = self.project.channel_mut(id) {
                channel.instrument = Instrument::SoundFont(SoundFontPatch {
                    bank,
                    preset: program,
                    percussion: bank == 128,
                });
            }
        }
        if let Some(path) = args.str_of("sample_path") {
            self.load_sample_now(id, &path)?;
        }
        if let Some(track) = args.usize_of("mixer_track") {
            let max = self.project.mixer.tracks.len().saturating_sub(1);
            if let Some(channel) = self.project.channel_mut(id) {
                channel.mixer_track = track.min(max);
            }
        }
        self.selected_channel = Some(id);
        self.touch_schedule();
        let summary = self
            .project
            .channel(id)
            .map(|c| self.channel_json(c))
            .unwrap_or(json!({ "id": id }));
        Ok(summary)
    }

    fn set_channel_tool(&mut self, args: &Value) -> Result<Value, String> {
        let id = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
        let colour = args.str_of("color").and_then(|c| parse_color(&c));
        let track_count = self.project.mixer.tracks.len();
        if self.project.channel(id).is_none() {
            return Err(format!("no channel with id {id}"));
        }
        self.checkpoint();
        let channel = self.project.channel_mut(id).ok_or("no such channel")?;
        if let Some(name) = args.str_of("name") {
            channel.name = name;
        }
        if let Some(volume) = args.f32_of("volume") {
            channel.volume = volume.clamp(0.0, 2.0);
        }
        if let Some(pan) = args.f32_of("pan") {
            channel.pan = pan.clamp(-1.0, 1.0);
        }
        if let Some(muted) = args.bool_of("muted") {
            channel.muted = muted;
        }
        if let Some(solo) = args.bool_of("solo") {
            channel.solo = solo;
        }
        if let Some(transpose) = args.f32_of("transpose") {
            channel.transpose = transpose.clamp(-48.0, 48.0);
        }
        if let Some(track) = args.usize_of("mixer_track") {
            channel.mixer_track = track.min(track_count.saturating_sub(1));
        }
        if let Some(key) = args.u32_of("step_key") {
            channel.step_key = key.min(127) as u8;
        }
        if let Some(colour) = colour {
            channel.color = colour;
        }
        self.touch_schedule();
        let summary = self
            .project
            .channel(id)
            .map(|c| self.channel_json(c))
            .unwrap_or(json!({ "id": id }));
        Ok(summary)
    }

    fn set_instrument_tool(&mut self, args: &Value) -> Result<Value, String> {
        let id = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
        let kind = args
            .str_of("instrument")
            .ok_or_else(|| missing("instrument"))?;
        if self.project.channel(id).is_none() {
            return Err(format!("no channel with id {id}"));
        }
        self.checkpoint();
        let instrument = match kind.as_str() {
            "sampler" => {
                let drum = args.str_of("drum").and_then(|name| {
                    DrumModel::ALL
                        .into_iter()
                        .find(|m| m.label().eq_ignore_ascii_case(&name))
                });
                match drum {
                    Some(model) => Instrument::Sampler(SamplerPatch::from_model(model)),
                    None => Instrument::Sampler(SamplerPatch::default()),
                }
            }
            "soundfont" => Instrument::SoundFont(SoundFontPatch {
                bank: args.u32_of("bank").unwrap_or(0) as u16,
                preset: args.u32_of("program").unwrap_or(0) as u16,
                percussion: args.u32_of("bank").unwrap_or(0) == 128,
            }),
            "synth" => Instrument::Synth(
                SynthPreset::from_name(&args.str_of("preset").unwrap_or_default()).patch(),
            ),
            other => return Err(format!("unknown instrument: {other}")),
        };
        if let Some(channel) = self.project.channel_mut(id) {
            channel.instrument = instrument;
        }
        if let Some(path) = args.str_of("sample_path") {
            self.load_sample_now(id, &path)?;
        }
        self.touch_schedule();
        let summary = self
            .project
            .channel(id)
            .map(|c| self.channel_json(c))
            .unwrap_or(json!({ "id": id }));
        Ok(summary)
    }

    fn load_sample_now(&mut self, channel: Id, path: &str) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let resolved = resolve_sample_path(path);
            if !self.bank.contains(resolved.to_string_lossy().as_ref()) {
                let sample = crate::audio::samples::load_wav(&resolved)
                    .map_err(|e| format!("could not load {}: {e}", resolved.display()))?;
                self.bank.insert(
                    resolved.to_string_lossy().to_string(),
                    std::sync::Arc::new(sample),
                );
            }
            let key = resolved.to_string_lossy().to_string();
            if let Some(c) = self.project.channel_mut(channel) {
                match &mut c.instrument {
                    Instrument::Sampler(p) => p.source = SampleSource::File { path: key },
                    other => {
                        *other = Instrument::Sampler(SamplerPatch {
                            source: SampleSource::File { path: key },
                            ..SamplerPatch::default()
                        });
                    }
                }
            }
            Ok(())
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (channel, path);
            Err("loading samples is only available in the desktop app".into())
        }
    }

    fn set_steps_tool(&mut self, args: &Value) -> Result<Value, String> {
        let pattern_id = self.pattern_arg(args)?;
        let channel = args.u32_of("channel").ok_or_else(|| missing("channel"))?;
        if self.project.channel(channel).is_none() {
            return Err(format!("no channel with id {channel}"));
        }
        let step_beats = args.f64_of("step_beats").unwrap_or(0.25).max(1.0 / 64.0);
        let velocity = args.f32_of("velocity").unwrap_or(0.78).clamp(0.0, 1.0);
        let replace = args.bool_of("replace").unwrap_or(true);

        let steps: Vec<bool> = match args.get("steps") {
            Some(Value::String(pattern)) => pattern
                .chars()
                .filter(|c| !c.is_whitespace() && *c != '|')
                .map(|c| matches!(c, 'x' | 'X' | '1' | '#' | '*' | 'o' | 'O'))
                .collect(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| match v {
                    Value::Bool(b) => *b,
                    Value::Number(n) => n.as_f64().unwrap_or(0.0) >= 0.5,
                    Value::String(s) => matches!(s.as_str(), "x" | "X" | "1"),
                    _ => false,
                })
                .collect(),
            _ => return Err(missing("steps")),
        };

        self.checkpoint();
        let key = self
            .project
            .channel(channel)
            .map(|c| c.step_key)
            .unwrap_or(60);
        let mut ids = Vec::new();
        {
            let pattern = self
                .project
                .pattern_mut(pattern_id)
                .ok_or("no such pattern")?;
            if replace {
                pattern.notes.retain(|n| n.channel != channel);
            }
        }
        for (index, on) in steps.iter().enumerate() {
            if !on {
                continue;
            }
            let id = self.project.next_id();
            ids.push(id);
            let beat = index as f64 * step_beats;
            if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                pattern.notes.push(Note {
                    id,
                    channel,
                    key,
                    start: beat,
                    length: step_beats,
                    velocity,
                    pan: 0.0,
                    fine_tune: 0.0,
                    release: 0.0,
                });
            }
        }
        self.touch_schedule();
        Ok(json!({
            "pattern": pattern_id,
            "channel": channel,
            "steps": steps.len(),
            "hits": ids.len(),
        }))
    }

    fn add_automation_tool(&mut self, args: &Value) -> Result<Value, String> {
        let pattern_id = self.pattern_arg(args)?;
        let target_value = args.get("target").ok_or_else(|| missing("target"))?;
        let target = self.parse_target(target_value)?;
        let points = args
            .get("points")
            .and_then(Value::as_array)
            .ok_or_else(|| missing("points"))?
            .clone();
        let replace = args.bool_of("replace").unwrap_or(false);

        self.checkpoint();
        let pattern = self
            .project
            .pattern_mut(pattern_id)
            .ok_or("no such pattern")?;
        let lane = pattern.lane_mut(target);
        if replace {
            lane.points.clear();
        }
        let mut added = 0;
        for point in &points {
            let beat = point
                .f64_of("beat")
                .ok_or_else(|| missing("beat"))?
                .max(0.0);
            let value = point
                .f64_of("value")
                .ok_or_else(|| missing("value"))?
                .clamp(0.0, 1.0);
            let curve = curve_from(&point.str_of("curve").unwrap_or_default());
            lane.points.push(AutomationPoint { beat, value, curve });
            added += 1;
        }
        lane.sort();
        let total = lane.points.len();
        self.touch_schedule();
        Ok(json!({
            "pattern": pattern_id,
            "target": target.label(),
            "added": added,
            "points": total,
        }))
    }

    fn parse_target(&self, value: &Value) -> Result<AutomationTarget, String> {
        let kind = value.str_of("kind").ok_or_else(|| missing("target.kind"))?;
        let channel = value.u32_of("channel");
        let track = value.usize_of("track");
        let slot = value.usize_of("slot");
        let param = value.str_of("param");

        let target = match kind.as_str() {
            "tempo" => AutomationTarget::Tempo,
            "master_volume" => AutomationTarget::MasterVolume,
            "channel_volume" => {
                AutomationTarget::ChannelVolume(channel.ok_or_else(|| missing("target.channel"))?)
            }
            "channel_pan" => {
                AutomationTarget::ChannelPan(channel.ok_or_else(|| missing("target.channel"))?)
            }
            "mixer_volume" => {
                AutomationTarget::MixerVolume(track.ok_or_else(|| missing("target.track"))?)
            }
            "mixer_pan" => {
                AutomationTarget::MixerPan(track.ok_or_else(|| missing("target.track"))?)
            }
            "mixer_effect" => AutomationTarget::MixerEffect {
                track: track.ok_or_else(|| missing("target.track"))?,
                slot: slot.ok_or_else(|| missing("target.slot"))?,
                param: ParamRef::new(&param.ok_or_else(|| missing("target.param"))?),
            },
            "channel_effect" => AutomationTarget::ChannelEffect {
                channel: channel.ok_or_else(|| missing("target.channel"))?,
                slot: slot.ok_or_else(|| missing("target.slot"))?,
                param: ParamRef::new(&param.ok_or_else(|| missing("target.param"))?),
            },
            "instrument" => AutomationTarget::Instrument {
                channel: channel.ok_or_else(|| missing("target.channel"))?,
                param: ParamRef::new(&param.ok_or_else(|| missing("target.param"))?),
            },
            other => return Err(format!("unknown automation target: {other}")),
        };

        // Fail loudly rather than silently automating something that is not there.
        match target {
            AutomationTarget::ChannelVolume(id)
            | AutomationTarget::ChannelPan(id)
            | AutomationTarget::ChannelEffect { channel: id, .. }
            | AutomationTarget::Instrument { channel: id, .. } => {
                if self.project.channel(id).is_none() {
                    return Err(format!("no channel with id {id}"));
                }
            }
            AutomationTarget::MixerVolume(t)
            | AutomationTarget::MixerPan(t)
            | AutomationTarget::MixerEffect { track: t, .. }
                if t >= self.project.mixer.tracks.len() =>
            {
                return Err(format!("no mixer track {t}"));
            }
            _ => {}
        }
        Ok(target)
    }

    fn export_wav(&mut self, args: &Value) -> Result<Value, String> {
        // The interface intercepts this tool and runs it on a worker; reaching
        // here means the platform cannot render at all.
        let _ = args;
        Err("rendering to a file is only available in the desktop app".into())
    }

    /// Start an MCP-requested render on a worker thread and answer from there.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn export_wav_for_mcp(
        &mut self,
        job: crate::app::SharedJob,
        args: &Value,
    ) -> iced::Task<crate::app::Message> {
        let mode = match args
            .str_of("mode")
            .unwrap_or_else(|| "song".into())
            .as_str()
        {
            "pattern" => match self.selected_pattern_id() {
                Some(id) => PlayMode::Pattern(id),
                None => {
                    crate::app::respond(job, Err("there is no pattern to render".into()));
                    return iced::Task::none();
                }
            },
            _ => PlayMode::Song,
        };
        let tail = args.f32_of("tail_seconds").unwrap_or(2.0).clamp(0.0, 30.0);
        let path = match args.str_of("path") {
            Some(raw) => {
                let p = PathBuf::from(&raw);
                if p.is_absolute() {
                    p
                } else {
                    crate::paths::renders_dir().join(raw)
                }
            }
            None => crate::paths::renders_dir().join(format!(
                "{}.wav",
                crate::actions::safe_name(&self.project.name)
            )),
        };

        if self.needs_sync {
            self.sync_engine();
        }
        let snapshot = self.build_render_snapshot(mode);
        let length = snapshot.schedule.length;
        if length <= 0.0 {
            crate::app::respond(job, Err("there is nothing to render".into()));
            return iced::Task::none();
        }
        let synths = self.build_render_synths_public();
        let sample_rate = 44_100.0;
        let progress = self.render_progress.clone();
        progress.reset();
        self.note("Rendering for MCP…");

        let snapshot = std::sync::Arc::new(snapshot);
        let reply_path = path.clone();
        std::thread::Builder::new()
            .name("limedaw-mcp-render".into())
            .spawn(move || {
                let samples = crate::audio::render_offline_with_progress(
                    snapshot,
                    synths,
                    sample_rate,
                    length,
                    tail,
                    Some(&progress),
                );
                let frames = samples.len() / 2;
                let result = crate::persist::write_wav(&reply_path, &samples, sample_rate as u32)
                    .map(|_| {
                        json!({
                            "path": reply_path.to_string_lossy(),
                            "seconds": frames as f64 / sample_rate as f64,
                            "sample_rate": sample_rate,
                        })
                    })
                    .map_err(|e| format!("could not write {}: {e}", reply_path.display()));
                crate::app::respond(job, result);
            })
            .expect("spawning the render thread");

        iced::Task::none()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_project_path(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() || raw.starts_with("./") || raw.starts_with("../") {
        return path;
    }
    let mut candidate = crate::paths::projects_dir().join(raw);
    if candidate.extension().is_none() {
        candidate.set_extension("lime");
    }
    candidate
}

#[cfg(target_arch = "wasm32")]
fn resolve_project_path(raw: &str) -> PathBuf {
    PathBuf::from(raw)
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_sample_path(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() || path.exists() {
        return path;
    }
    crate::paths::samples_dir().join(raw)
}
