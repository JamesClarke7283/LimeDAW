//! End-to-end checks: the model compiles into a schedule, the schedule renders
//! into audio, and projects survive a round trip through disk formats.

use std::sync::Arc;

use limedaw::audio::samples::SampleBank;
use limedaw::audio::schedule::{self, ChannelSnapshot, PlayMode, Snapshot};
use limedaw::audio::{EngineCmd, EngineCore, Working};
use limedaw::model::*;

const SR: f32 = 44_100.0;

/// Hand a snapshot to the engine along with the working copy that goes with
/// it, the way the application does.
fn install(core: &mut EngineCore, snapshot: Arc<Snapshot>) {
    let working = Box::new(Working::from_snapshot(&snapshot));
    core.handle(EngineCmd::SetSnapshot(snapshot, working));
}

/// Build an engine snapshot from a project, the way the application does.
fn snapshot_of(project: &Project, bank: &SampleBank, mode: PlayMode) -> Arc<Snapshot> {
    let compiled = schedule::compile(project, mode);
    let channels = project
        .channels
        .iter()
        .map(|c| ChannelSnapshot {
            id: c.id,
            instrument: c.instrument.clone(),
            volume: c.volume,
            pan: c.pan,
            audible: project.channel_audible(c.id),
            mixer_track: c.mixer_track,
            transpose: c.transpose,
            effects: c.effects.clone(),
            sample: match &c.instrument {
                Instrument::Sampler(p) => match &p.source {
                    SampleSource::Model(m) => bank.get(&SampleBank::key_for_model(*m)).cloned(),
                    SampleSource::File { path } => bank.get(path).cloned(),
                },
                _ => None,
            },
            sf_slot: None,
        })
        .collect();

    Arc::new(Snapshot {
        schedule: compiled,
        channels,
        mixer: project.mixer.clone(),
        tempo: project.tempo,
        master_volume: project.master_volume,
        time_signature: project.time_signature,
        loop_enabled: false,
        loop_region: None,
        metronome: false,
        generation: 1,
    })
}

fn render(project: &Project, mode: PlayMode, seconds: f32) -> Vec<f32> {
    let mut bank = SampleBank::default();
    bank.populate_models(SR);
    let mut core = EngineCore::new(SR);
    install(&mut core, snapshot_of(project, &bank, mode));
    core.handle(EngineCmd::SetLoopEnabled(false));
    core.handle(EngineCmd::Seek(0.0));
    core.handle(EngineCmd::Play);

    let frames = (SR * seconds) as usize;
    let mut out = vec![0.0f32; frames * 2];
    core.render(&mut out);
    core.trash.clear();
    out
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |acc, s| acc.max(s.abs()))
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

#[test]
fn starter_project_is_playable() {
    let project = Project::starter();
    assert!(!project.channels.is_empty(), "starter has channels");
    assert!(!project.patterns.is_empty(), "starter has a pattern");
    assert!(!project.playlist.clips.is_empty(), "starter has a clip");
    let pattern = &project.patterns[0];
    assert!(pattern.notes.len() >= 8, "starter beat has notes");
}

#[test]
fn sanitise_clamps_and_drops_dangling_references() {
    let mut project = Project::starter();
    project.tempo = 100_000.0;
    project.master_volume = 99.0;
    project.swing = 5.0;
    let ghost = 9_999;
    let pattern = project.patterns[0].id;
    let id = project.next_id();
    project.pattern_mut(pattern).unwrap().notes.push(Note {
        id,
        channel: ghost,
        key: 200,
        start: -4.0,
        length: 0.0,
        velocity: 12.0,
        pan: -9.0,
        fine_tune: 900.0,
        release: 0.0,
    });

    project.sanitise();

    assert!(project.tempo <= 999.0);
    assert!(project.master_volume <= 2.0);
    assert!(project.swing <= 0.9);
    assert!(
        project.pattern(pattern).unwrap().note(id).is_none(),
        "notes on a channel that does not exist are dropped"
    );
    for note in &project.pattern(pattern).unwrap().notes {
        assert!(note.start >= 0.0 && note.length > 0.0);
        assert!((0.0..=1.0).contains(&note.velocity));
    }
}

// `Project::next_id` is private, so the struct-update form clippy suggests
// cannot be used here.
#[allow(clippy::field_reassign_with_default)]
#[test]
fn project_survives_a_json_round_trip() {
    let mut original = Project::starter();
    original.name = "Round trip".into();
    original.mixer.tracks[1]
        .effects
        .push(Effect::from_slug("reverb").unwrap());
    original.channels[0]
        .effects
        .push(Effect::from_slug("compressor").unwrap());

    let json = limedaw::persist::project_to_json(&original).unwrap();
    let restored = limedaw::persist::project_from_json(&json).unwrap();

    assert_eq!(restored.name, original.name);
    assert_eq!(restored.channels.len(), original.channels.len());
    assert_eq!(
        restored.patterns[0].notes.len(),
        original.patterns[0].notes.len()
    );
    assert_eq!(restored.mixer.tracks[1].effects.len(), 1);
    assert_eq!(restored.channels[0].effects.len(), 1);
}

#[test]
fn instrument_parameters_round_trip_by_name() {
    let mut instrument = Instrument::Synth(SynthPatch::default());
    assert!(instrument.set_param("cutoff", 2_500.0));
    assert!(instrument.set_param("osc2_enabled", 1.0));
    assert!(!instrument.set_param("not_a_param", 1.0));

    let params = instrument.params();
    let cutoff = params.iter().find(|p| p.name == "cutoff").unwrap();
    assert!((cutoff.value - 2_500.0).abs() < 0.01);

    // Normalised writes land inside the declared range.
    assert!(instrument.set_param_normalised("cutoff", 1.0));
    let max = instrument
        .params()
        .into_iter()
        .find(|p| p.name == "cutoff")
        .unwrap();
    assert!((max.value - max.max).abs() < 0.01);
}

#[test]
fn effect_parameters_are_clamped() {
    let mut effect = Effect::from_slug("delay").unwrap();
    assert!(effect.kind.set_param("feedback", 50.0));
    let feedback = effect
        .kind
        .params()
        .into_iter()
        .find(|(name, _, _, _)| *name == "feedback")
        .unwrap();
    assert!(
        feedback.1 <= 0.98,
        "feedback stays below unity: {}",
        feedback.1
    );
    assert!(!effect.kind.set_param("nonsense", 1.0));
}

// ---------------------------------------------------------------------------
// Scheduling
// ---------------------------------------------------------------------------

#[test]
fn pattern_mode_schedules_every_note() {
    let project = Project::starter();
    let pattern = project.patterns[0].id;
    let compiled = schedule::compile(&project, PlayMode::Pattern(pattern));
    let expected = project.patterns[0].notes.len();
    assert_eq!(compiled.notes.len(), expected);
    assert!(compiled.length > 0.0);
    // The list must be sorted; the engine walks it with a cursor.
    for pair in compiled.notes.windows(2) {
        assert!(pair[0].beat <= pair[1].beat);
    }
}

#[test]
fn a_long_clip_repeats_its_pattern() {
    let mut project = Project::empty();
    let channel = project.add_channel("Kick", Instrument::Sampler(SamplerPatch::kick()));
    let pattern = project.add_pattern("One bar");
    project.pattern_mut(pattern).unwrap().length_override = Some(4.0);
    project.add_step_note(pattern, channel, 0.0);
    // Four bars of clip over a one-bar pattern is four hits.
    project.add_clip(0, pattern, 0.0, 16.0);

    let compiled = schedule::compile(&project, PlayMode::Song);
    assert_eq!(
        compiled.notes.len(),
        4,
        "the pattern repeats to fill the clip"
    );
    let starts: Vec<f64> = compiled.notes.iter().map(|n| n.beat).collect();
    assert_eq!(starts, vec![0.0, 4.0, 8.0, 12.0]);
}

#[test]
fn muted_channels_are_not_scheduled() {
    let mut project = Project::starter();
    let first = project.channels[0].id;
    project.channel_mut(first).unwrap().muted = true;
    let pattern = project.patterns[0].id;
    let compiled = schedule::compile(&project, PlayMode::Pattern(pattern));
    assert!(compiled.notes.iter().all(|n| n.channel != first));
}

#[test]
fn soloing_a_channel_silences_the_others() {
    let mut project = Project::starter();
    let soloed = project.channels[1].id;
    project.channel_mut(soloed).unwrap().solo = true;
    let pattern = project.patterns[0].id;
    let compiled = schedule::compile(&project, PlayMode::Pattern(pattern));
    assert!(!compiled.notes.is_empty());
    assert!(compiled.notes.iter().all(|n| n.channel == soloed));
}

#[test]
fn swing_delays_off_beat_sixteenths() {
    let mut project = Project::empty();
    let channel = project.add_channel("Hat", Instrument::Sampler(SamplerPatch::hat()));
    let pattern = project.add_pattern("Hats");
    for step in 0..4 {
        project.add_step_note(pattern, channel, step as f64 * 0.25);
    }
    project.swing = 0.5;

    let compiled = schedule::compile(&project, PlayMode::Pattern(pattern));
    let starts: Vec<f64> = compiled.notes.iter().map(|n| n.beat).collect();
    assert!((starts[0] - 0.0).abs() < 1e-9, "on-beat 16ths stay put");
    assert!(
        starts[1] > 0.25,
        "off-beat 16ths are pushed later: {}",
        starts[1]
    );
    assert!((starts[2] - 0.5).abs() < 1e-9);
}

#[test]
fn automation_lanes_interpolate() {
    let mut lane = AutomationLane::new(AutomationTarget::MasterVolume);
    lane.points.clear();
    lane.points.push(AutomationPoint {
        beat: 0.0,
        value: 0.0,
        curve: Curve::Linear,
    });
    lane.points.push(AutomationPoint {
        beat: 4.0,
        value: 1.0,
        curve: Curve::Linear,
    });

    assert_eq!(lane.value_at(-1.0), Some(0.0), "before the first point");
    assert_eq!(lane.value_at(9.0), Some(1.0), "after the last point");
    let midpoint = lane.value_at(2.0).unwrap();
    assert!(
        (midpoint - 0.5).abs() < 1e-9,
        "halfway is halfway: {midpoint}"
    );

    // A hold curve should not move until the next point.
    lane.points[0].curve = Curve::Hold;
    assert_eq!(lane.value_at(3.9), Some(0.0));
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

#[test]
fn silence_in_silence_out() {
    let project = Project::empty();
    let out = render(&project, PlayMode::Song, 0.2);
    assert_eq!(peak(&out), 0.0, "an empty project renders silence");
}

#[test]
fn a_drum_pattern_makes_sound() {
    let project = Project::starter();
    let pattern = project.patterns[0].id;
    let out = render(&project, PlayMode::Pattern(pattern), 2.0);

    assert!(
        peak(&out) > 0.05,
        "the starter beat is audible: peak {}",
        peak(&out)
    );
    assert!(peak(&out) <= 1.05, "and it does not blow past full scale");
    assert!(
        out.iter().all(|s| s.is_finite()),
        "no NaNs reach the output"
    );
}

#[test]
fn the_synth_makes_sound() {
    let mut project = Project::empty();
    let channel = project.add_channel("Lead", Instrument::Synth(SynthPatch::lead()));
    let pattern = project.add_pattern("Note");
    let id = project.next_id();
    project.pattern_mut(pattern).unwrap().notes.push(Note {
        id,
        channel,
        key: 60,
        start: 0.0,
        length: 2.0,
        velocity: 1.0,
        pan: 0.0,
        fine_tune: 0.0,
        release: 0.0,
    });

    let out = render(&project, PlayMode::Pattern(pattern), 1.0);
    assert!(rms(&out) > 0.01, "a held note has body: rms {}", rms(&out));
    assert!(out.iter().all(|s| s.is_finite()));
}

#[test]
fn a_muted_mixer_track_is_silent() {
    let mut project = Project::starter();
    for track in project.mixer.tracks.iter_mut().skip(1) {
        track.muted = true;
    }
    let pattern = project.patterns[0].id;
    let out = render(&project, PlayMode::Pattern(pattern), 1.0);
    assert!(peak(&out) < 1e-6, "muting every insert silences the mix");
}

#[test]
fn master_volume_scales_the_output() {
    let project = Project::starter();
    let pattern = project.patterns[0].id;
    let loud = rms(&render(&project, PlayMode::Pattern(pattern), 1.5));

    let mut quiet_project = project.clone();
    quiet_project.master_volume = 0.1;
    let quiet = rms(&render(&quiet_project, PlayMode::Pattern(pattern), 1.5));

    assert!(
        quiet < loud * 0.5,
        "pulling the master down is audible: {quiet} vs {loud}"
    );
}

#[test]
fn effects_stay_stable_under_extreme_settings() {
    let mut project = Project::starter();
    // Push every effect to a setting that would ring or blow up if the DSP
    // were unstable, and confirm the output stays finite and bounded.
    for slug in EffectKind::SLUGS {
        let mut effect = Effect::from_slug(slug).unwrap();
        for (name, _, _, max) in effect.kind.params() {
            effect.kind.set_param(name, max);
        }
        project.mixer.tracks[1].effects.push(effect);
    }
    let pattern = project.patterns[0].id;
    let out = render(&project, PlayMode::Pattern(pattern), 2.0);
    assert!(
        out.iter().all(|s| s.is_finite()),
        "no NaN or infinity escapes the chain"
    );
    assert!(
        peak(&out) <= 1.2,
        "the safety limiter holds: peak {}",
        peak(&out)
    );
}

#[test]
fn the_transport_advances_while_playing() {
    let project = Project::starter();
    let mut bank = SampleBank::default();
    bank.populate_models(SR);
    let mut core = EngineCore::new(SR);
    let pattern = project.patterns[0].id;
    install(
        &mut core,
        snapshot_of(&project, &bank, PlayMode::Pattern(pattern)),
    );
    core.handle(EngineCmd::Play);

    let mut out = vec![0.0f32; 4096];
    core.render(&mut out);
    let after_one = core.transport.position;
    assert!(after_one > 0.0, "the playhead moved");

    core.render(&mut out);
    assert!(core.transport.position > after_one, "and keeps moving");

    core.handle(EngineCmd::Stop);
    assert_eq!(core.transport.position, 0.0, "stop rewinds");
    assert!(!core.transport.playing);
}

#[test]
fn looping_wraps_the_playhead() {
    let mut project = Project::empty();
    let channel = project.add_channel("Kick", Instrument::Sampler(SamplerPatch::kick()));
    let pattern = project.add_pattern("Bar");
    project.pattern_mut(pattern).unwrap().length_override = Some(1.0);
    project.add_step_note(pattern, channel, 0.0);
    project.tempo = 240.0; // one beat is a quarter of a second

    let mut bank = SampleBank::default();
    bank.populate_models(SR);
    let mut core = EngineCore::new(SR);
    install(
        &mut core,
        snapshot_of(&project, &bank, PlayMode::Pattern(pattern)),
    );
    core.handle(EngineCmd::SetLoopEnabled(true));
    core.handle(EngineCmd::Play);

    // Render well past the one-beat loop and confirm we are still inside it.
    let mut out = vec![0.0f32; (SR as usize) * 2];
    core.render(&mut out);
    assert!(
        core.transport.position < 1.0,
        "the playhead wrapped instead of running away: {}",
        core.transport.position
    );
    assert!(peak(&out) > 0.05, "the loop kept retriggering the kick");
}

#[test]
fn panic_stops_every_voice() {
    let project = Project::starter();
    let mut bank = SampleBank::default();
    bank.populate_models(SR);
    let mut core = EngineCore::new(SR);
    let pattern = project.patterns[0].id;
    install(
        &mut core,
        snapshot_of(&project, &bank, PlayMode::Pattern(pattern)),
    );
    core.handle(EngineCmd::Play);

    let mut out = vec![0.0f32; 8192];
    core.render(&mut out);
    assert!(core.active_voices() > 0, "notes are sounding");

    core.handle(EngineCmd::Panic);
    assert_eq!(core.active_voices(), 0, "panic clears the voice pool");
}

#[test]
fn changing_the_snapshot_mid_flight_does_not_panic() {
    let project = Project::starter();
    let mut bank = SampleBank::default();
    bank.populate_models(SR);
    let mut core = EngineCore::new(SR);
    let pattern = project.patterns[0].id;
    install(
        &mut core,
        snapshot_of(&project, &bank, PlayMode::Pattern(pattern)),
    );
    core.handle(EngineCmd::Play);

    let mut out = vec![0.0f32; 2048];
    core.render(&mut out);

    // Remove channels while voices from them are still ringing.
    let mut trimmed = project.clone();
    let doomed: Vec<Id> = trimmed.channels.iter().skip(1).map(|c| c.id).collect();
    for id in doomed {
        trimmed.remove_channel(id);
    }
    let mut next = snapshot_of(&trimmed, &bank, PlayMode::Pattern(pattern));
    Arc::get_mut(&mut next).unwrap().generation = 2;
    install(&mut core, next);

    core.render(&mut out);
    assert!(out.iter().all(|s| s.is_finite()));
    core.trash.clear();
}

// ---------------------------------------------------------------------------
// MIDI
// ---------------------------------------------------------------------------

#[test]
fn midi_round_trips_through_a_standard_file() {
    let mut project = Project::empty();
    let channel = project.add_channel("Keys", Instrument::Synth(SynthPatch::pad()));
    let pattern_id = project.add_pattern("Chords");
    for (i, key) in [60u8, 64, 67, 72].iter().enumerate() {
        let id = project.next_id();
        project.pattern_mut(pattern_id).unwrap().notes.push(Note {
            id,
            channel,
            key: *key,
            start: i as f64 * 0.5,
            length: 0.5,
            velocity: 0.8,
            pan: 0.0,
            fine_tune: 0.0,
            release: 0.0,
        });
    }
    project.tempo = 96.0;

    let pattern = project.pattern(pattern_id).unwrap();
    let bytes = limedaw::persist::pattern_to_midi(&project, pattern);
    assert_eq!(&bytes[0..4], b"MThd", "it is a MIDI file");

    let mut next = 1;
    let (notes, tempo) = limedaw::persist::midi_to_notes(&bytes, channel, &mut next).unwrap();
    assert_eq!(notes.len(), 4, "every note came back");
    assert!((tempo.unwrap() - 96.0).abs() < 0.5, "and so did the tempo");
    for (original, restored) in pattern.notes.iter().zip(notes.iter()) {
        assert_eq!(original.key, restored.key);
        assert!((original.start - restored.start).abs() < 0.01);
        assert!((original.length - restored.length).abs() < 0.01);
    }
}

#[test]
fn malformed_midi_is_rejected_not_fatal() {
    let mut next = 1;
    assert!(limedaw::persist::midi_to_notes(b"not a midi file", 0, &mut next).is_err());
    assert!(limedaw::persist::midi_to_notes(&[], 0, &mut next).is_err());
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[test]
fn settings_round_trip_through_toml() {
    let settings = limedaw::config::Settings {
        theme: limedaw::theme::ThemeChoice::Dark,
        renderer: limedaw::config::RendererChoice::Software,
        soundfont: Some("/tmp/example.sf2".into()),
        recent_projects: vec!["/tmp/a.lime".into(), "/tmp/b.lime".into()],
        ..limedaw::config::Settings::default()
    };

    let text = toml::to_string_pretty(&settings).expect("settings serialise to TOML");
    let restored: limedaw::config::Settings = toml::from_str(&text).expect("and parse back");

    assert_eq!(restored.theme, settings.theme);
    assert_eq!(restored.renderer, settings.renderer);
    assert_eq!(restored.soundfont, settings.soundfont);
    assert_eq!(restored.recent_projects, settings.recent_projects);
}

#[test]
fn a_settings_file_with_missing_keys_still_loads() {
    let restored: limedaw::config::Settings =
        toml::from_str("theme = \"Dark\"\n").expect("partial settings fall back to defaults");
    assert_eq!(restored.theme, limedaw::theme::ThemeChoice::Dark);
    assert_eq!(restored.ui_scale, 1.0);
}

// ---------------------------------------------------------------------------
// MCP
// ---------------------------------------------------------------------------

#[test]
fn every_declared_tool_has_a_schema() {
    let tools = limedaw::mcp::tool_definitions();
    assert!(
        tools.len() >= 60,
        "the tool surface is broad: {}",
        tools.len()
    );
    for tool in &tools {
        assert!(
            !tool.description.is_empty(),
            "{} has a description",
            tool.name
        );
        assert_eq!(
            tool.schema["type"], "object",
            "{} takes an object",
            tool.name
        );
        assert!(
            tool.schema.get("properties").is_some(),
            "{} declares properties",
            tool.name
        );
    }
    let mut names: Vec<&str> = tools.iter().map(|t| t.name).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "tool names are unique");
}

#[test]
fn the_client_configuration_is_valid_json() {
    let config = limedaw::mcp::client_config_json("/usr/bin/limedaw", "/tmp/limedaw.sock");
    let parsed: serde_json::Value = serde_json::from_str(&config).unwrap();
    assert_eq!(
        parsed["mcpServers"]["limedaw"]["command"],
        "/usr/bin/limedaw"
    );
    assert_eq!(parsed["mcpServers"]["limedaw"]["args"][0], "--mcp-stdio");

    let socat = limedaw::mcp::client_config_socat("/tmp/limedaw.sock");
    let parsed: serde_json::Value = serde_json::from_str(&socat).unwrap();
    assert_eq!(parsed["mcpServers"]["limedaw"]["command"], "socat");
}

#[test]
fn the_initialize_result_matches_the_protocol() {
    let result = limedaw::mcp::initialize_result();
    assert_eq!(result["protocolVersion"], limedaw::mcp::PROTOCOL_VERSION);
    assert_eq!(result["serverInfo"]["name"], "limedaw");
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["instructions"].as_str().unwrap().len() > 50);
}
