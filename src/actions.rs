//! The bulk of the update logic, kept apart from the state definition.

use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use iced::Task;

use crate::app::*;
use crate::audio::EngineCmd;
use crate::audio::schedule::PlayMode;
use crate::model::*;
use crate::theme;

impl LimeDaw {
    pub(crate) fn update_action(&mut self, message: Message) -> Task<Message> {
        // Choosing anything from a menu dismisses it. Toggling a menu button is
        // handled below, so it is excluded here.
        if !matches!(
            message,
            Message::ToggleMenu(_) | Message::Tick | Message::Keyboard(_) | Message::McpJob(_)
        ) {
            self.open_menu = None;
        }
        match message {
            Message::Tick | Message::SystemTheme(_) | Message::Keyboard(_) | Message::Noop => {}

            // ---- transport -------------------------------------------
            Message::TogglePlay => {
                self.engine.resume();
                self.engine.send(EngineCmd::TogglePlay);
            }
            Message::Play => {
                self.engine.resume();
                self.engine.send(EngineCmd::Play);
            }
            Message::Pause => self.engine.send(EngineCmd::Pause),
            Message::Stop => {
                self.engine.send(EngineCmd::Stop);
                self.release_all_previews();
                // Stopping resets the effect state, so the engine needs its
                // processors again.
                self.forget_effect_chains();
                self.needs_sync = true;
            }
            Message::Seek(beat) => self.engine.send(EngineCmd::Seek(beat.max(0.0))),
            Message::SetTempo(bpm) => {
                self.project.tempo = bpm.clamp(20.0, 999.0);
                self.tempo_text = format!("{:.2}", self.project.tempo);
                self.engine.send(EngineCmd::SetTempo(self.project.tempo));
                self.touch();
            }
            Message::TempoInput(text) => {
                self.tempo_text = text;
            }
            Message::TempoCommit => {
                if let Ok(bpm) = self.tempo_text.trim().parse::<f64>() {
                    return self.update_action(Message::SetTempo(bpm));
                }
                self.tempo_text = format!("{:.2}", self.project.tempo);
            }
            Message::ToggleMetronome => {
                self.settings.metronome = !self.settings.metronome;
                self.engine
                    .send(EngineCmd::SetMetronome(self.settings.metronome));
                let _ = self.settings.save();
            }
            Message::ToggleRecord => {
                self.recording = !self.recording;
                if self.recording {
                    self.note("Recording armed — played notes go into the selected pattern");
                } else {
                    self.release_all_previews();
                    self.note("Recording off");
                }
            }
            Message::ToggleLoop => {
                let enabled = self.project.playlist.loop_region.is_none();
                if enabled {
                    let end = self.project.song_length_beats();
                    self.project.playlist.loop_region = Some((0.0, end));
                } else {
                    self.project.playlist.loop_region = None;
                }
                self.touch_schedule();
            }
            Message::SetPlayMode(mode) => {
                self.play_mode = mode;
                self.engine.send(EngineCmd::SetMode(mode));
                // A different mode is a different arrangement of the same
                // notes, so the schedule has to be built again.
                self.schedule_dirty = true;
                self.needs_sync = true;
                // Song mode belongs with the song editor, pattern mode with
                // the rack — switch so the transport and the view agree.
                self.view = match mode {
                    PlayMode::Song => MainView::Playlist,
                    PlayMode::Pattern(_) => {
                        if self.view == MainView::Playlist {
                            MainView::Channels
                        } else {
                            self.view
                        }
                    }
                };
            }
            Message::SetMasterVolume(v) => {
                self.project.master_volume = v.clamp(0.0, 2.0);
                self.touch();
            }
            Message::SetSwing(v) => {
                self.project.swing = v.clamp(0.0, 0.9);
                self.touch_schedule();
            }
            Message::SetTimeSignature(n, d) => {
                self.project.time_signature = TimeSignature {
                    numerator: n,
                    denominator: d,
                };
                self.project.sanitise();
                self.touch();
            }
            Message::Panic => {
                self.engine.send(EngineCmd::Panic);
                self.release_all_previews();
                self.forget_effect_chains();
                self.needs_sync = true;
                self.note("All notes stopped");
            }

            // ---- menus -----------------------------------------------
            Message::ToggleMenu(id) => {
                self.open_menu = if self.open_menu == Some(id) {
                    None
                } else {
                    Some(id)
                };
            }
            Message::CloseMenu => self.open_menu = None,
            Message::OpenProjectFolder => return open_folder(crate::paths::projects_dir()),
            Message::Quit => {
                let _ = self.settings.save();
                return iced::exit();
            }

            // ---- view -------------------------------------------------
            Message::SetView(view) => {
                self.view = view;
                if view == MainView::PianoRoll
                    && let Some(pattern) = self.selected_pattern_id()
                {
                    self.play_mode = PlayMode::Pattern(pattern);
                    self.engine.send(EngineCmd::SetMode(self.play_mode));
                    self.needs_sync = true;
                }
            }
            Message::ToggleBrowser => {
                self.settings.show_browser = !self.settings.show_browser;
                let _ = self.settings.save();
            }
            Message::ToggleInstrumentPanel => {
                self.settings.show_instrument_panel = !self.settings.show_instrument_panel;
                let _ = self.settings.save();
            }
            Message::SetBrowserTab(tab) => self.browser_tab = tab,
            Message::SetTheme(choice) => {
                self.settings.theme = choice;
                self.theme = theme::resolve(choice, self.system_dark);
                let _ = self.settings.save();
            }
            Message::SetRenderer(choice) => {
                self.settings.renderer = choice;
                let _ = self.settings.save();
                self.note("Renderer changes take effect the next time LimeDAW starts");
            }
            Message::SetUiScale(scale) => {
                self.settings.ui_scale = scale.clamp(0.6, 2.5);
                let _ = self.settings.save();
            }
            Message::FocusEffects(host) => {
                self.fx_focus = host;
                self.settings.show_effects_dock = true;
                if let FxHost::MixerTrack(track) = host {
                    self.selected_mixer_track = track.min(self.project.mixer.tracks.len() - 1);
                }
            }
            Message::ToggleEffectsDock => {
                self.settings.show_effects_dock = !self.settings.show_effects_dock;
                let _ = self.settings.save();
            }
            Message::SetEffectsDockHeight(height) => {
                self.settings.effects_dock_height = height.clamp(120.0, 620.0);
                let _ = self.settings.save();
            }
            Message::RouteChannelToNewInsert(id) => {
                // Find an insert nothing is using yet, the way LMMS hands out
                // the next free FX channel.
                let used: Vec<usize> = self
                    .project
                    .channels
                    .iter()
                    .map(|c| c.mixer_track)
                    .collect();
                let free = (1..self.project.mixer.tracks.len()).find(|t| !used.contains(t));
                match free {
                    Some(track) => {
                        let name = self
                            .project
                            .channel(id)
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| format!("Insert {track}"));
                        if let Some(c) = self.project.channel_mut(id) {
                            c.mixer_track = track;
                        }
                        if let Some(t) = self.project.mixer.tracks.get_mut(track) {
                            t.name = name;
                        }
                        self.fx_focus = FxHost::MixerTrack(track);
                        self.selected_mixer_track = track;
                        self.touch();
                        self.note(format!("Routed to insert {track}"));
                    }
                    None => self.note("Every mixer insert is in use"),
                }
            }
            Message::SetRackZoom(width) => {
                self.rack_zoom = width.clamp(4.0, 64.0);
            }
            Message::SetBufferFrames(frames) => {
                self.settings.buffer_frames = frames;
                let _ = self.settings.save();
                self.note("Buffer size applies the next time LimeDAW starts");
            }
            Message::SetStatus(text) => self.note(text),

            // ---- project ---------------------------------------------
            Message::NewProject => {
                self.checkpoint();
                self.load_project(Project::starter(), None);
                self.note("New project");
            }
            Message::NewEmptyProject => {
                self.checkpoint();
                self.load_project(Project::empty(), None);
                self.note("Empty project");
            }
            Message::OpenProjectDialog => return open_project_dialog(),
            Message::OpenProjectPath(path) => return load_project_task(path),
            Message::ProjectLoaded(path, result) => match result {
                Ok(project) => {
                    self.checkpoint();
                    self.load_project(*project, Some(path.clone()));
                    self.settings
                        .remember_project(path.to_string_lossy().as_ref());
                    let _ = self.settings.save();
                    self.note(format!("Opened {}", crate::paths::display(&path)));
                }
                Err(err) => self.note(format!("Could not open project: {err}")),
            },
            Message::SaveProject => {
                let path = self.project_path.clone().unwrap_or_else(|| {
                    crate::paths::projects_dir()
                        .join(format!("{}.lime", safe_name(&self.project.name)))
                });
                return self.save_to(path);
            }
            Message::SaveProjectAs => {
                let name = safe_name(&self.project.name);
                return save_project_dialog(name);
            }
            Message::ProjectSaved(path, result) => match result {
                Ok(()) => {
                    if let Some(path) = path {
                        self.project_path = Some(path.clone());
                        self.settings
                            .remember_project(path.to_string_lossy().as_ref());
                        let _ = self.settings.save();
                        self.note(format!("Saved to {}", crate::paths::display(&path)));
                    }
                    self.dirty = false;
                }
                Err(err) => self.note(format!("Could not save: {err}")),
            },
            Message::SetProjectName(name) => {
                self.project.name = name;
                self.dirty = true;
            }
            Message::ExportWav => return self.export_wav_task(),
            Message::ExportFinished(result) => match result {
                Ok(path) => self.note(format!("Rendered to {path}")),
                Err(err) => self.note(format!("Render failed: {err}")),
            },
            Message::Undo => {
                if self.apply_undo() {
                    self.note("Undo");
                } else {
                    self.note("Nothing to undo");
                }
            }
            Message::Redo => {
                if self.redo_pop() {
                    self.note("Redo");
                } else {
                    self.note("Nothing to redo");
                }
            }

            // ---- channels ---------------------------------------------
            Message::SelectChannel(id) => {
                self.selected_channel = Some(id);
                self.selected_effect = None;
                if let Some(c) = self.project.channel(id) {
                    self.selected_mixer_track = c.mixer_track;
                }
                // Show the channel's own chain, which is what LMMS opens when
                // you click a track's effects button.
                self.fx_focus = FxHost::Channel(id);
            }
            Message::AddChannel(kind) => {
                self.checkpoint();
                let id = self.add_channel_of(kind);
                self.selected_channel = Some(id);
                self.touch_schedule();
            }
            Message::RemoveChannel(id) => {
                self.checkpoint();
                self.project.remove_channel(id);
                self.schedule_dirty = true;
                if self.selected_channel == Some(id) {
                    self.selected_channel = self.project.channels.first().map(|c| c.id);
                }
                self.touch();
            }
            Message::RenameChannel(id, name) => {
                if let Some(c) = self.project.channel_mut(id) {
                    c.name = name;
                }
                self.dirty = true;
            }
            Message::SetChannelVolume(id, v) => {
                if let Some(c) = self.project.channel_mut(id) {
                    c.volume = v.clamp(0.0, 2.0);
                }
                self.touch();
            }
            Message::SetChannelPan(id, v) => {
                if let Some(c) = self.project.channel_mut(id) {
                    c.pan = v.clamp(-1.0, 1.0);
                }
                self.touch();
            }
            Message::ToggleChannelMute(id) => {
                if let Some(c) = self.project.channel_mut(id) {
                    c.muted = !c.muted;
                }
                self.touch_schedule();
            }
            Message::ToggleChannelSolo(id) => {
                if let Some(c) = self.project.channel_mut(id) {
                    c.solo = !c.solo;
                }
                self.touch_schedule();
            }
            Message::SetChannelMixerTrack(id, track) => {
                let max = self.project.mixer.tracks.len().saturating_sub(1);
                if let Some(c) = self.project.channel_mut(id) {
                    c.mixer_track = track.min(max);
                }
                self.touch();
            }
            Message::SetChannelTranspose(id, v) => {
                if let Some(c) = self.project.channel_mut(id) {
                    c.transpose = v.clamp(-48.0, 48.0);
                }
                self.touch();
            }
            Message::SetInstrumentParam(id, param, value) => {
                if let Some(c) = self.project.channel_mut(id) {
                    c.instrument.set_param(&param, value);
                }
                self.touch();
            }
            Message::SetInstrumentKind(id, kind) => {
                self.checkpoint();
                if let Some(c) = self.project.channel_mut(id) {
                    c.instrument = match kind {
                        InstrumentKindChoice::Synth => Instrument::Synth(SynthPatch::default()),
                        InstrumentKindChoice::Sampler => {
                            Instrument::Sampler(SamplerPatch::from_model(DrumModel::Kick))
                        }
                        InstrumentKindChoice::SoundFont => {
                            Instrument::SoundFont(SoundFontPatch::default())
                        }
                    };
                }
                self.touch();
            }
            Message::SetDrumModel(id, model) => {
                if let Some(c) = self.project.channel_mut(id)
                    && let Instrument::Sampler(p) = &mut c.instrument
                {
                    p.source = SampleSource::Model(model);
                }
                self.touch();
            }
            Message::SetSoundFontPreset(id, bank, program) => {
                if let Some(c) = self.project.channel_mut(id)
                    && let Instrument::SoundFont(p) = &mut c.instrument
                {
                    p.bank = bank;
                    p.preset = program;
                    p.percussion = bank == 128;
                }
                self.touch();
            }
            Message::LoadSampleDialog(id) => return load_sample_dialog(id),
            Message::SaveProjectTo(path) => return self.save_to(path),
            Message::SampleLoaded(id, path, result) => match result {
                Ok(sample) => {
                    self.bank.insert(path.clone(), sample);
                    if let Some(c) = self.project.channel_mut(id) {
                        if let Instrument::Sampler(p) = &mut c.instrument {
                            p.source = SampleSource::File { path: path.clone() };
                            p.root_key = 60;
                        } else {
                            c.instrument = Instrument::Sampler(SamplerPatch {
                                source: SampleSource::File { path: path.clone() },
                                ..SamplerPatch::default()
                            });
                        }
                    }
                    self.touch();
                    self.note("Sample loaded");
                }
                Err(err) => self.note(format!("Could not load sample: {err}")),
            },
            Message::AddSoundFontChannel {
                bank,
                program,
                name,
            } => {
                self.checkpoint();
                let id = self.add_channel_of(NewChannelKind::SoundFont);
                if let Some(channel) = self.project.channel_mut(id) {
                    channel.name = name;
                    channel.instrument = Instrument::SoundFont(SoundFontPatch {
                        bank,
                        preset: program,
                        percussion: bank == 128,
                    });
                }
                self.selected_channel = Some(id);
                self.touch();
            }
            Message::AddSampleChannel(path) => {
                self.checkpoint();
                let id = self.add_channel_of(NewChannelKind::EmptySampler);
                if let Some(channel) = self.project.channel_mut(id) {
                    channel.name = path
                        .file_stem()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Sample".into());
                }
                self.selected_channel = Some(id);
                self.touch();
                return load_sample_path(id, path);
            }
            Message::MoveChannel(id, delta) => {
                if let Some(index) = self.project.channel_index(id) {
                    let target = index as isize + delta;
                    if target >= 0 && (target as usize) < self.project.channels.len() {
                        self.checkpoint();
                        self.project.channels.swap(index, target as usize);
                        self.touch_schedule();
                    }
                }
            }
            Message::CloneChannel(id) => {
                if let Some(source) = self.project.channel(id).cloned() {
                    self.checkpoint();
                    let new_id = self.project.next_id();
                    let mut clone = source.clone();
                    clone.id = new_id;
                    clone.name = format!("{} copy", source.name);
                    let index = self.project.channel_index(id).unwrap_or(0);
                    self.project.channels.insert(index + 1, clone);
                    self.selected_channel = Some(new_id);
                    self.touch_schedule();
                }
            }

            // ---- patterns ---------------------------------------------
            Message::SelectPattern(id) => {
                self.selected_pattern = Some(id);
                if matches!(self.play_mode, PlayMode::Pattern(_)) {
                    self.play_mode = PlayMode::Pattern(id);
                    self.engine.send(EngineCmd::SetMode(self.play_mode));
                }
                self.schedule_dirty = true;
                self.needs_sync = true;
            }
            Message::AddPattern => {
                self.checkpoint();
                let n = self.project.patterns.len() + 1;
                let id = self.project.add_pattern(format!("Pattern {n}"));
                self.selected_pattern = Some(id);
                if matches!(self.play_mode, PlayMode::Pattern(_)) {
                    self.play_mode = PlayMode::Pattern(id);
                    self.engine.send(EngineCmd::SetMode(self.play_mode));
                }
                self.touch_schedule();
            }
            Message::RemovePattern(id) => {
                self.checkpoint();
                self.project.remove_pattern(id);
                if self.selected_pattern == Some(id) {
                    self.selected_pattern = self.project.patterns.first().map(|p| p.id);
                }
                self.touch_schedule();
            }
            Message::RenamePattern(id, name) => {
                if let Some(p) = self.project.pattern_mut(id) {
                    p.name = name;
                }
                self.dirty = true;
            }
            Message::ClonePattern(id) => {
                if let Some(source) = self.project.pattern(id).cloned() {
                    self.checkpoint();
                    let new_id = self.project.next_id();
                    let mut clone = source.clone();
                    clone.id = new_id;
                    clone.name = format!("{} copy", source.name);
                    for note in &mut clone.notes {
                        note.id = self.project.next_id();
                    }
                    self.project.patterns.push(clone);
                    self.selected_pattern = Some(new_id);
                    self.touch_schedule();
                }
            }
            Message::ClearPattern(id) => {
                self.checkpoint();
                if let Some(p) = self.project.pattern_mut(id) {
                    p.notes.clear();
                }
                self.touch_schedule();
            }
            Message::SetPatternLength(id, length) => {
                if let Some(p) = self.project.pattern_mut(id) {
                    p.length_override = length;
                }
                self.touch_schedule();
            }

            // ---- editing ----------------------------------------------
            Message::PianoRoll(action) => return self.piano_roll_action(action),
            Message::Steps(action) => return self.steps_action(action),
            Message::PlaylistEdit(action) => return self.playlist_action(action),
            Message::Automation(action) => return self.automation_action(action),
            Message::SetGridSnap(snap) => {
                self.settings.grid_snap = snap;
                let _ = self.settings.save();
            }
            Message::SetDefaultVelocity(v) => {
                self.settings.default_velocity = v.clamp(0.05, 1.0);
                let _ = self.settings.save();
            }
            Message::PreviewNote(channel, key) => {
                self.engine.resume();
                // Nothing sends a matching release for a click, so time it out.
                self.preview_briefly(channel, key);
            }
            Message::ReleaseNote(channel, key) => self.release_preview(channel, key),
            Message::KeyNoteOn(key) => {
                if let Some(channel) = self.selected_channel_id() {
                    self.engine.resume();
                    self.preview(channel, key);
                }
            }
            Message::KeyNoteOff(key) => {
                if let Some(channel) = self.selected_channel_id() {
                    self.release_preview(channel, key);
                }
            }
            Message::SetKeyboardOctave(octave) => {
                self.release_all_previews();
                self.keyboard_octave = octave.clamp(0, 9);
                self.note(format!("Keyboard octave {}", self.keyboard_octave));
            }
            Message::CopyNotes => {
                let copied = self.copy_selection();
                if copied == 0 {
                    self.note("Select notes in the piano roll first");
                } else {
                    self.note(format!("Copied {copied} notes"));
                }
            }
            Message::PasteNotes => {
                let pasted = self.paste_clipboard(None);
                if pasted == 0 {
                    self.note("Nothing to paste");
                } else {
                    self.note(format!("Pasted {pasted} notes"));
                }
            }
            Message::DuplicateNotes => {
                let copied = self.copy_selection();
                if copied == 0 {
                    self.note("Select notes in the piano roll first");
                } else {
                    // Duplicate lands one clipboard-length after the original.
                    let span = self
                        .clipboard
                        .iter()
                        .map(|n| n.end())
                        .fold(0.0f64, f64::max)
                        .max(0.25);
                    let origin = self
                        .project
                        .pattern(self.selected_pattern_id().unwrap_or(0))
                        .map(|p| {
                            p.notes
                                .iter()
                                .filter(|n| self.piano_roll.selection.contains(&n.id))
                                .map(|n| n.start)
                                .fold(f64::MAX, f64::min)
                        })
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.0);
                    let pasted = self.paste_clipboard(Some(origin + span));
                    self.note(format!("Duplicated {pasted} notes"));
                }
            }
            Message::DeleteSelection => {
                let removed = self.delete_selection();
                if removed > 0 {
                    self.note(format!("Deleted {removed} notes"));
                }
            }
            Message::QuantizeSelection => {
                let moved = self.quantize_selection();
                if moved > 0 {
                    self.note(format!("Quantised {moved} notes"));
                }
            }
            Message::ImportMidi => return import_midi_dialog(),
            Message::ExportMidi => return self.export_midi(),
            Message::MidiImported(result) => match result {
                Ok((notes, tempo)) => {
                    let count = notes.len();
                    self.checkpoint();
                    if let Some(tempo) = tempo {
                        self.project.tempo = tempo.clamp(20.0, 999.0);
                        self.tempo_text = format!("{:.2}", self.project.tempo);
                    }
                    if let Some(pattern_id) = self.selected_pattern_id() {
                        // Re-key the incoming notes so they cannot collide with
                        // ids the project already uses.
                        let mut renumbered = Vec::with_capacity(notes.len());
                        for mut note in notes {
                            note.id = self.project.next_id();
                            note.channel = self.selected_channel_id().unwrap_or(note.channel);
                            renumbered.push(note);
                        }
                        if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                            pattern.notes.extend(renumbered);
                        }
                    }
                    self.touch();
                    self.note(format!("Imported {count} notes"));
                }
                Err(err) => self.note(format!("Could not import MIDI: {err}")),
            },

            // ---- mixer -------------------------------------------------
            Message::SelectMixerTrack(track) => {
                self.selected_mixer_track = track.min(self.project.mixer.tracks.len() - 1);
                self.selected_effect = None;
                // The dock follows the selection, so clicking a strip shows
                // its effects without a second click.
                self.fx_focus = FxHost::MixerTrack(self.selected_mixer_track);
            }
            Message::SetMixerVolume(track, v) => {
                if let Some(t) = self.project.mixer.tracks.get_mut(track) {
                    t.volume = v.clamp(0.0, 2.0);
                }
                self.touch();
            }
            Message::SetMixerPan(track, v) => {
                if let Some(t) = self.project.mixer.tracks.get_mut(track) {
                    t.pan = v.clamp(-1.0, 1.0);
                }
                self.touch();
            }
            Message::ToggleMixerMute(track) => {
                if let Some(t) = self.project.mixer.tracks.get_mut(track) {
                    t.muted = !t.muted;
                }
                self.touch();
            }
            Message::ToggleMixerSolo(track) => {
                if let Some(t) = self.project.mixer.tracks.get_mut(track) {
                    t.solo = !t.solo;
                }
                self.touch();
            }
            Message::RenameMixerTrack(track, name) => {
                if let Some(t) = self.project.mixer.tracks.get_mut(track) {
                    t.name = name;
                }
                self.dirty = true;
            }
            Message::SetSend(from, to, amount) => {
                let count = self.project.mixer.tracks.len();
                if from < count
                    && to < count
                    && from != to
                    && let Some(t) = self.project.mixer.tracks.get_mut(from)
                {
                    t.sends.retain(|(dst, _)| *dst != to);
                    if amount > 0.001 {
                        t.sends.push((to, amount.clamp(0.0, 1.0)));
                    }
                }
                self.touch();
            }
            Message::AddEffect(host, slug) => {
                self.checkpoint();
                if let Some(effect) = Effect::from_slug(&slug)
                    && let Some(chain) = self.effect_chain_mut(host)
                {
                    chain.push(effect);
                    let slot = chain.len() - 1;
                    self.selected_effect = Some((host, slot));
                }
                self.touch();
            }
            Message::RemoveEffect(host, slot) => {
                self.checkpoint();
                if let Some(chain) = self.effect_chain_mut(host)
                    && slot < chain.len()
                {
                    chain.remove(slot);
                }
                self.selected_effect = None;
                self.touch();
            }
            Message::MoveEffect(host, slot, delta) => {
                if let Some(chain) = self.effect_chain_mut(host) {
                    let target = slot as isize + delta;
                    if target >= 0 && (target as usize) < chain.len() {
                        chain.swap(slot, target as usize);
                        self.selected_effect = Some((host, target as usize));
                    }
                }
                self.touch();
            }
            Message::ToggleEffect(host, slot) => {
                if let Some(chain) = self.effect_chain_mut(host)
                    && let Some(effect) = chain.get_mut(slot)
                {
                    effect.enabled = !effect.enabled;
                }
                self.touch();
            }
            Message::SetEffectParam(host, slot, param, value) => {
                if let Some(chain) = self.effect_chain_mut(host)
                    && let Some(effect) = chain.get_mut(slot)
                {
                    effect.kind.set_param(&param, value);
                }
                self.touch();
            }
            Message::SelectEffect(host, slot) => {
                self.selected_effect = Some((host, slot));
            }
            Message::ApplyEffectPreset(host, slot, name) => {
                self.checkpoint();
                let mut applied = false;
                if let Some(chain) = self.effect_chain_mut(host)
                    && let Some(effect) = chain.get_mut(slot)
                {
                    applied = effect.kind.apply_preset(&name);
                }
                if applied {
                    self.note(format!("Loaded preset “{name}”"));
                    self.touch();
                }
            }

            // ---- soundfont ---------------------------------------------
            Message::ChooseSoundFontDialog => return choose_soundfont_dialog(),
            Message::ChooseSoundFontPath(path) => {
                let result = self.apply_soundfont(path.clone());
                return Task::done(Message::SoundFontLoaded(path, result));
            }
            Message::SoundFontLoaded(path, result) => match result {
                Ok(()) => {
                    let count = self.soundfont_presets.len();
                    self.note(format!(
                        "Loaded {} ({count} presets)",
                        path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    ));
                    if matches!(self.setup, Setup::ChooseSoundFont { .. }) {
                        self.finish_setup();
                    }
                }
                Err(err) => {
                    if let Setup::ChooseSoundFont { error, .. } = &mut self.setup {
                        *error = Some(err.clone());
                    }
                    self.note(format!("Could not load soundfont: {err}"));
                }
            },
            Message::SkipSoundFont => self.finish_setup(),
            Message::SoundFontFilter(text) => self.sf_filter = text,

            // ---- MCP ----------------------------------------------------
            Message::McpJob(job) => {
                let (tool, args) = {
                    match job.lock() {
                        Ok(slot) => match slot.as_ref() {
                            Some(j) => (j.tool.clone(), j.arguments.clone()),
                            None => return Task::none(),
                        },
                        Err(_) => return Task::none(),
                    }
                };
                // Rendering is the one tool that takes real time. It goes to a
                // worker and answers from there, so a client asking for a
                // render does not freeze the interface for everyone else.
                #[cfg(not(target_arch = "wasm32"))]
                if tool == "export_wav" {
                    return self.export_wav_for_mcp(job, &args);
                }
                let result = self.handle_mcp_tool(&tool, &args);
                crate::app::respond(job, result);
            }
            Message::ToggleMcpServer => {
                self.settings.mcp_enabled = !self.settings.mcp_enabled;
                if self.settings.mcp_enabled {
                    self.start_mcp();
                    if self.mcp.is_some() {
                        self.note("MCP server started");
                    }
                } else {
                    self.stop_mcp();
                    self.note("MCP server stopped");
                }
                let _ = self.settings.save();
            }
            Message::CopyText(text) => {
                let label = "Copied to clipboard".to_string();
                return Task::batch([
                    iced::clipboard::write(text),
                    Task::done(Message::CopiedNotice(label)),
                ]);
            }
            Message::CopiedNotice(text) => self.note(text),
        }
        Task::none()
    }

    fn finish_setup(&mut self) {
        self.setup = Setup::Done;
        self.settings.setup_complete = true;
        let _ = self.settings.save();
    }

    pub(crate) fn load_project(&mut self, mut project: Project, path: Option<PathBuf>) {
        project.sanitise();
        self.selected_channel = project.channels.first().map(|c| c.id);
        self.selected_pattern = project.patterns.first().map(|p| p.id);
        self.selected_mixer_track = 0;
        self.selected_effect = None;
        self.tempo_text = format!("{:.2}", project.tempo);
        self.play_mode = match self.selected_pattern {
            Some(id) => PlayMode::Pattern(id),
            None => PlayMode::Song,
        };
        // Any samples the project references need loading before it will sound
        // right; missing ones fall back to silence rather than failing to open.
        let sample_paths: Vec<String> = project
            .channels
            .iter()
            .filter_map(|c| match &c.instrument {
                Instrument::Sampler(p) => match &p.source {
                    SampleSource::File { path } => Some(path.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        self.project = project;
        self.project_path = path;
        self.dirty = false;
        self.sf_signature.clear();
        self.engine.send(EngineCmd::Stop);
        self.engine.send(EngineCmd::SetMode(self.play_mode));
        self.forget_effect_chains();
        self.last_schedule = None;
        self.schedule_dirty = true;
        self.needs_sync = true;

        #[cfg(not(target_arch = "wasm32"))]
        for path in sample_paths {
            if self.bank.contains(&path) {
                continue;
            }
            if let Ok(sample) = crate::audio::samples::load_wav(std::path::Path::new(&path)) {
                self.bank.insert(path, Arc::new(sample));
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = sample_paths;
    }

    /// Undo, exposed for the MCP server as well as the menu.
    pub fn apply_undo(&mut self) -> bool {
        self.undo_pop().is_some()
    }

    /// Redo, exposed for the MCP server as well as the menu.
    pub fn apply_redo(&mut self) -> bool {
        self.redo_pop()
    }

    fn undo_pop(&mut self) -> Option<()> {
        let previous = self.undo_stack.pop()?;
        let current = std::mem::replace(&mut self.project, previous);
        self.redo_stack.push(current);
        self.after_history();
        Some(())
    }

    fn redo_pop(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.project, next);
        self.undo_stack.push(current);
        self.after_history();
        true
    }

    fn after_history(&mut self) {
        self.project.sanitise();
        if self
            .selected_channel
            .map(|id| self.project.channel(id).is_none())
            .unwrap_or(true)
        {
            self.selected_channel = self.project.channels.first().map(|c| c.id);
        }
        if self
            .selected_pattern
            .map(|id| self.project.pattern(id).is_none())
            .unwrap_or(true)
        {
            self.selected_pattern = self.project.patterns.first().map(|p| p.id);
        }
        self.selected_mixer_track = self
            .selected_mixer_track
            .min(self.project.mixer.tracks.len().saturating_sub(1));
        self.selected_effect = None;
        self.tempo_text = format!("{:.2}", self.project.tempo);
        self.touch();
    }

    pub(crate) fn add_channel_of(&mut self, kind: NewChannelKind) -> Id {
        let (name, instrument) = match kind {
            NewChannelKind::Synth(preset) => (
                preset.label().to_string(),
                Instrument::Synth(preset.patch()),
            ),
            NewChannelKind::Drum(model) => (
                model.label().to_string(),
                Instrument::Sampler(SamplerPatch::from_model(model)),
            ),
            NewChannelKind::SoundFont => {
                let patch = SoundFontPatch::default();
                let name = self
                    .soundfont_presets
                    .first()
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "SoundFont".to_string());
                (name, Instrument::SoundFont(patch))
            }
            NewChannelKind::EmptySampler => (
                "Sampler".to_string(),
                Instrument::Sampler(SamplerPatch::default()),
            ),
        };
        let id = self.project.add_channel(name, instrument);
        // Route each new channel to its own insert while there are spare ones.
        let used: Vec<usize> = self
            .project
            .channels
            .iter()
            .map(|c| c.mixer_track)
            .collect();
        let free = (1..self.project.mixer.tracks.len()).find(|t| !used.contains(t));
        if let (Some(track), Some(channel)) = (free, self.project.channel_mut(id)) {
            channel.mixer_track = track;
        }
        if let Some(track) = free {
            let channel_name = self
                .project
                .channel(id)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            if let Some(t) = self.project.mixer.tracks.get_mut(track) {
                t.name = channel_name;
            }
        }
        id
    }

    pub(crate) fn save_to(&mut self, path: PathBuf) -> Task<Message> {
        self.project.sanitise();
        let json = match serde_json::to_string_pretty(&self.project) {
            Ok(json) => json,
            Err(err) => return Task::done(Message::ProjectSaved(None, Err(err.to_string()))),
        };
        Task::perform(
            async move {
                let result = write_project(&path, &json).map_err(|e| e.to_string());
                (path, result)
            },
            |(path, result)| Message::ProjectSaved(Some(path), result),
        )
    }

    fn export_wav_task(&mut self) -> Task<Message> {
        #[cfg(target_arch = "wasm32")]
        {
            return Task::done(Message::ExportFinished(Err(
                "Rendering to a file is only available in the desktop app".into(),
            )));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.sync_engine();
            let name = safe_name(&self.project.name);
            let path = crate::paths::renders_dir().join(format!("{name}.wav"));
            let snapshot = self.build_render_snapshot(PlayMode::Song);
            let synths = self.build_render_synths();
            let sample_rate = 44_100.0;
            let length = snapshot.schedule.length;
            self.note("Rendering…");
            Task::perform(
                async move {
                    let samples = crate::audio::render_offline(
                        Arc::new(snapshot),
                        synths,
                        sample_rate,
                        length,
                        2.0,
                    );
                    crate::persist::write_wav(&path, &samples, sample_rate as u32)
                        .map(|_| crate::paths::display(&path))
                        .map_err(|e| e.to_string())
                },
                Message::ExportFinished,
            )
        }
    }

    /// Build a snapshot for offline rendering, independent of the live one.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn build_render_snapshot(&self, mode: PlayMode) -> crate::audio::Snapshot {
        let compiled = crate::audio::schedule::compile(&self.project, mode);
        let mut slots = 0usize;
        let mut channels = Vec::new();
        for c in &self.project.channels {
            let sf_slot = match &c.instrument {
                Instrument::SoundFont(_) if slots < crate::audio::core::MAX_SF_SLOTS => {
                    slots += 1;
                    Some(slots - 1)
                }
                _ => None,
            };
            let sample = match &c.instrument {
                Instrument::Sampler(p) => match &p.source {
                    SampleSource::Model(m) => self
                        .bank
                        .get(&crate::audio::samples::SampleBank::key_for_model(*m))
                        .cloned(),
                    SampleSource::File { path } => self.bank.get(path).cloned(),
                },
                _ => None,
            };
            channels.push(crate::audio::ChannelSnapshot {
                id: c.id,
                instrument: c.instrument.clone(),
                volume: c.volume,
                pan: c.pan,
                audible: self.project.channel_audible(c.id),
                mixer_track: c.mixer_track,
                transpose: c.transpose,
                effects: c.effects.clone(),
                sample,
                sf_slot,
            });
        }
        crate::audio::Snapshot {
            schedule: std::sync::Arc::new(compiled),
            channels,
            mixer: self.project.mixer.clone(),
            tempo: self.project.tempo,
            master_volume: self.project.master_volume,
            time_signature: self.project.time_signature,
            loop_enabled: false,
            loop_region: None,
            metronome: false,
            generation: 0,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn build_render_synths_public(&self) -> Vec<(usize, Box<rustysynth::Synthesizer>)> {
        self.build_render_synths()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn build_render_synths(&self) -> Vec<(usize, Box<rustysynth::Synthesizer>)> {
        let Some(sf) = &self.soundfont else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut slot = 0usize;
        for c in &self.project.channels {
            let Instrument::SoundFont(patch) = &c.instrument else {
                continue;
            };
            if slot >= crate::audio::core::MAX_SF_SLOTS {
                break;
            }
            let mut settings = rustysynth::SynthesizerSettings::new(44_100);
            settings.enable_reverb_and_chorus = false;
            if let Ok(mut synth) = rustysynth::Synthesizer::new(sf, &settings) {
                let channel = if patch.percussion { 9 } else { 0 };
                synth.process_midi_message(channel, 0xB0, 0x00, patch.bank as i32);
                synth.process_midi_message(channel, 0xC0, patch.preset as i32, 0);
                out.push((slot, Box::new(synth)));
            }
            slot += 1;
        }
        out
    }
}

/// Render on a worker thread and resolve when the file is on disk.
///
/// Rendering is CPU-bound; doing it inline would freeze the interface for as
/// long as the song is, which is exactly what an export should not do.
#[cfg(not(target_arch = "wasm32"))]
pub fn render_on_worker(
    snapshot: Arc<crate::audio::Snapshot>,
    synths: Vec<(usize, Box<rustysynth::Synthesizer>)>,
    sample_rate: f32,
    length_beats: f64,
    tail_seconds: f32,
    path: PathBuf,
    progress: Arc<crate::audio::RenderProgress>,
) -> impl std::future::Future<Output = Message> + Send + 'static {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("limedaw-render".into())
        .spawn(move || {
            let samples = crate::audio::render_offline_with_progress(
                snapshot,
                synths,
                sample_rate,
                length_beats,
                tail_seconds,
                Some(&progress),
            );
            let result = crate::persist::write_wav(&path, &samples, sample_rate as u32)
                .map(|_| crate::paths::display(&path))
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        })
        .expect("spawning the render thread");

    async move {
        match rx.await {
            Ok(result) => Message::ExportFinished(result),
            Err(_) => Message::ExportFinished(Err("the render was interrupted".into())),
        }
    }
}

/// Show a folder in the platform's file manager.
#[cfg(not(target_arch = "wasm32"))]
fn open_folder(path: PathBuf) -> Task<Message> {
    let _ = std::fs::create_dir_all(&path);
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let display = crate::paths::display(&path);
    match std::process::Command::new(opener).arg(&path).spawn() {
        Ok(_) => Task::done(Message::SetStatus(format!("Opened {display}"))),
        Err(err) => Task::done(Message::SetStatus(format!(
            "Could not open {display}: {err}"
        ))),
    }
}

#[cfg(target_arch = "wasm32")]
fn open_folder(_path: PathBuf) -> Task<Message> {
    Task::done(Message::SetStatus(
        "There is no file manager in the browser".into(),
    ))
}

/// Strip anything that has no business in a filename.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "Untitled".into()
    } else {
        trimmed.to_string()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_project(path: &std::path::Path, json: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)
}

#[cfg(target_arch = "wasm32")]
fn write_project(path: &std::path::Path, json: &str) -> std::io::Result<()> {
    // The browser build keeps projects in local storage instead of a filesystem.
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
    {
        let key = format!("limedaw.project.{}", path.display());
        let _ = storage.set_item(&key, json);
        return Ok(());
    }
    Err(std::io::Error::other("no local storage available"))
}

// ---------------------------------------------------------------------------
// File dialogs
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
fn open_project_dialog() -> Task<Message> {
    Task::perform(
        async {
            rfd::AsyncFileDialog::new()
                .add_filter("LimeDAW project", &["lime"])
                .set_directory(crate::paths::projects_dir())
                .set_title("Open project")
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        },
        |picked| match picked {
            Some(path) => Message::OpenProjectPath(path),
            None => Message::Noop,
        },
    )
}

#[cfg(target_arch = "wasm32")]
fn open_project_dialog() -> Task<Message> {
    Task::done(Message::SetStatus(
        "Opening files from disk is only available in the desktop app".into(),
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn load_project_task(path: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            let result = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|raw| {
                    serde_json::from_str::<Project>(&raw)
                        .map(Box::new)
                        .map_err(|e| e.to_string())
                });
            (path, result)
        },
        |(path, result)| Message::ProjectLoaded(path, result),
    )
}

#[cfg(target_arch = "wasm32")]
fn load_project_task(path: PathBuf) -> Task<Message> {
    let result = web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
        .and_then(|s| {
            s.get_item(&format!("limedaw.project.{}", path.display()))
                .ok()
                .flatten()
        })
        .ok_or_else(|| "project not found".to_string())
        .and_then(|raw| {
            serde_json::from_str::<Project>(&raw)
                .map(Box::new)
                .map_err(|e| e.to_string())
        });
    Task::done(Message::ProjectLoaded(path, result))
}

#[cfg(not(target_arch = "wasm32"))]
fn save_project_dialog(name: String) -> Task<Message> {
    Task::perform(
        async move {
            rfd::AsyncFileDialog::new()
                .add_filter("LimeDAW project", &["lime"])
                .set_directory(crate::paths::projects_dir())
                .set_file_name(format!("{name}.lime"))
                .set_title("Save project as")
                .save_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        },
        |picked| match picked {
            Some(path) => Message::SaveProjectTo(path),
            None => Message::Noop,
        },
    )
}

#[cfg(target_arch = "wasm32")]
fn save_project_dialog(_name: String) -> Task<Message> {
    Task::done(Message::SaveProject)
}

#[cfg(not(target_arch = "wasm32"))]
fn choose_soundfont_dialog() -> Task<Message> {
    Task::perform(
        async {
            rfd::AsyncFileDialog::new()
                .add_filter("SoundFont", &["sf2", "sf3"])
                .set_title("Choose a SoundFont")
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        },
        |picked| match picked {
            Some(path) => Message::ChooseSoundFontPath(path),
            None => Message::Noop,
        },
    )
}

#[cfg(target_arch = "wasm32")]
fn choose_soundfont_dialog() -> Task<Message> {
    Task::done(Message::SetStatus(
        "SoundFont loading is only available in the desktop app".into(),
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn load_sample_dialog(channel: Id) -> Task<Message> {
    Task::perform(
        async move {
            let handle = rfd::AsyncFileDialog::new()
                .add_filter("Audio", &["wav"])
                .set_directory(crate::paths::samples_dir())
                .set_title("Load sample")
                .pick_file()
                .await?;
            let path = handle.path().to_path_buf();
            let loaded = crate::audio::samples::load_wav(&path)
                .map(Arc::new)
                .map_err(|e| e.to_string());
            Some((channel, path.to_string_lossy().to_string(), loaded))
        },
        |picked| match picked {
            Some((channel, path, result)) => Message::SampleLoaded(channel, path, result),
            None => Message::Noop,
        },
    )
}

/// Load a sample that is already on disk, e.g. picked from the browser panel.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_sample_path(channel: Id, path: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            let loaded = crate::audio::samples::load_wav(&path)
                .map(Arc::new)
                .map_err(|e| e.to_string());
            (channel, path.to_string_lossy().to_string(), loaded)
        },
        |(channel, path, result)| Message::SampleLoaded(channel, path, result),
    )
}

#[cfg(target_arch = "wasm32")]
pub fn load_sample_path(_channel: Id, _path: PathBuf) -> Task<Message> {
    Task::done(Message::SetStatus(
        "Loading samples is only available in the desktop app".into(),
    ))
}

#[cfg(target_arch = "wasm32")]
fn load_sample_dialog(_channel: Id) -> Task<Message> {
    Task::done(Message::SetStatus(
        "Loading samples is only available in the desktop app".into(),
    ))
}

// ---------------------------------------------------------------------------
// Note clipboard and selection editing
// ---------------------------------------------------------------------------

impl LimeDaw {
    /// Copy the piano-roll selection, normalised so the earliest note sits at 0.
    pub(crate) fn copy_selection(&mut self) -> usize {
        let Some(pattern_id) = self.selected_pattern_id() else {
            return 0;
        };
        let Some(pattern) = self.project.pattern(pattern_id) else {
            return 0;
        };
        let selected: Vec<Note> = pattern
            .notes
            .iter()
            .filter(|n| self.piano_roll.selection.contains(&n.id))
            .copied()
            .collect();
        if selected.is_empty() {
            return 0;
        }
        let origin = selected.iter().map(|n| n.start).fold(f64::MAX, f64::min);
        self.clipboard = selected
            .into_iter()
            .map(|mut n| {
                n.start -= origin;
                n
            })
            .collect();
        self.clipboard.len()
    }

    /// Paste the clipboard at `at` beats, or at the playhead when `None`.
    pub(crate) fn paste_clipboard(&mut self, at: Option<f64>) -> usize {
        if self.clipboard.is_empty() {
            return 0;
        }
        let Some(pattern_id) = self.selected_pattern_id() else {
            return 0;
        };
        let channel = self.selected_channel_id();
        let target = at.unwrap_or_else(|| {
            let length = self.project.pattern_length_beats(pattern_id).max(0.25);
            let raw = self.engine.status().position().rem_euclid(length);
            let snap = self.settings.grid_snap;
            if snap > 0.0 {
                (raw / snap).round() * snap
            } else {
                raw
            }
        });

        self.checkpoint();
        let clipboard = self.clipboard.clone();
        let mut new_ids = Vec::with_capacity(clipboard.len());
        for mut note in clipboard {
            note.id = self.project.next_id();
            note.start = (note.start + target).max(0.0);
            // Notes pasted onto a different channel follow the selection.
            if let Some(channel) = channel
                && self.project.channel(note.channel).is_none()
            {
                note.channel = channel;
            }
            new_ids.push(note.id);
            if let Some(pattern) = self.project.pattern_mut(pattern_id) {
                pattern.notes.push(note);
            }
        }
        self.piano_roll.selection = new_ids.clone();
        self.touch();
        new_ids.len()
    }

    pub(crate) fn delete_selection(&mut self) -> usize {
        let Some(pattern_id) = self.selected_pattern_id() else {
            return 0;
        };
        if self.piano_roll.selection.is_empty() {
            return 0;
        }
        self.checkpoint();
        let selection = self.piano_roll.selection.clone();
        let removed = match self.project.pattern_mut(pattern_id) {
            Some(pattern) => {
                let before = pattern.notes.len();
                pattern.notes.retain(|n| !selection.contains(&n.id));
                before - pattern.notes.len()
            }
            None => 0,
        };
        self.piano_roll.selection.clear();
        self.touch();
        removed
    }

    /// Snap the selection to the grid, or the whole channel when nothing is
    /// selected — which is what a musician usually means by "quantise".
    pub(crate) fn quantize_selection(&mut self) -> usize {
        let Some(pattern_id) = self.selected_pattern_id() else {
            return 0;
        };
        let grid = self.settings.grid_snap;
        if grid <= 0.0 {
            self.note("Set a grid size first");
            return 0;
        }
        let selection = self.piano_roll.selection.clone();
        let channel = self.selected_channel_id();
        self.checkpoint();
        let moved = match self.project.pattern_mut(pattern_id) {
            Some(pattern) => {
                let mut count = 0;
                for note in &mut pattern.notes {
                    let in_scope = if selection.is_empty() {
                        Some(note.channel) == channel
                    } else {
                        selection.contains(&note.id)
                    };
                    if in_scope {
                        note.start = ((note.start / grid).round() * grid).max(0.0);
                        count += 1;
                    }
                }
                count
            }
            None => 0,
        };
        self.touch();
        moved
    }

    /// Write the selected pattern out as a Standard MIDI File.
    fn export_midi(&mut self) -> Task<Message> {
        #[cfg(target_arch = "wasm32")]
        {
            Task::done(Message::SetStatus(
                "MIDI export is only available in the desktop app".into(),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(pattern_id) = self.selected_pattern_id() else {
                return Task::done(Message::SetStatus("There is no pattern to export".into()));
            };
            let Some(pattern) = self.project.pattern(pattern_id) else {
                return Task::none();
            };
            let bytes = crate::persist::pattern_to_midi(&self.project, pattern);
            let name = safe_name(&pattern.name);
            let path = crate::paths::renders_dir().join(format!("{name}.mid"));
            match std::fs::create_dir_all(path.parent().unwrap_or(&path))
                .and_then(|_| std::fs::write(&path, &bytes))
            {
                Ok(()) => Task::done(Message::SetStatus(format!(
                    "Exported {}",
                    crate::paths::display(&path)
                ))),
                Err(err) => Task::done(Message::SetStatus(format!("Could not write MIDI: {err}"))),
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn import_midi_dialog() -> Task<Message> {
    Task::perform(
        async {
            let handle = rfd::AsyncFileDialog::new()
                .add_filter("MIDI", &["mid", "midi"])
                .set_directory(crate::paths::projects_dir())
                .set_title("Import MIDI")
                .pick_file()
                .await?;
            let bytes = handle.read().await;
            let mut next = 1u32;
            Some(crate::persist::midi_to_notes(&bytes, 0, &mut next).map_err(|e| e.to_string()))
        },
        |result| match result {
            Some(parsed) => Message::MidiImported(parsed),
            None => Message::Noop,
        },
    )
}

#[cfg(target_arch = "wasm32")]
fn import_midi_dialog() -> Task<Message> {
    Task::done(Message::SetStatus(
        "MIDI import is only available in the desktop app".into(),
    ))
}
