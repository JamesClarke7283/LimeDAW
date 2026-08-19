//! Application state, messages and update logic.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
// `web_time` re-exports the standard types on the desktop and browser-safe
// equivalents in wasm, so the same code compiles for both.
use iced::time::{Duration, Instant};

use iced::widget::container;
use iced::{Element, Subscription, Task, Theme};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use serde_json::Value;

use crate::audio::samples::SampleBank;
use crate::audio::schedule::{self, ChannelSnapshot, PlayMode, Snapshot};
use crate::audio::{Engine, EngineCmd};
use crate::config::{RendererChoice, Settings};
use crate::model::*;
use crate::theme::{self, ThemeChoice};
use crate::ui;

/// A tool call handed over by the MCP server. Wrapped so it can travel inside
/// a `Clone` message without the job itself being cloneable.
pub type SharedJob = Arc<Mutex<Option<crate::mcp::Job>>>;

/// Which editor fills the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MainView {
    Channels,
    PianoRoll,
    Playlist,
    Mixer,
    Settings,
    Mcp,
}

impl MainView {
    pub const ALL: [MainView; 6] = [
        MainView::Channels,
        MainView::PianoRoll,
        MainView::Playlist,
        MainView::Mixer,
        MainView::Settings,
        MainView::Mcp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MainView::Channels => "Channel rack",
            MainView::PianoRoll => "Piano roll",
            MainView::Playlist => "Song editor",
            MainView::Mixer => "Mixer",
            MainView::Settings => "Settings",
            MainView::Mcp => "MCP",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            MainView::Channels => "channels",
            MainView::PianoRoll => "piano_roll",
            MainView::Playlist => "playlist",
            MainView::Mixer => "mixer",
            MainView::Settings => "settings",
            MainView::Mcp => "mcp",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        // "playlist" is the historical name; "song" is what the interface and
        // LMMS both call it, so both are accepted.
        match slug {
            "song" | "song_editor" => Some(MainView::Playlist),
            "rack" | "beat" => Some(MainView::Channels),
            other => Self::ALL.into_iter().find(|v| v.slug() == other),
        }
    }
}

/// What the browser panel is listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserTab {
    Instruments,
    Presets,
    Samples,
    Projects,
}

impl BrowserTab {
    pub const ALL: [BrowserTab; 4] = [
        BrowserTab::Instruments,
        BrowserTab::Presets,
        BrowserTab::Samples,
        BrowserTab::Projects,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BrowserTab::Instruments => "Instruments",
            BrowserTab::Presets => "Presets",
            BrowserTab::Samples => "Samples",
            BrowserTab::Projects => "Projects",
        }
    }
}

/// A chain of effects, addressed either on a channel or on a mixer track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxHost {
    Channel(Id),
    MixerTrack(usize),
}

/// The first-run wizard's state.
#[derive(Debug, Clone, PartialEq)]
pub enum Setup {
    /// Not shown.
    Done,
    /// Waiting for the user to pick a soundfont.
    ChooseSoundFont {
        found: Vec<PathBuf>,
        error: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum Message {
    // ---- frame and system ----
    Tick,
    SystemTheme(iced::theme::Mode),
    Keyboard(iced::keyboard::Event),
    Noop,

    // ---- transport ----
    TogglePlay,
    Play,
    Pause,
    Stop,
    Seek(f64),
    SetTempo(f64),
    TempoInput(String),
    TempoCommit,
    ToggleMetronome,
    ToggleRecord,
    ToggleLoop,
    SetPlayMode(PlayMode),
    SetMasterVolume(f32),
    SetSwing(f64),
    SetTimeSignature(u32, u32),
    Panic,

    // ---- view ----
    SetView(MainView),
    ToggleMenu(crate::ui::menu::MenuId),
    CloseMenu,
    OpenProjectFolder,
    Quit,
    ToggleBrowser,
    ToggleInstrumentPanel,
    SetBrowserTab(BrowserTab),
    SetTheme(ThemeChoice),
    SetRenderer(RendererChoice),
    SetUiScale(f32),
    SetRackZoom(f32),
    FocusEffects(FxHost),
    ToggleEffectsDock,
    SetEffectsDockHeight(f32),
    RouteChannelToNewInsert(Id),
    SetBufferFrames(u32),
    SetStatus(String),

    // ---- project ----
    NewProject,
    NewEmptyProject,
    OpenProjectDialog,
    OpenProjectPath(PathBuf),
    SaveProject,
    SaveProjectAs,
    SaveProjectTo(PathBuf),
    ProjectSaved(Option<PathBuf>, Result<(), String>),
    ProjectLoaded(PathBuf, Result<Box<Project>, String>),
    SetProjectName(String),
    ExportWav,
    ExportFinished(Result<String, String>),
    Undo,
    Redo,

    // ---- channels ----
    SelectChannel(Id),
    AddChannel(NewChannelKind),
    RemoveChannel(Id),
    RenameChannel(Id, String),
    SetChannelVolume(Id, f32),
    SetChannelPan(Id, f32),
    ToggleChannelMute(Id),
    ToggleChannelSolo(Id),
    SetChannelMixerTrack(Id, usize),
    SetChannelTranspose(Id, f32),
    SetInstrumentParam(Id, String, f32),
    SetInstrumentKind(Id, InstrumentKindChoice),
    SetDrumModel(Id, DrumModel),
    SetSoundFontPreset(Id, u16, u16),
    LoadSampleDialog(Id),
    SampleLoaded(
        Id,
        String,
        Result<Arc<crate::audio::samples::Sample>, String>,
    ),
    AddSoundFontChannel {
        bank: u16,
        program: u16,
        name: String,
    },
    AddSampleChannel(PathBuf),
    MoveChannel(Id, isize),
    CloneChannel(Id),

    // ---- patterns ----
    SelectPattern(Id),
    AddPattern,
    RemovePattern(Id),
    RenamePattern(Id, String),
    ClonePattern(Id),
    ClearPattern(Id),
    SetPatternLength(Id, Option<f64>),

    // ---- editing ----
    PianoRoll(ui::piano_roll::Action),
    Steps(ui::channel_rack::Action),
    PlaylistEdit(ui::playlist::Action),
    Automation(ui::automation::Action),
    SetGridSnap(f64),
    SetDefaultVelocity(f32),
    PreviewNote(Id, u8),
    ReleaseNote(Id, u8),
    KeyNoteOn(u8),
    KeyNoteOff(u8),
    SetKeyboardOctave(i32),
    CopyNotes,
    PasteNotes,
    DuplicateNotes,
    DeleteSelection,
    QuantizeSelection,
    ImportMidi,
    ExportMidi,
    MidiImported(Result<(Vec<Note>, Option<f64>), String>),

    // ---- mixer ----
    SelectMixerTrack(usize),
    SetMixerVolume(usize, f32),
    SetMixerPan(usize, f32),
    ToggleMixerMute(usize),
    ToggleMixerSolo(usize),
    RenameMixerTrack(usize, String),
    SetSend(usize, usize, f32),
    AddEffect(FxHost, String),
    RemoveEffect(FxHost, usize),
    MoveEffect(FxHost, usize, isize),
    ToggleEffect(FxHost, usize),
    SetEffectParam(FxHost, usize, String, f32),
    SelectEffect(FxHost, usize),
    ApplyEffectPreset(FxHost, usize, String),

    // ---- soundfont ----
    ChooseSoundFontDialog,
    ChooseSoundFontPath(PathBuf),
    SoundFontLoaded(PathBuf, Result<(), String>),
    SkipSoundFont,
    SoundFontFilter(String),

    // ---- MCP ----
    McpJob(SharedJob),
    ToggleMcpServer,
    CopyText(String),
    CopiedNotice(String),
}

/// What kind of channel the "add" menu creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewChannelKind {
    Synth(SynthPreset),
    Drum(DrumModel),
    SoundFont,
    EmptySampler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynthPreset {
    Default,
    Bass,
    Lead,
    Pad,
    Pluck,
}

impl SynthPreset {
    pub const ALL: [SynthPreset; 5] = [
        SynthPreset::Default,
        SynthPreset::Bass,
        SynthPreset::Lead,
        SynthPreset::Pad,
        SynthPreset::Pluck,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SynthPreset::Default => "Basic",
            SynthPreset::Bass => "Bass",
            SynthPreset::Lead => "Lead",
            SynthPreset::Pad => "Pad",
            SynthPreset::Pluck => "Pluck",
        }
    }

    pub fn patch(self) -> SynthPatch {
        match self {
            SynthPreset::Default => SynthPatch::default(),
            SynthPreset::Bass => SynthPatch::bass(),
            SynthPreset::Lead => SynthPatch::lead(),
            SynthPreset::Pad => SynthPatch::pad(),
            SynthPreset::Pluck => SynthPatch::pluck(),
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name {
            "bass" => SynthPreset::Bass,
            "lead" => SynthPreset::Lead,
            "pad" => SynthPreset::Pad,
            "pluck" => SynthPreset::Pluck,
            _ => SynthPreset::Default,
        }
    }
}

/// The instrument kinds offered in the instrument panel's picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentKindChoice {
    Synth,
    Sampler,
    SoundFont,
}

impl InstrumentKindChoice {
    pub const ALL: [InstrumentKindChoice; 3] = [
        InstrumentKindChoice::Synth,
        InstrumentKindChoice::Sampler,
        InstrumentKindChoice::SoundFont,
    ];

    pub fn label(self) -> &'static str {
        match self {
            InstrumentKindChoice::Synth => "Synth",
            InstrumentKindChoice::Sampler => "Sampler",
            InstrumentKindChoice::SoundFont => "SoundFont",
        }
    }
}

impl std::fmt::Display for InstrumentKindChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A soundfont preset, listed for the instrument picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfPreset {
    pub bank: u16,
    pub program: u16,
    pub name: String,
}

/// The whole application.
pub struct LimeDaw {
    pub project: Project,
    pub settings: Settings,
    pub engine: Engine,
    pub bank: SampleBank,

    pub theme: Theme,
    pub system_dark: bool,

    pub view: MainView,
    pub browser_tab: BrowserTab,
    /// Which menu is showing its dropdown, if any.
    pub open_menu: Option<crate::ui::menu::MenuId>,
    /// Width of one step cell in the channel rack, in pixels.
    pub rack_zoom: f32,
    /// Which chain the effects dock along the bottom is showing.
    pub fx_focus: FxHost,
    pub setup: Setup,

    pub selected_channel: Option<Id>,
    pub selected_pattern: Option<Id>,
    pub selected_mixer_track: usize,
    pub selected_effect: Option<(FxHost, usize)>,
    pub play_mode: PlayMode,

    pub piano_roll: ui::piano_roll::State,
    pub playlist: ui::playlist::State,
    pub automation: ui::automation::State,

    pub project_path: Option<PathBuf>,
    pub dirty: bool,
    pub status: String,
    pub status_at: Instant,
    pub tempo_text: String,
    pub sf_filter: String,

    pub soundfont: Option<Arc<SoundFont>>,
    pub soundfont_path: Option<PathBuf>,
    pub soundfont_presets: Vec<SfPreset>,

    pub mcp: Option<crate::mcp::ServerHandle>,
    pub mcp_error: Option<String>,

    /// Arm the transport to write played notes into the selected pattern.
    pub recording: bool,
    /// The octave the computer keyboard plays in; C of this octave is the
    /// lowest key on the bottom row.
    pub keyboard_octave: i32,
    /// Notes copied out of the piano roll, relative to the earliest one.
    pub clipboard: Vec<Note>,
    /// The size the window was last told to be. Only the browser build uses
    /// this, to keep the canvas matched to the page.
    pub viewport: iced::Size,
    /// How far the current export has got, shared with its worker thread.
    pub render_progress: Arc<crate::audio::RenderProgress>,

    pub(crate) undo_stack: Vec<Project>,
    pub(crate) redo_stack: Vec<Project>,
    pub(crate) needs_sync: bool,
    /// Set when an edit changed what will be played, as opposed to how it
    /// sounds. Only then is the schedule recompiled.
    pub(crate) schedule_dirty: bool,
    /// The schedule last handed to the engine, reused when only parameters
    /// changed.
    pub(crate) last_schedule: Option<Arc<crate::audio::Schedule>>,
    /// When the engine was last given a snapshot, so edits are coalesced.
    pub(crate) last_sync: Instant,
    pub(crate) sf_signature: Vec<(Id, u16, u16, bool)>,
    /// The effect chains the engine was last given, by slug. Comparing against
    /// this is what lets a processor be built here rather than in the audio
    /// callback, where allocating a delay line would cause a dropout.
    pub(crate) sent_channel_fx: Vec<(Id, Vec<&'static str>)>,
    pub(crate) sent_mixer_fx: Vec<Vec<&'static str>>,
    pub(crate) generation: u64,
    pub(crate) last_tick: Instant,
    /// Notes currently held down by the mouse or keyboard, so they release.
    /// Notes currently sounding from the mouse or keyboard, so they release —
    /// and so a recorded note knows where it started.
    pub(crate) held_notes: Vec<HeldNote>,
}

/// A note being held down live.
#[derive(Debug, Clone, Copy)]
pub struct HeldNote {
    pub channel: Id,
    pub key: u8,
    /// The note being written, when the transport is recording.
    pub recorded: Option<Id>,
    /// Where the recorded note began, in beats within the pattern.
    pub start: f64,
    /// Auditions triggered by a click release themselves at this moment;
    /// notes held by a key or the mouse have no deadline.
    pub until: Option<Instant>,
}

impl LimeDaw {
    pub fn new() -> (Self, Task<Message>) {
        let mut settings = Settings::load();
        settings.sanitise();

        let _ = crate::paths::ensure_all();

        let engine = Engine::start(settings.buffer_frames);
        let sample_rate = engine.sample_rate();
        let mut bank = SampleBank::default();
        bank.populate_models(sample_rate);

        let project = Project::starter();
        let selected_channel = project.channels.first().map(|c| c.id);
        let selected_pattern = project.patterns.first().map(|p| p.id);
        let tempo_text = format!("{:.2}", project.tempo);

        let setup = if settings.setup_complete {
            Setup::Done
        } else {
            Setup::ChooseSoundFont {
                found: crate::paths::discover_soundfonts(),
                error: None,
            }
        };

        let mut app = Self {
            project,
            settings,
            engine,
            bank,
            theme: theme::dark(),
            system_dark: theme::system_prefers_dark(),
            view: MainView::Channels,
            browser_tab: BrowserTab::Instruments,
            open_menu: None,
            rack_zoom: 26.0,
            fx_focus: FxHost::MixerTrack(0),
            setup,
            selected_channel,
            selected_pattern,
            selected_mixer_track: 0,
            selected_effect: None,
            play_mode: PlayMode::Pattern(selected_pattern.unwrap_or(0)),
            piano_roll: ui::piano_roll::State::default(),
            playlist: ui::playlist::State::default(),
            automation: ui::automation::State::default(),
            project_path: None,
            dirty: false,
            status: String::new(),
            status_at: Instant::now(),
            tempo_text,
            sf_filter: String::new(),
            soundfont: None,
            soundfont_path: None,
            soundfont_presets: Vec::new(),
            mcp: None,
            mcp_error: None,
            recording: false,
            keyboard_octave: 4,
            clipboard: Vec::new(),
            viewport: iced::Size::new(0.0, 0.0),
            render_progress: Arc::new(crate::audio::RenderProgress::default()),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            needs_sync: true,
            schedule_dirty: true,
            last_schedule: None,
            last_sync: Instant::now(),
            sf_signature: Vec::new(),
            sent_channel_fx: Vec::new(),
            sent_mixer_fx: Vec::new(),
            generation: 0,
            last_tick: Instant::now(),
            held_notes: Vec::new(),
        };

        app.theme = theme::resolve(app.settings.theme, app.system_dark);
        app.start_mcp();

        let mut tasks = vec![iced::system::theme().map(Message::SystemTheme)];

        // Load the soundfont the user picked last time, if it is still there.
        if let Some(path) = app.settings.soundfont.clone() {
            let path = PathBuf::from(path);
            if path.exists() {
                tasks.push(Task::done(Message::ChooseSoundFontPath(path)));
            }
        }

        (app, Task::batch(tasks))
    }

    pub fn title(&self) -> String {
        let name = if self.project.name.is_empty() {
            "Untitled"
        } else {
            &self.project.name
        };
        let dirty = if self.dirty { "*" } else { "" };
        format!("{dirty}{name} — LimeDAW")
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    pub fn scale_factor(&self) -> f32 {
        self.settings.ui_scale
    }

    pub fn style(&self, theme: &Theme) -> iced::theme::Style {
        let p = theme::palette(theme);
        iced::theme::Style {
            background_color: p.base,
            text_color: p.text,
        }
    }

    // -----------------------------------------------------------------
    // Housekeeping
    // -----------------------------------------------------------------

    pub fn note(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.status_at = Instant::now();
    }

    /// Record the current project so the change about to happen can be undone.
    pub fn checkpoint(&mut self) {
        self.undo_stack.push(self.project.clone());
        if self.undo_stack.len() > 64 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// Note that something changed that affects how the project sounds.
    pub fn touch(&mut self) {
        self.dirty = true;
        self.needs_sync = true;
    }

    /// Note that something changed about *what* is played — notes, patterns,
    /// clips, mutes — so the schedule has to be compiled again.
    pub fn touch_schedule(&mut self) {
        self.dirty = true;
        self.needs_sync = true;
        self.schedule_dirty = true;
    }

    pub fn undo_stack_len(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_stack_len(&self) -> usize {
        self.redo_stack.len()
    }

    pub fn selected_pattern_id(&self) -> Option<Id> {
        self.selected_pattern
            .filter(|id| self.project.pattern(*id).is_some())
            .or_else(|| self.project.patterns.first().map(|p| p.id))
    }

    pub fn selected_channel_id(&self) -> Option<Id> {
        self.selected_channel
            .filter(|id| self.project.channel(*id).is_some())
            .or_else(|| self.project.channels.first().map(|c| c.id))
    }

    pub fn effect_chain(&self, host: FxHost) -> Option<&Vec<Effect>> {
        match host {
            FxHost::Channel(id) => self.project.channel(id).map(|c| &c.effects),
            FxHost::MixerTrack(t) => self.project.mixer.tracks.get(t).map(|t| &t.effects),
        }
    }

    pub fn effect_chain_mut(&mut self, host: FxHost) -> Option<&mut Vec<Effect>> {
        match host {
            FxHost::Channel(id) => self.project.channel_mut(id).map(|c| &mut c.effects),
            FxHost::MixerTrack(t) => self.project.mixer.tracks.get_mut(t).map(|t| &mut t.effects),
        }
    }

    // -----------------------------------------------------------------
    // Engine synchronisation
    // -----------------------------------------------------------------

    /// Rebuild the engine snapshot from the project and hand it over.
    pub fn sync_engine(&mut self) {
        self.generation += 1;
        self.last_sync = Instant::now();
        let mode = self.play_mode;

        // Recompiling walks every note in the project; a knob drag would do it
        // sixty times a second and starve the audio thread of a core.
        let compiled = match (self.schedule_dirty, &self.last_schedule) {
            (false, Some(existing)) => existing.clone(),
            _ => {
                let built = Arc::new(schedule::compile(&self.project, mode));
                self.last_schedule = Some(built.clone());
                self.schedule_dirty = false;
                built
            }
        };

        // Assign a soundfont slot to every channel that needs one.
        let mut slots: Vec<(Id, u16, u16, bool)> = Vec::new();
        let mut channels = Vec::with_capacity(self.project.channels.len());
        for c in &self.project.channels {
            let sf_slot = match &c.instrument {
                Instrument::SoundFont(p) if slots.len() < crate::audio::core::MAX_SF_SLOTS => {
                    slots.push((c.id, p.bank, p.preset, p.percussion));
                    Some(slots.len() - 1)
                }
                _ => None,
            };
            let sample = match &c.instrument {
                Instrument::Sampler(p) => match &p.source {
                    SampleSource::Model(m) => {
                        self.bank.get(&SampleBank::key_for_model(*m)).cloned()
                    }
                    SampleSource::File { path } => self.bank.get(path).cloned(),
                },
                _ => None,
            };
            channels.push(ChannelSnapshot {
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

        if slots != self.sf_signature {
            self.rebuild_soundfont_synths(&slots);
            self.sf_signature = slots;
        }

        self.sync_effect_chains();

        let snapshot = Snapshot {
            schedule: compiled,
            channels,
            mixer: self.project.mixer.clone(),
            tempo: self.project.tempo,
            master_volume: self.project.master_volume,
            time_signature: self.project.time_signature,
            loop_enabled: true,
            loop_region: self.project.playlist.loop_region,
            metronome: self.settings.metronome,
            generation: self.generation,
        };
        // Build the engine's mutable working copy here, on the UI thread, so
        // the audio callback never has to allocate to accept an edit.
        let working = Box::new(crate::audio::Working::from_snapshot(&snapshot));
        self.engine
            .send(EngineCmd::SetSnapshot(Arc::new(snapshot), working));
        self.needs_sync = false;
    }

    /// Send the engine any effect processor whose slot has changed shape.
    ///
    /// Constructing a reverb or a delay allocates megabytes of buffer, so it
    /// happens here and the finished processor is handed over. Slots whose kind
    /// is unchanged are left alone, which is what keeps a reverb tail alive
    /// while another knob is being turned.
    fn sync_effect_chains(&mut self) {
        use crate::audio::core::FxTarget;
        use crate::audio::effects::EffectProc;

        let sample_rate = self.engine.sample_rate();

        let mut channel_fx: Vec<(Id, Vec<&'static str>)> =
            Vec::with_capacity(self.project.channels.len());
        for (index, channel) in self.project.channels.iter().enumerate() {
            let slugs: Vec<&'static str> = channel.effects.iter().map(|e| e.kind.slug()).collect();
            // A channel that moved takes its chain's identity with it, so the
            // comparison is against the same channel, not the same position.
            let previous = self
                .sent_channel_fx
                .get(index)
                .filter(|(id, _)| *id == channel.id)
                .map(|(_, slugs)| slugs.as_slice())
                .unwrap_or(&[]);
            for (slot, slug) in slugs.iter().enumerate() {
                if previous.get(slot) != Some(slug) {
                    let proc = EffectProc::new(&channel.effects[slot].kind, sample_rate);
                    self.engine.send(crate::audio::EngineCmd::InstallEffect {
                        target: FxTarget::Channel(index),
                        slot,
                        proc: Box::new(proc),
                    });
                }
            }
            if previous.len() > slugs.len() {
                self.engine.send(crate::audio::EngineCmd::TruncateChain {
                    target: FxTarget::Channel(index),
                    len: slugs.len(),
                });
            }
            channel_fx.push((channel.id, slugs));
        }

        let mut mixer_fx: Vec<Vec<&'static str>> =
            Vec::with_capacity(self.project.mixer.tracks.len());
        for (index, track) in self.project.mixer.tracks.iter().enumerate() {
            let slugs: Vec<&'static str> = track.effects.iter().map(|e| e.kind.slug()).collect();
            let previous = self
                .sent_mixer_fx
                .get(index)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            for (slot, slug) in slugs.iter().enumerate() {
                if previous.get(slot) != Some(slug) {
                    let proc = EffectProc::new(&track.effects[slot].kind, sample_rate);
                    self.engine.send(crate::audio::EngineCmd::InstallEffect {
                        target: FxTarget::Mixer(index),
                        slot,
                        proc: Box::new(proc),
                    });
                }
            }
            if previous.len() > slugs.len() {
                self.engine.send(crate::audio::EngineCmd::TruncateChain {
                    target: FxTarget::Mixer(index),
                    len: slugs.len(),
                });
            }
            mixer_fx.push(slugs);
        }

        self.sent_channel_fx = channel_fx;
        self.sent_mixer_fx = mixer_fx;
    }

    /// Forget what the engine holds, so the next sync rebuilds every chain.
    pub(crate) fn forget_effect_chains(&mut self) {
        self.sent_channel_fx.clear();
        self.sent_mixer_fx.clear();
    }

    /// Build one `Synthesizer` per soundfont channel, off the audio thread.
    fn rebuild_soundfont_synths(&mut self, slots: &[(Id, u16, u16, bool)]) {
        let sample_rate = self.engine.sample_rate() as i32;
        for slot in 0..crate::audio::core::MAX_SF_SLOTS {
            let synth = slots.get(slot).and_then(|(_, bank, program, percussion)| {
                let sf = self.soundfont.as_ref()?;
                let mut settings = SynthesizerSettings::new(sample_rate);
                settings.enable_reverb_and_chorus = false;
                settings.maximum_polyphony = 64;
                let mut synth = Synthesizer::new(sf, &settings).ok()?;
                let channel = if *percussion { 9 } else { 0 };
                // Bank select then program change, the way a MIDI device expects.
                synth.process_midi_message(channel, 0xB0, 0x00, *bank as i32);
                synth.process_midi_message(channel, 0xC0, *program as i32, 0);
                Some(Box::new(synth))
            });
            self.engine.send(EngineCmd::SetSynth { slot, synth });
        }
    }

    /// Read every preset out of the loaded soundfont, sorted for the picker.
    fn index_soundfont(&mut self) {
        self.soundfont_presets.clear();
        let Some(sf) = &self.soundfont else {
            return;
        };
        for preset in sf.get_presets() {
            self.soundfont_presets.push(SfPreset {
                bank: preset.get_bank_number().max(0) as u16,
                program: preset.get_patch_number().max(0) as u16,
                name: preset.get_name().trim().to_string(),
            });
        }
        self.soundfont_presets.sort_by_key(|a| (a.bank, a.program));
    }

    pub fn apply_soundfont(&mut self, path: PathBuf) -> Result<(), String> {
        let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let sf = SoundFont::new(&mut file).map_err(|e| e.to_string())?;
        self.soundfont = Some(Arc::new(sf));
        self.soundfont_path = Some(path.clone());
        self.settings.soundfont = Some(path.to_string_lossy().to_string());
        self.project.soundfont = self.settings.soundfont.clone();
        self.index_soundfont();
        // Force the synths to be rebuilt against the new soundfont.
        self.sf_signature.clear();
        self.needs_sync = true;
        let _ = self.settings.save();
        Ok(())
    }

    pub fn start_mcp(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !self.settings.mcp_enabled || self.mcp.is_some() {
                return;
            }
            match crate::mcp::ServerHandle::start(crate::mcp::server::socket_path()) {
                Ok(handle) => {
                    self.mcp_error = None;
                    self.mcp = Some(handle);
                }
                Err(err) => {
                    self.mcp_error = Some(err.to_string());
                }
            }
        }
    }

    pub fn stop_mcp(&mut self) {
        self.mcp = None;
    }

    // -----------------------------------------------------------------
    // Note preview
    // -----------------------------------------------------------------

    pub fn preview(&mut self, channel: Id, key: u8) {
        if self
            .held_notes
            .iter()
            .any(|h| h.channel == channel && h.key == key)
        {
            return; // key repeat, or a second source holding the same note
        }
        self.engine.send(EngineCmd::NoteOn {
            channel,
            key,
            velocity: self.settings.default_velocity,
        });
        let recorded = self.begin_recording(channel, key);
        self.held_notes.push(HeldNote {
            channel,
            key,
            recorded: recorded.0,
            start: recorded.1,
            until: None,
        });
    }

    /// Audition a note that nothing will release — a step-sequencer click, say.
    pub fn preview_briefly(&mut self, channel: Id, key: u8) {
        self.preview(channel, key);
        if let Some(held) = self
            .held_notes
            .iter_mut()
            .find(|h| h.channel == channel && h.key == key)
        {
            held.until = Some(Instant::now() + Duration::from_millis(400));
        }
    }

    /// Register a note the engine is already sounding so it still gets a
    /// release — used where the caller cannot send one, such as MCP.
    pub fn hold_preview(&mut self, channel: Id, key: u8) {
        if self
            .held_notes
            .iter()
            .any(|h| h.channel == channel && h.key == key)
        {
            return;
        }
        self.held_notes.push(HeldNote {
            channel,
            key,
            recorded: None,
            start: 0.0,
            until: Some(Instant::now() + Duration::from_millis(1_200)),
        });
    }

    /// Release any audition whose time is up. Called once per frame.
    fn expire_previews(&mut self) {
        let now = Instant::now();
        let expired: Vec<(Id, u8)> = self
            .held_notes
            .iter()
            .filter(|h| h.until.map(|t| t <= now).unwrap_or(false))
            .map(|h| (h.channel, h.key))
            .collect();
        for (channel, key) in expired {
            self.release_preview(channel, key);
        }
    }

    pub fn release_preview(&mut self, channel: Id, key: u8) {
        self.engine.send(EngineCmd::NoteOff { channel, key });
        if let Some(index) = self
            .held_notes
            .iter()
            .position(|h| h.channel == channel && h.key == key)
        {
            let held = self.held_notes.remove(index);
            self.finish_recording(held);
        }
    }

    pub fn release_all_previews(&mut self) {
        for held in std::mem::take(&mut self.held_notes) {
            self.engine.send(EngineCmd::NoteOff {
                channel: held.channel,
                key: held.key,
            });
            self.finish_recording(held);
        }
    }

    /// If the transport is recording, start a note in the selected pattern.
    fn begin_recording(&mut self, channel: Id, key: u8) -> (Option<Id>, f64) {
        if !self.recording || !self.engine.status().playing() {
            return (None, 0.0);
        }
        let Some(pattern_id) = self.selected_pattern_id() else {
            return (None, 0.0);
        };
        // Recording is relative to the pattern, so wrap the song position.
        let length = self.project.pattern_length_beats(pattern_id).max(0.25);
        let raw = self.engine.status().position().rem_euclid(length);
        let snap = self.settings.grid_snap;
        let start = if snap > 0.0 {
            (raw / snap).round() * snap
        } else {
            raw
        };
        let id = self.project.next_id();
        let velocity = self.settings.default_velocity;
        if let Some(pattern) = self.project.pattern_mut(pattern_id) {
            pattern.notes.push(Note {
                id,
                channel,
                key,
                start: start.max(0.0),
                length: snap.max(1.0 / 16.0),
                velocity,
                pan: 0.0,
                fine_tune: 0.0,
                release: 0.0,
            });
        }
        self.touch();
        (Some(id), start)
    }

    /// Close a recorded note at the current playhead.
    fn finish_recording(&mut self, held: HeldNote) {
        let Some(note_id) = held.recorded else {
            return;
        };
        let Some(pattern_id) = self.selected_pattern_id() else {
            return;
        };
        let length = self.project.pattern_length_beats(pattern_id).max(0.25);
        let now = self.engine.status().position().rem_euclid(length);
        // A note held across the loop point should not end up negative.
        let played = if now >= held.start {
            now - held.start
        } else {
            length - held.start + now
        };
        let snap = self.settings.grid_snap.max(1.0 / 32.0);
        let quantised = ((played / snap).round() * snap).max(snap);
        if let Some(pattern) = self.project.pattern_mut(pattern_id)
            && let Some(note) = pattern.note_mut(note_id)
        {
            note.length = quantised;
        }
        self.touch();
    }

    /// The MIDI key a computer-keyboard key plays, if any.
    pub fn keyboard_key(&self, character: &str) -> Option<u8> {
        // Two rows, laid out like a piano: the bottom row is the lower octave.
        const LOWER: [(&str, i32); 17] = [
            ("z", 0),
            ("s", 1),
            ("x", 2),
            ("d", 3),
            ("c", 4),
            ("v", 5),
            ("g", 6),
            ("b", 7),
            ("h", 8),
            ("n", 9),
            ("j", 10),
            ("m", 11),
            (",", 12),
            ("l", 13),
            (".", 14),
            (";", 15),
            ("/", 16),
        ];
        const UPPER: [(&str, i32); 17] = [
            ("q", 12),
            ("2", 13),
            ("w", 14),
            ("3", 15),
            ("e", 16),
            ("r", 17),
            ("5", 18),
            ("t", 19),
            ("6", 20),
            ("y", 21),
            ("7", 22),
            ("u", 23),
            ("i", 24),
            ("9", 25),
            ("o", 26),
            ("0", 27),
            ("p", 28),
        ];
        let offset = LOWER
            .iter()
            .chain(UPPER.iter())
            .find(|(k, _)| *k == character)
            .map(|(_, o)| *o)?;
        let key = (self.keyboard_octave + 1) * 12 + offset;
        (0..=127).contains(&key).then_some(key as u8)
    }

    // -----------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Noop => {}
            Message::Tick => {
                let now = Instant::now();
                let dt = now
                    .saturating_duration_since(self.last_tick)
                    .as_secs_f32()
                    .min(0.25);
                self.last_tick = now;
                if self.engine.silent {
                    self.engine.tick_silent(dt);
                } else {
                    self.engine.collect();
                }
                // Coalesce edits: dragging a control changes the project on
                // every frame, but the engine only needs the result.
                if self.needs_sync
                    && now.saturating_duration_since(self.last_sync) >= Duration::from_millis(30)
                {
                    self.sync_engine();
                }
                self.expire_previews();
                #[cfg(target_arch = "wasm32")]
                {
                    // The browser has no window-resize event that reaches iced,
                    // so the viewport is checked each frame and the canvas
                    // resized when the page changes shape.
                    if let Some(size) = crate::browser_viewport() {
                        if (size.width - self.viewport.width).abs() > 1.0
                            || (size.height - self.viewport.height).abs() > 1.0
                        {
                            self.viewport = size;
                            return iced::window::latest().then(move |id| match id {
                                Some(id) => iced::window::resize(id, size),
                                None => Task::none(),
                            });
                        }
                    }
                }
                if !self.status.is_empty() && self.status_at.elapsed() > Duration::from_secs(6) {
                    self.status.clear();
                }
            }
            Message::SystemTheme(mode) => {
                self.system_dark = matches!(mode, iced::theme::Mode::Dark);
                self.theme = theme::resolve(self.settings.theme, self.system_dark);
            }
            Message::Keyboard(event) => {
                if let Some(message) = self.keyboard(event) {
                    return self.update_action(message);
                }
            }
            other => return self.update_action(other),
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        #[allow(unused_mut)]
        let mut subs = vec![
            // Sixty frames a second while something is moving; a quarter of
            // that when nothing is, which leaves the audio thread alone on a
            // machine that has other work to do.
            iced::time::every(Duration::from_millis(
                if self.engine.status().playing() || self.render_progress.is_running() {
                    16
                } else {
                    48
                },
            ))
            .map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemTheme),
            iced::keyboard::listen().map(Message::Keyboard),
        ];

        #[cfg(not(target_arch = "wasm32"))]
        if self.mcp.is_some() {
            subs.push(Subscription::run(mcp_jobs));
        }

        Subscription::batch(subs)
    }

    /// Turn a keyboard event into a message. Only events that no widget
    /// consumed reach here, so typing in a text field never fires a shortcut
    /// or plays a note.
    pub(crate) fn keyboard(&mut self, event: iced::keyboard::Event) -> Option<Message> {
        use iced::keyboard::{Event, Key, Modifiers, key::Named};
        match event {
            Event::KeyPressed { key, modifiers, .. } => {
                let ctrl = modifiers.contains(Modifiers::CTRL);
                let shift = modifiers.shift();
                Some(match (key.as_ref(), ctrl, shift) {
                    (Key::Named(Named::Space), _, _) => Message::TogglePlay,
                    (Key::Character("z"), true, true) => Message::Redo,
                    (Key::Character("z"), true, false) => Message::Undo,
                    (Key::Character("y"), true, _) => Message::Redo,
                    (Key::Character("s"), true, true) => Message::SaveProjectAs,
                    (Key::Character("s"), true, false) => Message::SaveProject,
                    (Key::Character("o"), true, _) => Message::OpenProjectDialog,
                    (Key::Character("n"), true, _) => Message::NewProject,
                    (Key::Character("e"), true, _) => Message::ExportWav,
                    (Key::Character("r"), true, _) => Message::ToggleRecord,
                    (Key::Character("c"), true, _) => Message::CopyNotes,
                    (Key::Character("v"), true, _) => Message::PasteNotes,
                    (Key::Character("d"), true, _) => Message::DuplicateNotes,
                    (Key::Character("q"), true, true) => Message::QuantizeSelection,
                    (Key::Character("q"), true, false) => Message::Quit,
                    (Key::Named(Named::Delete) | Key::Named(Named::Backspace), _, _) => {
                        Message::DeleteSelection
                    }
                    (Key::Named(Named::F5), _, _) => Message::SetView(MainView::Channels),
                    (Key::Named(Named::F6), _, _) => Message::SetView(MainView::PianoRoll),
                    (Key::Named(Named::F7), _, _) => Message::SetView(MainView::Playlist),
                    (Key::Named(Named::F9), _, _) => Message::SetView(MainView::Mixer),
                    (Key::Named(Named::Escape), _, _) => {
                        if self.open_menu.is_some() {
                            Message::CloseMenu
                        } else {
                            Message::Panic
                        }
                    }
                    (Key::Character("["), false, _) => {
                        Message::SetKeyboardOctave(self.keyboard_octave - 1)
                    }
                    (Key::Character("]"), false, _) => {
                        Message::SetKeyboardOctave(self.keyboard_octave + 1)
                    }
                    (Key::Character(character), false, false) => {
                        return self.keyboard_key(character).map(Message::KeyNoteOn);
                    }
                    _ => return None,
                })
            }
            Event::KeyReleased { key, .. } => match key.as_ref() {
                Key::Character(character) => self.keyboard_key(character).map(Message::KeyNoteOff),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = match &self.setup {
            Setup::ChooseSoundFont { found, error } => {
                ui::setup::view(self, found, error.as_deref())
            }
            Setup::Done => ui::shell::view(self),
        };
        container(content)
            .width(iced::Fill)
            .height(iced::Fill)
            .style(theme::root)
            .into()
    }
}

/// The stream of MCP jobs, wrapped so each one can ride in a `Clone` message.
#[cfg(not(target_arch = "wasm32"))]
fn mcp_jobs() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::StreamExt;
    crate::mcp::server::job_stream().map(|job| Message::McpJob(Arc::new(Mutex::new(Some(job)))))
}

/// Handle a tool call and reply to the MCP client.
pub fn respond(job: SharedJob, result: Result<Value, String>) {
    if let Ok(mut slot) = job.lock()
        && let Some(job) = slot.take()
    {
        job.respond(result);
    }
}
