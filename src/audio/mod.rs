//! Audio: the engine core, its DSP, and the platform hosts that drive it.

pub mod core;
pub mod dsp;
pub mod effects;
pub mod samples;
pub mod schedule;
pub mod voice;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
pub mod web;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub use self::core::{EngineCmd, EngineCore, Meter, Transport, Trash, Working};
pub use self::schedule::{ChannelSnapshot, PlayMode, Schedule, Snapshot};

/// The number of mixer meters the status block can carry. Sized for the
/// default mixer plus headroom.
pub const MAX_METERS: usize = 64;

/// Lock-free status published by the audio thread and read by the UI.
pub struct EngineStatus {
    position: AtomicU64,
    playing: AtomicBool,
    voices: AtomicU32,
    sample_rate: AtomicU32,
    cpu_load: AtomicU32,
    tempo: AtomicU64,
    /// Interleaved peak L/R per mixer track, as f32 bits.
    meters: [AtomicU32; MAX_METERS * 2],
    /// Bumped whenever the audio callback runs, so the UI can spot a dead host.
    heartbeat: AtomicU64,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineStatus {
    pub fn new() -> Self {
        Self {
            position: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            voices: AtomicU32::new(0),
            sample_rate: AtomicU32::new(44_100),
            cpu_load: AtomicU32::new(0),
            tempo: AtomicU64::new(130.0f64.to_bits()),
            meters: std::array::from_fn(|_| AtomicU32::new(0)),
            heartbeat: AtomicU64::new(0),
        }
    }

    pub fn position(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Relaxed))
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn voices(&self) -> u32 {
        self.voices.load(Ordering::Relaxed)
    }

    pub fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }

    pub fn cpu_load(&self) -> f32 {
        f32::from_bits(self.cpu_load.load(Ordering::Relaxed))
    }

    pub fn tempo(&self) -> f64 {
        f64::from_bits(self.tempo.load(Ordering::Relaxed))
    }

    pub fn heartbeat(&self) -> u64 {
        self.heartbeat.load(Ordering::Relaxed)
    }

    pub fn meter(&self, track: usize) -> (f32, f32) {
        if track >= MAX_METERS {
            return (0.0, 0.0);
        }
        (
            f32::from_bits(self.meters[track * 2].load(Ordering::Relaxed)),
            f32::from_bits(self.meters[track * 2 + 1].load(Ordering::Relaxed)),
        )
    }

    fn publish(&self, core: &EngineCore, cpu: f32) {
        self.position
            .store(core.transport.position.to_bits(), Ordering::Relaxed);
        self.playing
            .store(core.transport.playing, Ordering::Relaxed);
        self.voices
            .store(core.active_voices() as u32, Ordering::Relaxed);
        self.sample_rate
            .store(core.sample_rate().to_bits(), Ordering::Relaxed);
        self.cpu_load.store(cpu.to_bits(), Ordering::Relaxed);
        self.tempo.store(core.tempo().to_bits(), Ordering::Relaxed);
        for (i, m) in core.meters.iter().take(MAX_METERS).enumerate() {
            // Meters fall back smoothly rather than flickering to zero.
            let prev_l = f32::from_bits(self.meters[i * 2].load(Ordering::Relaxed));
            let prev_r = f32::from_bits(self.meters[i * 2 + 1].load(Ordering::Relaxed));
            let l = m.peak_left.max(prev_l * 0.82);
            let r = m.peak_right.max(prev_r * 0.82);
            self.meters[i * 2].store(l.to_bits(), Ordering::Relaxed);
            self.meters[i * 2 + 1].store(r.to_bits(), Ordering::Relaxed);
        }
        self.heartbeat.fetch_add(1, Ordering::Relaxed);
    }
}

/// The shared plumbing between the control side and the audio callback.
pub struct EngineShared {
    pub commands: Mutex<VecDeque<EngineCmd>>,
    pub status: EngineStatus,
    pub trash: Mutex<Vec<Trash>>,
}

impl Default for EngineShared {
    fn default() -> Self {
        Self {
            commands: Mutex::new(VecDeque::with_capacity(256)),
            status: EngineStatus::new(),
            trash: Mutex::new(Vec::with_capacity(32)),
        }
    }
}

impl EngineShared {
    pub fn send(&self, cmd: EngineCmd) {
        if let Ok(mut q) = self.commands.lock() {
            // A backlog this deep means the audio thread is gone; drop the
            // oldest rather than grow without bound.
            if q.len() > 4096 {
                q.pop_front();
            }
            q.push_back(cmd);
        }
    }

    /// Called from the audio side. Never blocks.
    pub fn drain_into(&self, core: &mut EngineCore) {
        if let Ok(mut q) = self.commands.try_lock() {
            while let Some(cmd) = q.pop_front() {
                core.handle(cmd);
            }
        }
    }

    /// Called from the audio side to hand ownership of dead objects back.
    pub fn deposit_trash(&self, core: &mut EngineCore) {
        if core.trash.is_empty() {
            return;
        }
        if let Ok(mut t) = self.trash.try_lock() {
            t.append(&mut core.trash);
        }
    }

    /// Called from the control side; frees whatever the engine let go of.
    pub fn collect_trash(&self) {
        if let Ok(mut t) = self.trash.lock() {
            t.clear();
        }
    }
}

/// Run one audio callback's worth of work: commands in, samples out.
pub fn pump(shared: &EngineShared, core: &mut EngineCore, out: &mut [f32]) {
    shared.drain_into(core);
    core.render(out);
    shared.deposit_trash(core);
}

/// The control-side handle to the running engine.
pub struct Engine {
    shared: Arc<EngineShared>,
    #[cfg(not(target_arch = "wasm32"))]
    _host: Option<native::NativeHost>,
    #[cfg(target_arch = "wasm32")]
    host: Option<web::WebHost>,
    /// Set when no audio device could be opened; the transport still runs so
    /// the UI stays usable.
    pub silent: bool,
    /// A core used only when running without a device, driven by the UI clock.
    fallback: Option<Box<EngineCore>>,
    fallback_buffer: Vec<f32>,
}

impl Engine {
    /// Start the engine, opening the default output device if there is one.
    ///
    /// `buffer_frames` is what to ask the device for; the device may say no and
    /// the host reports what it actually agreed to.
    pub fn start(buffer_frames: u32) -> Self {
        let shared = Arc::new(EngineShared::default());

        #[cfg(not(target_arch = "wasm32"))]
        {
            match native::NativeHost::start(shared.clone(), buffer_frames) {
                Ok(host) => {
                    return Self {
                        shared,
                        _host: Some(host),
                        silent: false,
                        fallback: None,
                        fallback_buffer: Vec::new(),
                    };
                }
                Err(err) => {
                    log::warn!("no audio output: {err}; running silently");
                }
            }
            let sr = 48_000.0;
            Self {
                shared,
                _host: None,
                silent: true,
                fallback: Some(Box::new(EngineCore::new(sr))),
                fallback_buffer: vec![0.0; 2048],
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let _ = buffer_frames;
            match web::WebHost::start(shared.clone()) {
                Ok(host) => Self {
                    shared,
                    host: Some(host),
                    silent: false,
                    fallback: None,
                    fallback_buffer: Vec::new(),
                },
                Err(err) => {
                    log::warn!("no web audio: {err:?}; running silently");
                    Self {
                        shared,
                        host: None,
                        silent: true,
                        fallback: Some(Box::new(EngineCore::new(48_000.0))),
                        fallback_buffer: vec![0.0; 2048],
                    }
                }
            }
        }
    }

    pub fn shared(&self) -> &Arc<EngineShared> {
        &self.shared
    }

    pub fn status(&self) -> &EngineStatus {
        &self.shared.status
    }

    pub fn send(&self, cmd: EngineCmd) {
        self.shared.send(cmd);
    }

    pub fn sample_rate(&self) -> f32 {
        let sr = self.shared.status.sample_rate();
        if sr > 1.0 { sr } else { 48_000.0 }
    }

    /// The buffer size the device agreed to, in frames. Zero means the device
    /// chose for itself.
    pub fn buffer_frames(&self) -> u32 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self._host.as_ref().map(|h| h.buffer_frames).unwrap_or(0)
        }
        #[cfg(target_arch = "wasm32")]
        {
            0
        }
    }

    /// Browsers refuse to start audio until the user interacts with the page.
    /// The UI calls this on the first click.
    pub fn resume(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(host) = &self.host {
            host.resume();
        }
    }

    /// Advance a device-less engine by `seconds`, so the transport, meters and
    /// automation still behave when there is no sound card.
    pub fn tick_silent(&mut self, seconds: f32) {
        let Some(core) = self.fallback.as_mut() else {
            self.shared.collect_trash();
            return;
        };
        let frames = ((core.sample_rate() * seconds) as usize).clamp(0, 8192);
        if frames == 0 {
            return;
        }
        if self.fallback_buffer.len() < frames * 2 {
            self.fallback_buffer.resize(frames * 2, 0.0);
        }
        pump(&self.shared, core, &mut self.fallback_buffer[..frames * 2]);
        self.shared.status.publish(core, 0.0);
        self.shared.collect_trash();
    }

    /// Free anything the audio thread handed back. Called once per UI frame.
    pub fn collect(&self) {
        self.shared.collect_trash();
    }
}

/// How far along an offline render is, so the interface can say so.
#[derive(Default)]
pub struct RenderProgress {
    /// Frames written so far.
    done: AtomicU64,
    /// Frames expected in total. Zero until the render starts.
    total: AtomicU64,
}

impl RenderProgress {
    /// Completion from 0 to 1.
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        (self.done.load(Ordering::Relaxed) as f32 / total as f32).clamp(0.0, 1.0)
    }

    pub fn is_running(&self) -> bool {
        let total = self.total.load(Ordering::Relaxed);
        total > 0 && self.done.load(Ordering::Relaxed) < total
    }

    pub fn reset(&self) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
    }
}

/// Render a snapshot to interleaved stereo, offline and as fast as possible.
///
/// This is CPU-bound and must not be called on the interface thread; the
/// callers hand it to a worker and wait for the result.
pub fn render_offline(
    snapshot: Arc<Snapshot>,
    synths: Vec<(usize, Box<rustysynth::Synthesizer>)>,
    sample_rate: f32,
    length_beats: f64,
    tail_seconds: f32,
) -> Vec<f32> {
    render_offline_with_progress(
        snapshot,
        synths,
        sample_rate,
        length_beats,
        tail_seconds,
        None,
    )
}

/// The same, reporting how far it has got.
pub fn render_offline_with_progress(
    snapshot: Arc<Snapshot>,
    synths: Vec<(usize, Box<rustysynth::Synthesizer>)>,
    sample_rate: f32,
    length_beats: f64,
    tail_seconds: f32,
    progress: Option<&RenderProgress>,
) -> Vec<f32> {
    let mut core = EngineCore::new(sample_rate);
    let working = Box::new(Working::from_snapshot(&snapshot));
    core.handle(EngineCmd::SetSnapshot(snapshot.clone(), working));
    for (slot, synth) in synths {
        core.handle(EngineCmd::SetSynth {
            slot,
            synth: Some(synth),
        });
    }
    core.handle(EngineCmd::SetLoopEnabled(false));
    core.handle(EngineCmd::Seek(0.0));
    core.handle(EngineCmd::Play);

    let seconds = length_beats * 60.0 / snapshot.tempo.max(1.0) + tail_seconds as f64;
    let total_frames = (seconds * sample_rate as f64).max(0.0) as usize;
    if let Some(progress) = progress {
        progress.done.store(0, Ordering::Relaxed);
        progress.total.store(total_frames as u64, Ordering::Relaxed);
    }

    let mut out = vec![0.0f32; total_frames * 2];
    // A larger chunk than the live callback uses: there is no deadline here,
    // and fewer, bigger passes render measurably faster.
    let chunk = 16_384;
    let mut done = 0;
    while done < total_frames {
        let n = (total_frames - done).min(chunk);
        core.render(&mut out[done * 2..(done + n) * 2]);
        core.trash.clear();
        done += n;
        if let Some(progress) = progress {
            progress.done.store(done as u64, Ordering::Relaxed);
        }
    }
    if let Some(progress) = progress {
        progress.done.store(total_frames as u64, Ordering::Relaxed);
    }
    out
}
