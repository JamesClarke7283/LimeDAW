//! The tool surface the MCP server exposes.
//!
//! Every tool is dispatched by name into the application, which owns the
//! project, so the definitions here are purely descriptive.

use serde_json::{Value, json};

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

impl ToolDef {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.schema,
        })
    }
}

/// Shorthand for an object schema.
fn object(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

fn empty() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn num(desc: &str) -> Value {
    json!({ "type": "number", "description": desc })
}

fn int(desc: &str) -> Value {
    json!({ "type": "integer", "description": desc })
}

fn text(desc: &str) -> Value {
    json!({ "type": "string", "description": desc })
}

fn flag(desc: &str) -> Value {
    json!({ "type": "boolean", "description": desc })
}

/// Every tool, in the order they are listed to a client.
pub fn tool_definitions() -> Vec<ToolDef> {
    vec![
        // ---- session ---------------------------------------------------
        ToolDef {
            name: "get_state",
            description: "Summarise the session: tempo, transport position, selected pattern, channels, patterns, playlist clips and mixer levels. Call this first to find out what exists.",
            schema: empty(),
        },
        ToolDef {
            name: "get_project",
            description: "Return the entire project as JSON, in the same format LimeDAW saves to disk.",
            schema: empty(),
        },
        ToolDef {
            name: "set_project",
            description: "Replace the whole project with the supplied JSON. Use the shape returned by get_project.",
            schema: object(
                json!({ "project": { "type": "object", "description": "A full project document." } }),
                &["project"],
            ),
        },
        ToolDef {
            name: "new_project",
            description: "Start a new project. Pass empty:true for a blank one, otherwise it opens the starter kit.",
            schema: object(
                json!({
                    "name": text("Project name."),
                    "empty": flag("Start with no channels or patterns."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "list_projects",
            description: "List the .lime project files in ~/LimeProjects.",
            schema: empty(),
        },
        ToolDef {
            name: "open_project",
            description: "Open a project file. A bare name is resolved inside ~/LimeProjects.",
            schema: object(
                json!({ "path": text("File path or project name.") }),
                &["path"],
            ),
        },
        ToolDef {
            name: "save_project",
            description: "Save the project. Without a path it saves over the current file, or into ~/LimeProjects using the project name.",
            schema: object(json!({ "path": text("Optional destination path.") }), &[]),
        },
        ToolDef {
            name: "export_wav",
            description: "Render the song (or the current pattern) to a 32-bit float stereo WAV file.",
            schema: object(
                json!({
                    "path": text("Destination .wav path. Defaults to ~/LimeProjects/Renders/<name>.wav."),
                    "mode": { "type": "string", "enum": ["song", "pattern"], "description": "What to render. Defaults to song." },
                    "tail_seconds": num("Extra time rendered after the last note, for reverb tails. Default 2."),
                }),
                &[],
            ),
        },
        // ---- transport -------------------------------------------------
        ToolDef {
            name: "play",
            description: "Start playback. Optionally seek first.",
            schema: object(json!({ "position": num("Beat to start from.") }), &[]),
        },
        ToolDef {
            name: "stop",
            description: "Stop playback and return the playhead to the start.",
            schema: empty(),
        },
        ToolDef {
            name: "pause",
            description: "Pause playback, leaving the playhead where it is.",
            schema: empty(),
        },
        ToolDef {
            name: "set_position",
            description: "Move the playhead to a beat position.",
            schema: object(json!({ "beat": num("Position in beats.") }), &["beat"]),
        },
        ToolDef {
            name: "set_tempo",
            description: "Set the project tempo in beats per minute (20-999).",
            schema: object(json!({ "bpm": num("Beats per minute.") }), &["bpm"]),
        },
        ToolDef {
            name: "set_time_signature",
            description: "Set the time signature.",
            schema: object(
                json!({
                    "numerator": int("Beats per bar."),
                    "denominator": int("Note value that gets one beat: 1, 2, 4, 8, 16 or 32."),
                }),
                &["numerator", "denominator"],
            ),
        },
        ToolDef {
            name: "set_swing",
            description: "Set the global swing amount, 0 (straight) to 0.9 (heavy shuffle).",
            schema: object(json!({ "amount": num("Swing, 0 to 0.9.") }), &["amount"]),
        },
        ToolDef {
            name: "set_play_mode",
            description: "Choose whether the transport loops the selected pattern or plays the arrangement.",
            schema: object(
                json!({
                    "mode": { "type": "string", "enum": ["pattern", "song"], "description": "Playback mode." },
                }),
                &["mode"],
            ),
        },
        ToolDef {
            name: "set_loop",
            description: "Set or clear the song loop region.",
            schema: object(
                json!({
                    "start": num("Loop start in beats."),
                    "end": num("Loop end in beats."),
                    "enabled": flag("Whether looping is on."),
                    "clear": flag("Remove the loop region entirely."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "set_metronome",
            description: "Turn the metronome click on or off.",
            schema: object(json!({ "enabled": flag("Metronome on.") }), &["enabled"]),
        },
        ToolDef {
            name: "set_master_volume",
            description: "Set the master output level, 0 to 2 (1 is unity).",
            schema: object(json!({ "volume": num("Linear gain.") }), &["volume"]),
        },
        // ---- channels --------------------------------------------------
        ToolDef {
            name: "list_channels",
            description: "List every channel with its id, name, instrument and mixer routing.",
            schema: empty(),
        },
        ToolDef {
            name: "add_channel",
            description: "Add an instrument channel. Instruments: synth (built-in subtractive synth), sampler (procedural drum or a WAV file) or soundfont.",
            schema: object(
                json!({
                    "name": text("Channel name."),
                    "instrument": { "type": "string", "enum": ["synth", "sampler", "soundfont"], "description": "Instrument type." },
                    "preset": { "type": "string", "enum": ["bass", "lead", "pad", "pluck", "default"], "description": "Synth starting patch." },
                    "drum": { "type": "string", "enum": ["Kick", "Snare", "Closed hat", "Open hat", "Clap", "Tom", "Rim", "Cymbal", "Cowbell"], "description": "Procedural drum model for a sampler channel." },
                    "sample_path": text("WAV file to load into a sampler channel."),
                    "bank": int("SoundFont bank number."),
                    "program": int("SoundFont preset number."),
                    "mixer_track": int("Mixer insert to route to."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "remove_channel",
            description: "Delete a channel and every note that belongs to it.",
            schema: object(json!({ "channel": int("Channel id.") }), &["channel"]),
        },
        ToolDef {
            name: "set_channel",
            description: "Change a channel's name, level, pan, mute, solo, transpose or mixer routing.",
            schema: object(
                json!({
                    "channel": int("Channel id."),
                    "name": text("New name."),
                    "volume": num("Linear gain, 0 to 2."),
                    "pan": num("-1 hard left to 1 hard right."),
                    "muted": flag("Mute the channel."),
                    "solo": flag("Solo the channel."),
                    "transpose": num("Transpose in semitones."),
                    "mixer_track": int("Mixer insert index."),
                    "step_key": int("MIDI key that step-sequencer steps play."),
                    "color": text("Hex colour such as #8BD13A."),
                }),
                &["channel"],
            ),
        },
        ToolDef {
            name: "set_instrument",
            description: "Replace a channel's instrument.",
            schema: object(
                json!({
                    "channel": int("Channel id."),
                    "instrument": { "type": "string", "enum": ["synth", "sampler", "soundfont"], "description": "Instrument type." },
                    "preset": { "type": "string", "enum": ["bass", "lead", "pad", "pluck", "default"], "description": "Synth starting patch." },
                    "drum": text("Procedural drum model for a sampler."),
                    "sample_path": text("WAV file for a sampler."),
                    "bank": int("SoundFont bank."),
                    "program": int("SoundFont preset."),
                }),
                &["channel", "instrument"],
            ),
        },
        ToolDef {
            name: "get_instrument_params",
            description: "List a channel instrument's parameters with their current values and ranges.",
            schema: object(json!({ "channel": int("Channel id.") }), &["channel"]),
        },
        ToolDef {
            name: "set_instrument_param",
            description: "Set one instrument parameter by name. Use get_instrument_params to discover names.",
            schema: object(
                json!({
                    "channel": int("Channel id."),
                    "param": text("Parameter name, e.g. cutoff, attack, osc1_wave."),
                    "value": num("Value in the parameter's own units."),
                    "normalised": num("Alternative: a 0-1 position within the parameter's range."),
                }),
                &["channel", "param"],
            ),
        },
        ToolDef {
            name: "preview_note",
            description: "Play a note immediately, without recording it, to audition a sound.",
            schema: object(
                json!({
                    "channel": int("Channel id."),
                    "key": int("MIDI key, 0-127."),
                    "velocity": num("0-1, default 0.8."),
                }),
                &["channel", "key"],
            ),
        },
        // ---- patterns and notes ----------------------------------------
        ToolDef {
            name: "list_patterns",
            description: "List patterns with their ids, names, lengths and note counts.",
            schema: empty(),
        },
        ToolDef {
            name: "add_pattern",
            description: "Create an empty pattern and select it.",
            schema: object(
                json!({
                    "name": text("Pattern name."),
                    "length_beats": num("Fixed length in beats. Omit to size the pattern to its content."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "remove_pattern",
            description: "Delete a pattern and any playlist clips that use it.",
            schema: object(json!({ "pattern": int("Pattern id.") }), &["pattern"]),
        },
        ToolDef {
            name: "set_pattern",
            description: "Rename a pattern, recolour it, or fix its length.",
            schema: object(
                json!({
                    "pattern": int("Pattern id."),
                    "name": text("New name."),
                    "length_beats": num("Fixed length in beats."),
                    "auto_length": flag("Size the pattern to its content instead."),
                    "color": text("Hex colour."),
                }),
                &["pattern"],
            ),
        },
        ToolDef {
            name: "select_pattern",
            description: "Make a pattern the one the piano roll edits and pattern mode plays.",
            schema: object(json!({ "pattern": int("Pattern id.") }), &["pattern"]),
        },
        ToolDef {
            name: "clear_pattern",
            description: "Delete notes from a pattern, optionally only for one channel.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "channel": int("Only clear this channel."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "list_notes",
            description: "List the notes in a pattern.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "channel": int("Only list notes on this channel."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "add_note",
            description: "Add one note to a pattern.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "channel": int("Channel id."),
                    "key": int("MIDI key, 60 is middle C."),
                    "start": num("Start in beats from the pattern's beginning."),
                    "length": num("Length in beats. Default 0.25 (a 16th)."),
                    "velocity": num("0-1. Default 0.78."),
                    "pan": num("-1 to 1."),
                    "fine_tune": num("Cents, -100 to 100."),
                }),
                &["channel", "key", "start"],
            ),
        },
        ToolDef {
            name: "add_notes",
            description: "Add many notes at once — the efficient way to write a part.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "channel": int("Default channel for notes that do not name one."),
                    "notes": {
                        "type": "array",
                        "description": "Notes to add.",
                        "items": object(json!({
                            "channel": int("Channel id."),
                            "key": int("MIDI key."),
                            "start": num("Start in beats."),
                            "length": num("Length in beats."),
                            "velocity": num("0-1."),
                            "pan": num("-1 to 1."),
                            "fine_tune": num("Cents."),
                        }), &["key", "start"]),
                    },
                }),
                &["notes"],
            ),
        },
        ToolDef {
            name: "update_note",
            description: "Change an existing note.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "note": int("Note id."),
                    "key": int("New MIDI key."),
                    "start": num("New start in beats."),
                    "length": num("New length in beats."),
                    "velocity": num("New velocity, 0-1."),
                    "pan": num("New pan."),
                    "fine_tune": num("New fine tuning in cents."),
                    "channel": int("Move the note to another channel."),
                }),
                &["note"],
            ),
        },
        ToolDef {
            name: "remove_note",
            description: "Delete a note by id.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "note": int("Note id."),
                }),
                &["note"],
            ),
        },
        ToolDef {
            name: "set_steps",
            description: "Write a step-sequencer row: a string or array of on/off steps for one channel, one step per 16th note.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "channel": int("Channel id."),
                    "steps": {
                        "description": "Either a string like 'x...x...x...x...' where x, X, 1, #, * or o is a hit and anything else is a rest, or an array of booleans/numbers.",
                        "anyOf": [
                            { "type": "string" },
                            { "type": "array", "items": { "type": ["boolean", "number", "string"] } }
                        ]
                    },
                    "step_beats": num("Length of one step in beats. Default 0.25."),
                    "velocity": num("Velocity for the hits, 0-1."),
                    "replace": flag("Clear the channel's existing notes first. Default true."),
                }),
                &["channel", "steps"],
            ),
        },
        ToolDef {
            name: "transpose_notes",
            description: "Shift notes up or down in semitones.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "channel": int("Only transpose this channel."),
                    "semitones": int("How far to move the notes."),
                }),
                &["semitones"],
            ),
        },
        ToolDef {
            name: "quantize_notes",
            description: "Snap note starts to a grid.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "channel": int("Only quantise this channel."),
                    "grid": num("Grid size in beats, e.g. 0.25 for 16ths."),
                    "strength": num("0-1, how far notes move toward the grid. Default 1."),
                }),
                &["grid"],
            ),
        },
        ToolDef {
            name: "humanize_notes",
            description: "Add small, deterministic variations to timing and velocity so a part sounds played rather than programmed.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "channel": int("Only affect this channel."),
                    "timing": num("Maximum timing shift in beats. Default 0.01."),
                    "velocity": num("Maximum velocity variation, 0-1. Default 0.1."),
                }),
                &[],
            ),
        },
        // ---- playlist --------------------------------------------------
        ToolDef {
            name: "list_clips",
            description: "List playlist clips with their ids, tracks, patterns and positions.",
            schema: empty(),
        },
        ToolDef {
            name: "add_clip",
            description: "Place a pattern on the playlist.",
            schema: object(
                json!({
                    "pattern": int("Pattern id."),
                    "track": int("Playlist row, from 0."),
                    "start": num("Start in beats."),
                    "length": num("Length in beats. Defaults to the pattern's own length."),
                }),
                &["pattern", "track", "start"],
            ),
        },
        ToolDef {
            name: "remove_clip",
            description: "Delete a playlist clip.",
            schema: object(json!({ "clip": int("Clip id.") }), &["clip"]),
        },
        ToolDef {
            name: "move_clip",
            description: "Move or resize a playlist clip.",
            schema: object(
                json!({
                    "clip": int("Clip id."),
                    "track": int("New playlist row."),
                    "start": num("New start in beats."),
                    "length": num("New length in beats."),
                    "muted": flag("Mute the clip."),
                }),
                &["clip"],
            ),
        },
        ToolDef {
            name: "set_playlist_track",
            description: "Rename, mute or solo a playlist row.",
            schema: object(
                json!({
                    "track": int("Row index."),
                    "name": text("New name."),
                    "muted": flag("Mute the row."),
                    "solo": flag("Solo the row."),
                    "color": text("Hex colour."),
                }),
                &["track"],
            ),
        },
        // ---- mixer -----------------------------------------------------
        ToolDef {
            name: "get_mixer",
            description: "Return every mixer track with its level, pan, effects and sends.",
            schema: empty(),
        },
        ToolDef {
            name: "set_mixer_track",
            description: "Set a mixer track's name, level, pan, mute or solo. Track 0 is the master.",
            schema: object(
                json!({
                    "track": int("Mixer track index."),
                    "name": text("New name."),
                    "volume": num("Linear gain, 0 to 2."),
                    "pan": num("-1 to 1."),
                    "muted": flag("Mute."),
                    "solo": flag("Solo."),
                }),
                &["track"],
            ),
        },
        ToolDef {
            name: "add_effect",
            description: "Add an effect to a mixer track or a channel. Kinds: reverb, delay, chorus, distortion, filter, eq, compressor, limiter, bitcrush, phaser, flanger, gate, stereo.",
            schema: object(
                json!({
                    "kind": text("Effect kind."),
                    "track": int("Mixer track index."),
                    "channel": int("Channel id, to insert on the channel instead."),
                    "position": int("Where in the chain to insert. Appends by default."),
                }),
                &["kind"],
            ),
        },
        ToolDef {
            name: "remove_effect",
            description: "Remove an effect from a chain by slot index.",
            schema: object(
                json!({
                    "track": int("Mixer track index."),
                    "channel": int("Channel id."),
                    "slot": int("Slot index in the chain."),
                }),
                &["slot"],
            ),
        },
        ToolDef {
            name: "list_effects",
            description: "List a chain's effects with their parameters and ranges.",
            schema: object(
                json!({
                    "track": int("Mixer track index."),
                    "channel": int("Channel id."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "set_effect_param",
            description: "Set one parameter on one effect, or bypass it.",
            schema: object(
                json!({
                    "track": int("Mixer track index."),
                    "channel": int("Channel id."),
                    "slot": int("Slot index in the chain."),
                    "param": text("Parameter name, e.g. mix, feedback, cutoff."),
                    "value": num("Value in the parameter's own units."),
                    "normalised": num("Alternative: a 0-1 position within the range."),
                    "enabled": flag("Enable or bypass the effect."),
                }),
                &["slot"],
            ),
        },
        ToolDef {
            name: "move_effect",
            description: "Reorder an effect within its chain. Order matters: a compressor before a reverb sounds different from one after it.",
            schema: object(
                json!({
                    "track": int("Mixer track index."),
                    "channel": int("Channel id, to address the channel's chain instead."),
                    "slot": int("Current slot index."),
                    "to": int("Destination slot index."),
                }),
                &["slot", "to"],
            ),
        },
        ToolDef {
            name: "apply_effect_preset",
            description: "Load one of an effect's named presets. Use list_effect_kinds to see what each effect offers.",
            schema: object(
                json!({
                    "track": int("Mixer track index."),
                    "channel": int("Channel id."),
                    "slot": int("Slot index in the chain."),
                    "preset": text("Preset name, e.g. 'Hall' for a reverb."),
                }),
                &["slot", "preset"],
            ),
        },
        ToolDef {
            name: "set_effect_params",
            description: "Set several parameters on one effect at once — the efficient way to dial in a sound.",
            schema: object(
                json!({
                    "track": int("Mixer track index."),
                    "channel": int("Channel id."),
                    "slot": int("Slot index in the chain."),
                    "params": {
                        "type": "object",
                        "description": "Parameter names mapped to values in their own units.",
                        "additionalProperties": { "type": "number" },
                    },
                    "normalised": {
                        "type": "object",
                        "description": "Alternative: parameter names mapped to 0-1 positions within their range.",
                        "additionalProperties": { "type": "number" },
                    },
                    "enabled": flag("Enable or bypass the effect."),
                }),
                &["slot"],
            ),
        },
        ToolDef {
            name: "clear_effects",
            description: "Remove every effect from a chain.",
            schema: object(
                json!({
                    "track": int("Mixer track index."),
                    "channel": int("Channel id."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "copy_effects",
            description: "Copy a whole effect chain from one place to another.",
            schema: object(
                json!({
                    "from_track": int("Source mixer track."),
                    "from_channel": int("Source channel id."),
                    "to_track": int("Destination mixer track."),
                    "to_channel": int("Destination channel id."),
                    "append": flag("Add to the destination chain rather than replacing it."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "list_effect_kinds",
            description: "The catalogue: every effect that can be added, with its family, what it does, its parameters and ranges, and its presets.",
            schema: object(
                json!({
                    "kind": text("Only describe this one effect."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "set_send",
            description: "Route part of a mixer track's output to another track.",
            schema: object(
                json!({
                    "track": int("Source mixer track."),
                    "destination": int("Destination mixer track."),
                    "amount": num("0-1. Zero removes the send."),
                }),
                &["track", "destination", "amount"],
            ),
        },
        // ---- automation ------------------------------------------------
        ToolDef {
            name: "add_automation",
            description: "Add automation points to a pattern. Values are normalised 0-1 within the target's range.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "target": {
                        "type": "object",
                        "description": "What to automate. Give exactly one of these shapes.",
                        "properties": {
                            "kind": { "type": "string", "enum": ["tempo", "master_volume", "channel_volume", "channel_pan", "mixer_volume", "mixer_pan", "mixer_effect", "channel_effect", "instrument"] },
                            "channel": int("Channel id, for channel targets."),
                            "track": int("Mixer track, for mixer targets."),
                            "slot": int("Effect slot, for effect targets."),
                            "param": text("Parameter name, for effect and instrument targets."),
                        },
                        "required": ["kind"],
                    },
                    "points": {
                        "type": "array",
                        "description": "Points to add.",
                        "items": object(json!({
                            "beat": num("Position in beats."),
                            "value": num("Normalised value, 0-1."),
                            "curve": { "type": "string", "enum": ["linear", "hold", "smooth", "ease_in", "ease_out"] },
                        }), &["beat", "value"]),
                    },
                    "replace": flag("Clear the lane before adding. Default false."),
                }),
                &["target", "points"],
            ),
        },
        ToolDef {
            name: "list_automation",
            description: "List the automation lanes in a pattern.",
            schema: object(
                json!({ "pattern": int("Pattern id. Defaults to the selected pattern.") }),
                &[],
            ),
        },
        ToolDef {
            name: "clear_automation",
            description: "Remove automation lanes from a pattern.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "all": flag("Remove every lane."),
                    "index": int("Remove one lane by its position in the list."),
                }),
                &[],
            ),
        },
        // ---- soundfont and samples -------------------------------------
        ToolDef {
            name: "load_soundfont",
            description: "Load a SoundFont (.sf2) so soundfont channels can play it.",
            schema: object(json!({ "path": text("Path to a .sf2 file.") }), &["path"]),
        },
        ToolDef {
            name: "list_soundfont_presets",
            description: "List the presets in the loaded soundfont, with their bank and program numbers.",
            schema: object(
                json!({ "filter": text("Only return presets whose name contains this text.") }),
                &[],
            ),
        },
        ToolDef {
            name: "export_midi",
            description: "Write a pattern out as a Standard MIDI File.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "path": text("Destination .mid path. Defaults to ~/LimeProjects/Renders/<pattern>.mid."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "import_midi",
            description: "Read a Standard MIDI File into a pattern, on one channel.",
            schema: object(
                json!({
                    "path": text("Path to a .mid file."),
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "channel": int("Channel to place the notes on. Defaults to the selected channel."),
                    "replace": flag("Clear that channel's existing notes first. Default false."),
                    "use_tempo": flag("Adopt the tempo declared in the file. Default true."),
                }),
                &["path"],
            ),
        },
        ToolDef {
            name: "list_samples",
            description: "List the WAV files available under ~/LimeProjects/Samples plus the built-in drum models.",
            schema: empty(),
        },
        // ---- interface -------------------------------------------------
        ToolDef {
            name: "set_instrument_params",
            description: "Set several instrument parameters at once.",
            schema: object(
                json!({
                    "channel": int("Channel id."),
                    "params": {
                        "type": "object",
                        "description": "Parameter names mapped to values in their own units.",
                        "additionalProperties": { "type": "number" },
                    },
                    "normalised": {
                        "type": "object",
                        "description": "Alternative: parameter names mapped to 0-1 positions within their range.",
                        "additionalProperties": { "type": "number" },
                    },
                }),
                &["channel"],
            ),
        },
        ToolDef {
            name: "clone_channel",
            description: "Duplicate a channel, instrument, effects and all.",
            schema: object(
                json!({
                    "channel": int("Channel id."),
                    "name": text("Name for the copy."),
                    "with_notes": flag("Also copy the channel's notes in every pattern. Default false."),
                }),
                &["channel"],
            ),
        },
        ToolDef {
            name: "move_channel",
            description: "Move a channel up or down the rack.",
            schema: object(
                json!({
                    "channel": int("Channel id."),
                    "to": int("Destination position, from 0."),
                }),
                &["channel", "to"],
            ),
        },
        ToolDef {
            name: "clone_pattern",
            description: "Duplicate a pattern and everything in it.",
            schema: object(
                json!({
                    "pattern": int("Pattern id. Defaults to the selected pattern."),
                    "name": text("Name for the copy."),
                }),
                &[],
            ),
        },
        ToolDef {
            name: "copy_notes",
            description: "Copy notes from one channel to another, optionally transposed and offset in time.",
            schema: object(
                json!({
                    "pattern": int("Source pattern. Defaults to the selected pattern."),
                    "to_pattern": int("Destination pattern. Defaults to the source."),
                    "channel": int("Source channel id."),
                    "to_channel": int("Destination channel id. Defaults to the source channel."),
                    "offset": num("Shift the copies by this many beats."),
                    "semitones": int("Transpose the copies."),
                    "velocity_scale": num("Multiply the copies' velocities."),
                }),
                &["channel"],
            ),
        },
        ToolDef {
            name: "set_grid",
            description: "Set the snap resolution the interface and recording quantise to.",
            schema: object(
                json!({
                    "beats": num("Grid size in beats: 0.25 is a 16th, 0 turns snapping off."),
                }),
                &["beats"],
            ),
        },
        ToolDef {
            name: "set_recording",
            description: "Arm or disarm recording. While armed and playing, notes played into the app are written to the selected pattern.",
            schema: object(json!({ "enabled": flag("Recording armed.") }), &["enabled"]),
        },
        ToolDef {
            name: "get_meters",
            description: "Current peak levels for every mixer track, plus voice count and engine load.",
            schema: empty(),
        },
        ToolDef {
            name: "set_theme",
            description: "Switch the interface theme.",
            schema: object(
                json!({
                    "theme": { "type": "string", "enum": ["system", "light", "dark"], "description": "Theme to use." },
                }),
                &["theme"],
            ),
        },
        ToolDef {
            name: "set_view",
            description: "Switch the main view.",
            schema: object(
                json!({
                    "view": { "type": "string", "enum": ["channels", "piano_roll", "playlist", "mixer", "settings", "mcp"], "description": "View to show." },
                }),
                &["view"],
            ),
        },
        ToolDef {
            name: "undo",
            description: "Undo the last change.",
            schema: empty(),
        },
        ToolDef {
            name: "redo",
            description: "Redo the change that was just undone.",
            schema: empty(),
        },
    ]
}

/// Tool definitions as the JSON `tools/list` returns.
pub fn tools_list_json() -> Value {
    json!({
        "tools": tool_definitions().iter().map(ToolDef::to_json).collect::<Vec<_>>()
    })
}
