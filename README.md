# LimeDAW

A pattern-based digital audio workstation in Rust, built on [iced].

LimeDAW follows the workflow FL Studio and LMMS made popular: you build
**patterns** in a channel rack or piano roll, arrange those patterns on a
**playlist**, and mix the result through a **mixer** with insert effects. It
ships with its own synthesiser and drum kit, plays SoundFonts, renders to WAV,
and exposes the whole application over **MCP** so an assistant can drive it.

---

## Running it

```bash
cargo run --release
```

`--help` lists the other modes:

| Command | What it does |
| --- | --- |
| `limedaw` | Open the workstation |
| `limedaw --mcp-stdio` | Bridge stdin/stdout onto the running app's MCP socket |
| `limedaw --paths` | Print where LimeDAW keeps its files |

### In a browser

The interface also builds to WebAssembly, which is handy for trying the UI
without a desktop session:

```bash
trunk serve
```

Then open <http://127.0.0.1:8080>. Audio runs through WebAudio and starts on
your first click, the way browsers require. Projects are kept in local storage;
loading SoundFonts, importing samples and the MCP server are desktop-only.

---

## Where things live

Config and data follow the platform conventions (via `directories`, the Rust
equivalent of Python's platformdirs), while **projects live in a plain
`LimeProjects` folder in your home directory** so they are easy to find and
back up.

| | Path (Linux) |
| --- | --- |
| Projects | `~/LimeProjects` |
| Renders | `~/LimeProjects/Renders` |
| Samples | `~/LimeProjects/Samples` |
| Settings | `~/.local/share/LimeDAW/settings.toml` |
| MCP socket | `~/.local/share/LimeDAW/mcp.sock` |
| SoundFonts | `~/.local/share/LimeDAW/soundfonts` |

Settings are TOML, so you can edit them by hand; the app reads them again at
start-up.

---

## Making music

**Channel rack** — one row per instrument. Click the step grid to program a
beat; each step is a 16th note. Add channels from the browser on the left or
the `+` buttons above the grid.

**Piano roll** — draw notes with the left mouse button, drag to move them, drag
the right edge to resize, right-click to delete. The lane underneath edits
velocity. Notes on other channels show as ghosts so parts line up. Middle-drag
pans, the wheel scrolls, shift-wheel scrolls in time.

**Playlist** — click an empty lane to place the selected pattern, drag clips to
move them between tracks, drag the right edge to stretch (a stretched clip
repeats its pattern). Drag across the ruler to set a loop region.

**Mixer** — every channel routes to an insert. Select a strip to edit its
effect chain; each effect exposes its parameters as knobs.

**Automation** — the panel below the piano roll draws curves for tempo, any
channel or mixer level, any effect parameter, and any instrument parameter.

### Instruments

- **Synth** — three band-limited oscillators (sine, triangle, saw, square, two
  pulse widths, noise) with up to eight unison voices each, a state-variable
  filter with its own envelope, an LFO with five destinations, glide, mono mode
  and drive.
- **Sampler** — plays a WAV file or one of nine procedurally generated drum
  models, with tuning, start/end points, reverse, looping, an envelope and a
  filter. The built-in kit means a fresh install makes drum sounds immediately.
- **SoundFont** — any `.sf2`, one synthesiser instance per channel so each gets
  its own mixer routing and effects.

### Effects

Reverb, delay (tempo-synced, optional ping-pong), chorus, distortion, filter,
three-band EQ, compressor, limiter, bitcrush, phaser, flanger, gate and a
stereo width tool. Available on channels and on mixer inserts.

### Keyboard

| Key | Action |
| --- | --- |
| <kbd>Space</kbd> | Play / pause |
| <kbd>Ctrl</kbd>+<kbd>S</kbd> | Save |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd> | Save as |
| <kbd>Ctrl</kbd>+<kbd>O</kbd> | Open |
| <kbd>Ctrl</kbd>+<kbd>Z</kbd> / <kbd>Ctrl</kbd>+<kbd>Y</kbd> | Undo / redo |
| <kbd>Ctrl</kbd>+<kbd>E</kbd> | Render to WAV |
| <kbd>F5</kbd> … <kbd>F9</kbd> | Channel rack, piano roll, playlist, mixer |
| <kbd>Esc</kbd> | Stop all sound |

---

## Themes

LimeDAW has a light and a dark theme and follows your desktop's preference by
default. Change it under **Settings → Appearance**; picking Light or Dark
explicitly overrides the system. The web build reads
`prefers-color-scheme` instead.

## GPU acceleration

Rendering is GPU-accelerated through `wgpu` by default, with a CPU renderer
compiled in as well. **Settings → Rendering** switches between:

- **Automatic** — GPU where it works, software as a fallback.
- **GPU accelerated** — force `wgpu` (Vulkan, Metal, DX12, OpenGL).
- **Software** — force CPU rendering, which is the reliable choice over remote
  desktops or with broken drivers.

The choice is applied at start-up. `ICED_BACKEND=wgpu` or
`ICED_BACKEND=tiny-skia` overrides it for one run.

---

## The MCP server

While LimeDAW is open it serves [Model Context Protocol][mcp] over a unix
socket at `~/.local/share/LimeDAW/mcp.sock`. Everything the interface can do —
creating channels, writing notes, arranging clips, mixing, automating,
rendering — is reachable as a tool.

Open the **MCP** tab in the app for a ready-made configuration with a copy
button. It looks like this:

```json
{
  "mcpServers": {
    "limedaw": {
      "command": "/path/to/limedaw",
      "args": ["--mcp-stdio"],
      "env": { "LIMEDAW_MCP_SOCKET": "/home/you/.local/share/LimeDAW/mcp.sock" }
    }
  }
}
```

`--mcp-stdio` is a thin bridge: MCP clients spawn a process and talk over
stdin/stdout, so the bridge forwards that to the socket the running app is
listening on. If you would rather not point at the binary, `socat` works too
(the app shows that variant as well).

A session usually starts with `get_state`, which reports the tempo, transport,
channels, patterns and clips that exist. From there:

```
add_channel     { "name": "Bass", "instrument": "synth", "preset": "bass" }
set_steps       { "channel": 1, "steps": "x...x...x..x.x.." }
add_notes       { "channel": 5, "notes": [{ "key": 36, "start": 0, "length": 1 }] }
add_clip        { "pattern": 8, "track": 0, "start": 0 }
play
```

Positions are in beats (a quarter note is `1.0`) and keys are MIDI numbers
(middle C is `60`).

---

## Layout of the source

```
src/
  model/      plain, serialisable project data — no audio or UI dependencies
  audio/      the engine: DSP, voices, effects, sequencing, and platform hosts
  ui/         iced views; the canvas editors live one per file
  mcp/        the JSON-RPC server, its tool schemas and the stdio bridge
  app.rs      application state and messages
  actions.rs  the update logic
  editing.rs  applying editor gestures to the project
  mcp_dispatch.rs  running MCP tools against the app
```

The engine core is free of I/O: the desktop host drives it from a cpal
callback, the web host from a WebAudio node, and the offline renderer from a
plain loop. Project edits reach it as an immutable snapshot, and anything the
audio thread finishes with is handed back to be freed off-thread.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).

[iced]: https://iced.rs
[mcp]: https://modelcontextprotocol.io
