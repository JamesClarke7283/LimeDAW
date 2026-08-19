//! The engine core: transport, sequencing, voices, mixing.
//!
//! This is deliberately free of any I/O. The native host drives it from a cpal
//! callback; the offline renderer drives it in a loop; the web host drives it
//! from a WebAudio worklet or, failing that, a timer.

use std::sync::Arc;

use rustysynth::Synthesizer;

use crate::audio::effects::EffectChain;
use crate::audio::schedule::{PlayMode, Snapshot};
use crate::audio::voice::VoicePool;
use crate::model::{AutomationTarget, Effect, Id, Instrument};

/// How many frames the engine processes between control updates. At 48 kHz
/// this puts event timing inside a millisecond, which is tighter than any
/// human can hear and keeps the per-sample loop tight.
pub const CONTROL_BLOCK: usize = 32;

/// The maximum number of simultaneously sounding notes.
pub const MAX_VOICES: usize = 192;

/// How many SoundFont synthesiser slots exist. One per channel that uses a
/// soundfont, so each gets its own mixer routing and effects.
pub const MAX_SF_SLOTS: usize = 16;

/// Which effect chain a command addresses. Indices refer to positions in the
/// current snapshot, which is why chains are resent whenever those move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxTarget {
    Channel(usize),
    Mixer(usize),
}

/// Things the control side asks the engine to do.
pub enum EngineCmd {
    Play,
    Pause,
    Stop,
    TogglePlay,
    Seek(f64),
    SetLoopEnabled(bool),
    SetMetronome(bool),
    /// Live tempo override, applied without recompiling the schedule.
    SetTempo(f64),
    NoteOn {
        channel: Id,
        key: u8,
        velocity: f32,
    },
    NoteOff {
        channel: Id,
        key: u8,
    },
    AllNotesOff,
    /// Hand over a prepared soundfont synthesiser, built off the audio thread.
    SetSynth {
        slot: usize,
        synth: Option<Box<Synthesizer>>,
    },
    /// Swap in a new compiled snapshot, together with the mutable working copy
    /// built for it. Both are prepared on the control thread so the audio
    /// thread only ever swaps pointers.
    SetSnapshot(Arc<Snapshot>, Box<Working>),
    SetMode(PlayMode),
    /// Put a processor built on the control thread into a chain slot.
    InstallEffect {
        target: FxTarget,
        slot: usize,
        proc: Box<crate::audio::effects::EffectProc>,
    },
    /// Shorten a chain to `len` slots.
    TruncateChain {
        target: FxTarget,
        len: usize,
    },
    Panic,
}

/// Objects the engine has finished with. The control side drains this so that
/// nothing is deallocated on the audio thread.
pub enum Trash {
    Snapshot(Arc<Snapshot>),
    Working(Box<Working>),
    Synth(Box<Synthesizer>),
    Effect(Box<crate::audio::effects::EffectProc>),
}

#[derive(Debug, Clone, Copy)]
struct PendingOff {
    end: f64,
    channel: Id,
    uid: u32,
    key: u8,
}

/// Transport state.
#[derive(Debug, Clone, Copy)]
pub struct Transport {
    pub playing: bool,
    /// Position in beats.
    pub position: f64,
    pub mode: PlayMode,
    pub loop_enabled: bool,
    pub metronome: bool,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            position: 0.0,
            mode: PlayMode::Song,
            loop_enabled: true,
            metronome: false,
        }
    }
}

/// A mutable working copy of the snapshot's automatable values.
///
/// Automation writes into this rather than into the shared snapshot, so it has
/// to be mutable and per-engine. It is built on the control thread and handed
/// over with each snapshot, which keeps the allocation off the audio thread.
pub struct Working {
    tempo: f64,
    master_volume: f32,
    channel_volume: Vec<f32>,
    channel_pan: Vec<f32>,
    channel_instrument: Vec<Instrument>,
    channel_effects: Vec<Vec<Effect>>,
    mixer_volume: Vec<f32>,
    mixer_pan: Vec<f32>,
    mixer_effects: Vec<Vec<Effect>>,
    generation: u64,
}

impl Working {
    /// Build the working copy for a snapshot. Call this on the control thread.
    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        let mut working = Self::default();
        working.rebuild(snapshot);
        working
    }

    fn rebuild(&mut self, snapshot: &Snapshot) {
        self.tempo = snapshot.tempo;
        self.master_volume = snapshot.master_volume;

        self.channel_volume.clear();
        self.channel_pan.clear();
        self.channel_instrument.clear();
        self.channel_effects.clear();
        for c in &snapshot.channels {
            self.channel_volume.push(c.volume);
            self.channel_pan.push(c.pan);
            self.channel_instrument.push(c.instrument.clone());
            self.channel_effects.push(c.effects.clone());
        }

        self.mixer_volume.clear();
        self.mixer_pan.clear();
        self.mixer_effects.clear();
        for t in &snapshot.mixer.tracks {
            self.mixer_volume.push(t.volume);
            self.mixer_pan.push(t.pan);
            self.mixer_effects.push(t.effects.clone());
        }
        self.generation = snapshot.generation;
    }
}

impl Default for Working {
    fn default() -> Self {
        Self {
            tempo: 130.0,
            master_volume: 0.85,
            channel_volume: Vec::new(),
            channel_pan: Vec::new(),
            channel_instrument: Vec::new(),
            channel_effects: Vec::new(),
            mixer_volume: Vec::new(),
            mixer_pan: Vec::new(),
            mixer_effects: Vec::new(),
            generation: u64::MAX,
        }
    }
}

/// Per-block metering, read by the UI.
#[derive(Debug, Clone, Copy, Default)]
pub struct Meter {
    pub peak_left: f32,
    pub peak_right: f32,
}

pub struct EngineCore {
    sample_rate: f32,
    snapshot: Arc<Snapshot>,
    working: Box<Working>,
    pub transport: Transport,
    pool: VoicePool,
    pending_offs: Vec<PendingOff>,
    /// Index of the next note in the schedule to consider.
    cursor: usize,
    channel_chains: Vec<EffectChain>,
    mixer_chains: Vec<EffectChain>,
    /// Per-mixer-track accumulation for one control block.
    bus_l: Vec<f32>,
    bus_r: Vec<f32>,
    /// Send returns, applied at the start of the next frame. The one-sample
    /// delay is inaudible and means a send can point anywhere in the mixer.
    send_l: Vec<f32>,
    send_r: Vec<f32>,
    /// Per-channel accumulation for a single frame.
    chan_l: Vec<f32>,
    chan_r: Vec<f32>,
    synths: Vec<Option<Box<Synthesizer>>>,
    sf_buf_l: Vec<Vec<f32>>,
    sf_buf_r: Vec<Vec<f32>>,
    /// Live metering, one entry per mixer track plus the master at index 0.
    pub meters: Vec<Meter>,
    metronome_phase: f32,
    metronome_env: f32,
    last_click_beat: f64,
    /// Deferred deallocation, drained by the control side.
    pub trash: Vec<Trash>,
    /// A tempo override coming from the transport rather than the project.
    tempo_override: Option<f64>,
    /// The snapshot generation the effect chains were last built for.
    chains_generation: u64,
    /// The snapshot generation the live-track set was worked out for.
    live_generation: u64,
    /// Which mixer tracks carry signal. Silent, effect-free inserts are skipped
    /// entirely, which is most of them in a typical project. Recomputed only
    /// when the snapshot changes.
    live_tracks: Vec<usize>,
    /// Whether each track is fed by a channel or a send, cached alongside.
    track_fed: Vec<bool>,
    /// How many frames have passed with nothing sounding. Once the longest
    /// plausible effect tail has run out there is nothing left to compute, and
    /// the engine can hand back silence for free until something happens.
    idle_frames: usize,
}

impl EngineCore {
    pub fn new(sample_rate: f32) -> Self {
        let snapshot = Arc::new(Snapshot::default());
        let track_count = snapshot.mixer.tracks.len();
        Self {
            sample_rate,
            snapshot,
            working: Box::new(Working::default()),
            transport: Transport::default(),
            pool: VoicePool::new(MAX_VOICES),
            pending_offs: Vec::with_capacity(MAX_VOICES * 2),
            cursor: 0,
            channel_chains: Vec::new(),
            mixer_chains: Vec::new(),
            bus_l: vec![0.0; track_count * CONTROL_BLOCK],
            bus_r: vec![0.0; track_count * CONTROL_BLOCK],
            send_l: vec![0.0; track_count],
            send_r: vec![0.0; track_count],
            chan_l: Vec::new(),
            chan_r: Vec::new(),
            synths: (0..MAX_SF_SLOTS).map(|_| None).collect(),
            sf_buf_l: (0..MAX_SF_SLOTS)
                .map(|_| vec![0.0; CONTROL_BLOCK])
                .collect(),
            sf_buf_r: (0..MAX_SF_SLOTS)
                .map(|_| vec![0.0; CONTROL_BLOCK])
                .collect(),
            meters: vec![Meter::default(); track_count],
            metronome_phase: 0.0,
            metronome_env: 0.0,
            last_click_beat: -1.0,
            trash: Vec::with_capacity(16),
            tempo_override: None,
            chains_generation: u64::MAX,
            live_generation: u64::MAX,
            live_tracks: Vec::with_capacity(track_count),
            track_fed: vec![false; track_count],
            idle_frames: usize::MAX,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        if (sr - self.sample_rate).abs() > 0.5 {
            self.sample_rate = sr;
            // Every effect has to be rebuilt at the new rate; the control side
            // notices the empty chains and sends fresh processors.
            self.reset_effects();
            self.pool.kill_all();
        }
    }

    pub fn snapshot(&self) -> &Arc<Snapshot> {
        &self.snapshot
    }

    pub fn tempo(&self) -> f64 {
        self.tempo_override.unwrap_or(self.working.tempo)
    }

    pub fn active_voices(&self) -> usize {
        self.pool.active_count()
    }

    pub fn handle(&mut self, cmd: EngineCmd) {
        // Any command at all might start something sounding, so come out of
        // the idle fast path and let the next block decide afresh.
        self.idle_frames = 0;
        match cmd {
            EngineCmd::Play => {
                self.transport.playing = true;
            }
            EngineCmd::Pause => {
                self.transport.playing = false;
                self.release_all(false);
            }
            EngineCmd::Stop => {
                self.transport.playing = false;
                self.transport.position = 0.0;
                self.cursor = 0;
                self.pending_offs.clear();
                self.pool.kill_all();
                self.reset_effects();
            }
            EngineCmd::TogglePlay => {
                self.transport.playing = !self.transport.playing;
                if !self.transport.playing {
                    self.release_all(false);
                }
            }
            EngineCmd::Seek(beat) => {
                self.transport.position = beat.max(0.0);
                self.cursor = self.snapshot.schedule.seek(self.transport.position);
                self.pending_offs.clear();
                self.release_all(true);
            }
            EngineCmd::SetLoopEnabled(v) => self.transport.loop_enabled = v,
            EngineCmd::SetMetronome(v) => self.transport.metronome = v,
            EngineCmd::SetTempo(t) => {
                self.tempo_override = Some(t.clamp(20.0, 999.0));
            }
            EngineCmd::NoteOn {
                channel,
                key,
                velocity,
            } => {
                self.note_on(channel, None, key, velocity, 0.0, 0.0);
            }
            EngineCmd::NoteOff { channel, key } => {
                self.note_off_key(channel, key);
            }
            EngineCmd::AllNotesOff => self.release_all(false),
            EngineCmd::SetSynth { slot, synth } => {
                if slot < MAX_SF_SLOTS {
                    if let Some(old) = self.synths[slot].take() {
                        self.trash.push(Trash::Synth(old));
                    }
                    self.synths[slot] = synth;
                }
            }
            EngineCmd::SetSnapshot(snapshot, working) => {
                let old = std::mem::replace(&mut self.snapshot, snapshot);
                self.trash.push(Trash::Snapshot(old));
                // Swap the boxes rather than unboxing, so nothing is allocated
                // here just to be thrown away.
                let old_working = std::mem::replace(&mut self.working, working);
                self.trash.push(Trash::Working(old_working));
                self.tempo_override = None;
                self.resize_buffers();
                self.cursor = self.snapshot.schedule.seek(self.transport.position);
                // Re-point sounding voices at their channel's new position, and
                // drop any whose channel has gone away.
                let snapshot = self.snapshot.clone();
                for v in &mut self.pool.voices {
                    if !v.active {
                        continue;
                    }
                    match snapshot.channels.iter().position(|c| c.id == v.channel) {
                        Some(index) => v.slot = index,
                        None => v.kill(),
                    }
                }
            }
            EngineCmd::SetMode(mode) => {
                self.transport.mode = mode;
                self.transport.position = 0.0;
                self.cursor = 0;
                self.pending_offs.clear();
                self.release_all(true);
            }
            EngineCmd::InstallEffect { target, slot, proc } => {
                let chain = match target {
                    FxTarget::Channel(i) => self.channel_chains.get_mut(i),
                    FxTarget::Mixer(i) => self.mixer_chains.get_mut(i),
                };
                if let Some(chain) = chain {
                    chain.install(slot, *proc, &mut self.trash);
                }
            }
            EngineCmd::TruncateChain { target, len } => {
                let chain = match target {
                    FxTarget::Channel(i) => self.channel_chains.get_mut(i),
                    FxTarget::Mixer(i) => self.mixer_chains.get_mut(i),
                };
                if let Some(chain) = chain {
                    chain.truncate(len, &mut self.trash);
                }
            }
            EngineCmd::Panic => {
                self.pool.kill_all();
                self.pending_offs.clear();
                for s in self.synths.iter_mut().flatten() {
                    s.note_off_all(true);
                }
                self.reset_effects();
            }
        }
    }

    /// Silence every effect's state without freeing anything on this thread.
    fn reset_effects(&mut self) {
        for chain in self
            .channel_chains
            .iter_mut()
            .chain(self.mixer_chains.iter_mut())
        {
            chain.truncate(0, &mut self.trash);
        }
        self.chains_generation = u64::MAX;
    }

    fn release_all(&mut self, immediate: bool) {
        if immediate {
            self.pool.kill_all();
        } else {
            for v in &mut self.pool.voices {
                if v.active {
                    v.release(true);
                }
            }
        }
        self.pending_offs.clear();
        for s in self.synths.iter_mut().flatten() {
            s.note_off_all(immediate);
        }
    }

    fn resize_buffers(&mut self) {
        let tracks = self.snapshot.mixer.tracks.len().max(1);
        if self.bus_l.len() != tracks * CONTROL_BLOCK {
            self.bus_l.resize(tracks * CONTROL_BLOCK, 0.0);
            self.bus_r.resize(tracks * CONTROL_BLOCK, 0.0);
        }
        if self.send_l.len() != tracks {
            self.send_l.resize(tracks, 0.0);
            self.send_r.resize(tracks, 0.0);
            self.send_l.iter_mut().for_each(|v| *v = 0.0);
            self.send_r.iter_mut().for_each(|v| *v = 0.0);
        }
        if self.meters.len() != tracks {
            self.meters.resize(tracks, Meter::default());
        }
        let channels = self.snapshot.channels.len();
        if self.chan_l.len() != channels {
            self.chan_l.resize(channels, 0.0);
            self.chan_r.resize(channels, 0.0);
        }
        // The working copy arrives with its snapshot. If they ever disagree —
        // a snapshot set without one — rebuild rather than render nonsense.
        if self.working.generation != self.snapshot.generation {
            let snapshot = self.snapshot.clone();
            self.working.rebuild(&snapshot);
        }
        while self.channel_chains.len() < channels {
            self.channel_chains.push(EffectChain::default());
        }
        self.channel_chains.truncate(channels);
        while self.mixer_chains.len() < tracks {
            self.mixer_chains.push(EffectChain::default());
        }
        self.mixer_chains.truncate(tracks);
        // Chains that are now longer than their effect list are trimmed here;
        // building the processors themselves belongs to the control thread.
        if self.chains_generation != self.snapshot.generation {
            for (i, chain) in self.channel_chains.iter_mut().enumerate() {
                let wanted = self.working.channel_effects[i].len();
                if chain.len() > wanted {
                    chain.truncate(wanted, &mut self.trash);
                }
            }
            for (i, chain) in self.mixer_chains.iter_mut().enumerate() {
                let wanted = self.working.mixer_effects[i].len();
                if chain.len() > wanted {
                    chain.truncate(wanted, &mut self.trash);
                }
            }
            self.chains_generation = self.snapshot.generation;
        }
    }

    fn channel_index(&self, id: Id) -> Option<usize> {
        self.snapshot.channels.iter().position(|c| c.id == id)
    }

    fn note_on(
        &mut self,
        channel: Id,
        uid: Option<u32>,
        key: u8,
        velocity: f32,
        pan: f32,
        fine: f32,
    ) {
        let Some(index) = self.channel_index(channel) else {
            return;
        };
        let snapshot = self.snapshot.clone();
        let ch = &snapshot.channels[index];
        if !ch.audible {
            return;
        }
        let transpose = ch.transpose;
        let sr = self.sample_rate;

        match &self.working.channel_instrument[index] {
            Instrument::Synth(patch) => {
                let patch = patch.clone();
                if patch.mono
                    && let Some(vi) = self.pool.mono_voice(channel)
                {
                    // A mono channel reuses its sounding voice, so glide and
                    // legato behave the way a monosynth should.
                    self.pool.voices[vi].glide_to(key, transpose, fine, &patch, sr);
                    self.pool.voices[vi].velocity = velocity;
                    self.pool.voices[vi].note = uid;
                    return;
                }
                let age = self.pool.tick();
                let slot = self.pool.allocate();
                self.pool.voices[slot].start_synth(
                    channel, index, uid, key, velocity, pan, fine, transpose, &patch, sr, age, None,
                );
            }
            Instrument::Sampler(patch) => {
                let patch = patch.clone();
                let sample = ch.sample.clone();
                let age = self.pool.tick();
                let slot = self.pool.allocate();
                self.pool.voices[slot].start_sampler(
                    channel, index, uid, key, velocity, pan, fine, transpose, &patch, sample, sr,
                    age,
                );
            }
            Instrument::SoundFont(patch) => {
                if let Some(slot) = ch.sf_slot {
                    let midi_channel = if patch.percussion { 9 } else { 0 };
                    if let Some(synth) = self.synths.get_mut(slot).and_then(|s| s.as_mut()) {
                        let k = (key as f32 + transpose).clamp(0.0, 127.0) as i32;
                        synth.note_on(midi_channel, k, (velocity * 127.0).clamp(1.0, 127.0) as i32);
                    }
                }
            }
        }
    }

    fn note_off_uid(&mut self, channel: Id, uid: u32, key: u8) {
        let Some(index) = self.channel_index(channel) else {
            return;
        };
        let gated = match &self.working.channel_instrument[index] {
            Instrument::Sampler(p) => p.gated,
            _ => true,
        };
        if let Instrument::SoundFont(p) = &self.working.channel_instrument[index] {
            if let Some(slot) = self.snapshot.channels[index].sf_slot {
                let midi_channel = if p.percussion { 9 } else { 0 };
                let transpose = self.snapshot.channels[index].transpose;
                if let Some(synth) = self.synths.get_mut(slot).and_then(|s| s.as_mut()) {
                    let k = (key as f32 + transpose).clamp(0.0, 127.0) as i32;
                    synth.note_off(midi_channel, k);
                }
            }
            return;
        }
        self.pool.release_note(channel, uid, gated);
    }

    fn note_off_key(&mut self, channel: Id, key: u8) {
        let Some(index) = self.channel_index(channel) else {
            return;
        };
        match &self.working.channel_instrument[index] {
            Instrument::SoundFont(p) => {
                if let Some(slot) = self.snapshot.channels[index].sf_slot {
                    let midi_channel = if p.percussion { 9 } else { 0 };
                    let transpose = self.snapshot.channels[index].transpose;
                    if let Some(synth) = self.synths.get_mut(slot).and_then(|s| s.as_mut()) {
                        let k = (key as f32 + transpose).clamp(0.0, 127.0) as i32;
                        synth.note_off(midi_channel, k);
                    }
                }
            }
            Instrument::Sampler(p) => {
                let gated = p.gated;
                self.pool.release_key(channel, key, gated);
            }
            _ => self.pool.release_key(channel, key, true),
        }
    }

    /// Loop bounds in beats for the current mode.
    fn loop_bounds(&self) -> (f64, f64) {
        match self.transport.mode {
            PlayMode::Pattern(_) => (0.0, self.snapshot.schedule.length),
            PlayMode::Song => match self.snapshot.loop_region {
                Some((a, b)) if b > a => (a, b),
                _ => (0.0, self.snapshot.schedule.length),
            },
        }
    }

    /// Apply every automation lane at the current position.
    fn apply_automation(&mut self, beat: f64) {
        if self.snapshot.schedule.automation.is_empty() {
            return;
        }
        let snapshot = self.snapshot.clone();
        for lane in &snapshot.schedule.automation {
            let Some(value) = lane.value_at(beat) else {
                continue;
            };
            let v = value.clamp(0.0, 1.0) as f32;
            match lane.target {
                AutomationTarget::Tempo => {
                    self.tempo_override = Some(20.0 + value.clamp(0.0, 1.0) * 280.0);
                }
                AutomationTarget::MasterVolume => self.working.master_volume = v * 2.0,
                AutomationTarget::ChannelVolume(id) => {
                    if let Some(i) = self.channel_index(id) {
                        self.working.channel_volume[i] = v * 2.0;
                    }
                }
                AutomationTarget::ChannelPan(id) => {
                    if let Some(i) = self.channel_index(id) {
                        self.working.channel_pan[i] = v * 2.0 - 1.0;
                    }
                }
                AutomationTarget::MixerVolume(t) => {
                    if let Some(slot) = self.working.mixer_volume.get_mut(t) {
                        *slot = v * 2.0;
                    }
                }
                AutomationTarget::MixerPan(t) => {
                    if let Some(slot) = self.working.mixer_pan.get_mut(t) {
                        *slot = v * 2.0 - 1.0;
                    }
                }
                AutomationTarget::MixerEffect { track, slot, param } => {
                    if let Some(fx) = self
                        .working
                        .mixer_effects
                        .get_mut(track)
                        .and_then(|c| c.get_mut(slot))
                    {
                        set_effect_normalised(fx, param.as_str(), v);
                    }
                }
                AutomationTarget::ChannelEffect {
                    channel,
                    slot,
                    param,
                } => {
                    if let Some(i) = snapshot.channels.iter().position(|c| c.id == channel)
                        && let Some(fx) = self
                            .working
                            .channel_effects
                            .get_mut(i)
                            .and_then(|c| c.get_mut(slot))
                    {
                        set_effect_normalised(fx, param.as_str(), v);
                    }
                }
                AutomationTarget::Instrument { channel, param } => {
                    if let Some(i) = snapshot.channels.iter().position(|c| c.id == channel)
                        && let Some(inst) = self.working.channel_instrument.get_mut(i)
                    {
                        inst.set_param_normalised(param.as_str(), v);
                    }
                }
            }
        }
    }

    /// Fire everything scheduled between `from` and `to`.
    fn advance_schedule(&mut self, from: f64, to: f64) {
        // Note-offs first, so a repeated note releases before it retriggers.
        let mut i = 0;
        while i < self.pending_offs.len() {
            if self.pending_offs[i].end <= to {
                let off = self.pending_offs.swap_remove(i);
                self.note_off_uid(off.channel, off.uid, off.key);
            } else {
                i += 1;
            }
        }

        let snapshot = self.snapshot.clone();
        let notes = &snapshot.schedule.notes;
        while self.cursor < notes.len() {
            let n = notes[self.cursor];
            if n.beat >= to {
                break;
            }
            if n.beat >= from {
                self.note_on(n.channel, Some(n.uid), n.key, n.velocity, n.pan, n.fine);
                // If the queue is full, release whatever is due first rather
                // than dropping this note's release and hanging the voice.
                if self.pending_offs.len() >= MAX_VOICES * 2
                    && let Some(index) = self
                        .pending_offs
                        .iter()
                        .enumerate()
                        .min_by(|a, b| {
                            a.1.end
                                .partial_cmp(&b.1.end)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|(i, _)| i)
                {
                    let stale = self.pending_offs.swap_remove(index);
                    self.note_off_uid(stale.channel, stale.uid, stale.key);
                }
                self.pending_offs.push(PendingOff {
                    end: n.end,
                    channel: n.channel,
                    uid: n.uid,
                    key: n.key,
                });
            }
            self.cursor += 1;
        }
    }

    /// Render `frames` of interleaved stereo into `out`.
    pub fn render(&mut self, out: &mut [f32]) {
        self.resize_buffers();
        for m in &mut self.meters {
            m.peak_left = 0.0;
            m.peak_right = 0.0;
        }

        let frames = out.len() / 2;
        let mut done = 0;
        while done < frames {
            let block = (frames - done).min(CONTROL_BLOCK);
            self.render_block(&mut out[done * 2..(done + block) * 2], block);
            done += block;
        }
    }

    /// Whether anything at all could be making sound right now.
    fn is_sounding(&self) -> bool {
        self.transport.playing
            || self.metronome_env > 1e-4
            || self.pool.voices.iter().any(|v| v.active)
            || self.synths.iter().any(Option::is_some)
    }

    fn render_block(&mut self, out: &mut [f32], frames: usize) {
        // Once nothing has sounded for long enough that even a long reverb tail
        // has decayed, there is no arithmetic worth doing. This is the usual
        // state while someone is editing, and it keeps the callback free.
        if self.is_sounding() {
            self.idle_frames = 0;
        } else {
            let tail = (self.sample_rate * 6.0) as usize;
            if self.idle_frames < tail {
                self.idle_frames = self.idle_frames.saturating_add(frames);
            } else {
                out.fill(0.0);
                for m in &mut self.meters {
                    m.peak_left = 0.0;
                    m.peak_right = 0.0;
                }
                return;
            }
        }

        let sr = self.sample_rate;
        let tempo = self.tempo();
        let beats_per_sample = tempo / (60.0 * sr as f64);
        let block_beats = beats_per_sample * frames as f64;

        if self.transport.playing {
            let (loop_start, loop_end) = self.loop_bounds();
            let from = self.transport.position;
            let to = from + block_beats;

            if self.transport.loop_enabled && to >= loop_end && loop_end > loop_start {
                // Fire the tail of the loop, then wrap.
                self.advance_schedule(from, loop_end);
                // Anything still held at the loop point has to be released here:
                // once the playhead jumps back, its note-off can never arrive.
                for off in std::mem::take(&mut self.pending_offs) {
                    self.note_off_uid(off.channel, off.uid, off.key);
                }
                let overshoot = to - loop_end;
                self.transport.position = loop_start + overshoot;
                self.cursor = self.snapshot.schedule.seek(loop_start);
                self.advance_schedule(loop_start, self.transport.position);
            } else {
                self.advance_schedule(from, to);
                self.transport.position = to;
                if !self.transport.loop_enabled && to >= loop_end {
                    self.transport.playing = false;
                    self.release_all(false);
                }
            }
            self.apply_automation(self.transport.position);

            // Metronome: one click per beat, accented on the bar.
            if self.transport.metronome {
                let beat_index = self.transport.position.floor();
                if beat_index != self.last_click_beat {
                    self.last_click_beat = beat_index;
                    let bpb = self.snapshot.time_signature.beats_per_bar();
                    let accent = (beat_index % bpb).abs() < 0.5;
                    self.metronome_env = if accent { 1.0 } else { 0.55 };
                    self.metronome_phase = 0.0;
                }
            }
        }

        let track_count = self.snapshot.mixer.tracks.len().max(1);
        for v in self.bus_l.iter_mut().take(track_count * frames) {
            *v = 0.0;
        }
        for v in self.bus_r.iter_mut().take(track_count * frames) {
            *v = 0.0;
        }

        // Render every soundfont slot that is in use for this block.
        let snapshot = self.snapshot.clone();
        for slot in 0..MAX_SF_SLOTS {
            let used = snapshot.channels.iter().any(|c| c.sf_slot == Some(slot));
            if !used {
                continue;
            }
            let (l, r) = (&mut self.sf_buf_l[slot], &mut self.sf_buf_r[slot]);
            match self.synths[slot].as_mut() {
                Some(synth) => synth.render(&mut l[..frames], &mut r[..frames]),
                // No synth loaded: clear the block, or the last one it rendered
                // would be mixed in again for every block from now on.
                None => {
                    l[..frames].fill(0.0);
                    r[..frames].fill(0.0);
                }
            }
        }

        let channel_count = snapshot.channels.len();
        let spb = (60.0 / tempo) as f32;
        // Solo state cannot change mid-block, so resolve it once.
        let solo_active = snapshot.mixer.any_solo();

        // An insert is worth processing if a channel feeds it, if it holds
        // effects (which may still be ringing out), or if a send returns into
        // it. Everything else is silence and can be skipped outright. The set
        // only changes with the snapshot, so it is worked out there.
        if self.live_generation != snapshot.generation {
            self.track_fed.resize(track_count, false);
            self.live_tracks.clear();
            for track in 1..track_count {
                let t = &snapshot.mixer.tracks[track];
                if t.muted || (solo_active && !t.solo) {
                    self.track_fed[track] = false;
                    continue;
                }
                let fed = snapshot
                    .channels
                    .iter()
                    .any(|c| c.audible && c.mixer_track.min(track_count - 1) == track);
                let has_effects = !self.working.mixer_effects[track].is_empty();
                let receives_send = snapshot.mixer.tracks.iter().any(|src| {
                    src.sends
                        .iter()
                        .any(|(dst, amt)| *dst == track && *amt > 0.0)
                });
                self.track_fed[track] = fed || has_effects || receives_send;
                if self.track_fed[track] {
                    self.live_tracks.push(track);
                }
            }
            self.live_generation = snapshot.generation;
        }

        for f in 0..frames {
            for c in self.chan_l.iter_mut().take(channel_count) {
                *c = 0.0;
            }
            for c in self.chan_r.iter_mut().take(channel_count) {
                *c = 0.0;
            }

            // Voices. `slot` is kept in step with the snapshot at swap time, so
            // this stays a direct index rather than a per-sample search.
            for v in &mut self.pool.voices {
                if !v.active {
                    continue;
                }
                let index = v.slot;
                if index >= channel_count {
                    v.kill();
                    continue;
                }
                let (l, r) = v.render(&self.working.channel_instrument[index], sr);
                self.chan_l[index] += l;
                self.chan_r[index] += r;
            }

            // SoundFont channels.
            for (index, ch) in snapshot.channels.iter().enumerate() {
                if let Some(slot) = ch.sf_slot {
                    self.chan_l[index] += self.sf_buf_l[slot][f];
                    self.chan_r[index] += self.sf_buf_r[slot][f];
                }
            }

            // Channel strip: insert effects, volume, pan, route to the mixer.
            for (index, ch) in snapshot.channels.iter().enumerate() {
                let mut l = self.chan_l[index];
                let mut r = self.chan_r[index];
                if l == 0.0 && r == 0.0 && self.working.channel_effects[index].is_empty() {
                    continue;
                }
                if !self.working.channel_effects[index].is_empty() {
                    let (nl, nr) = self.channel_chains[index].process(
                        l,
                        r,
                        &self.working.channel_effects[index],
                        sr,
                        spb,
                    );
                    l = nl;
                    r = nr;
                }
                let vol = self.working.channel_volume[index];
                let (gl, gr) = crate::audio::dsp::pan_gains(self.working.channel_pan[index]);
                let track = ch.mixer_track.min(track_count - 1);
                self.bus_l[track * frames + f] += l * vol * gl * std::f32::consts::SQRT_2;
                self.bus_r[track * frames + f] += r * vol * gr * std::f32::consts::SQRT_2;
            }

            // Metronome goes straight to master.
            if self.metronome_env > 1e-4 {
                self.metronome_phase += 1_760.0 / sr;
                if self.metronome_phase >= 1.0 {
                    self.metronome_phase -= 1.0;
                }
                let click = (self.metronome_phase * crate::audio::dsp::TAU).sin()
                    * self.metronome_env
                    * 0.25;
                self.metronome_env *= 1.0 - (14.0 / sr);
                self.bus_l[f] += click;
                self.bus_r[f] += click;
            }

            // Fold in last frame's send returns before the inserts run.
            if self.send_l[0] != 0.0 || self.send_r[0] != 0.0 {
                self.bus_l[f] += self.send_l[0];
                self.bus_r[f] += self.send_r[0];
                self.send_l[0] = 0.0;
                self.send_r[0] = 0.0;
            }
            for i in 0..self.live_tracks.len() {
                let track = self.live_tracks[i];
                if self.send_l[track] != 0.0 || self.send_r[track] != 0.0 {
                    self.bus_l[track * frames + f] += self.send_l[track];
                    self.bus_r[track * frames + f] += self.send_r[track];
                    self.send_l[track] = 0.0;
                    self.send_r[track] = 0.0;
                }
            }

            // Mixer inserts, then master.
            for i in 0..self.live_tracks.len() {
                let track = self.live_tracks[i];
                let idx = track * frames + f;
                let mut l = self.bus_l[idx];
                let mut r = self.bus_r[idx];
                let t = &snapshot.mixer.tracks[track];
                if !self.working.mixer_effects[track].is_empty() {
                    let (nl, nr) = self.mixer_chains[track].process(
                        l,
                        r,
                        &self.working.mixer_effects[track],
                        sr,
                        spb,
                    );
                    l = nl;
                    r = nr;
                }
                let vol = self.working.mixer_volume[track];
                let (gl, gr) = crate::audio::dsp::pan_gains(self.working.mixer_pan[track]);
                let out_l = l * vol * gl * std::f32::consts::SQRT_2;
                let out_r = r * vol * gr * std::f32::consts::SQRT_2;

                self.meters[track].peak_left = self.meters[track].peak_left.max(out_l.abs());
                self.meters[track].peak_right = self.meters[track].peak_right.max(out_r.abs());

                for (dst, amount) in &t.sends {
                    let d = (*dst).min(track_count - 1);
                    if d == track {
                        continue;
                    }
                    self.send_l[d] += out_l * amount;
                    self.send_r[d] += out_r * amount;
                }
                self.bus_l[f] += out_l;
                self.bus_r[f] += out_r;
            }

            // Master bus. A snapshot with no mixer tracks should not be
            // possible, but the engine must not panic if one turns up.
            let mut l = self.bus_l[f];
            let mut r = self.bus_r[f];
            if self
                .working
                .mixer_effects
                .first()
                .map(|fx| !fx.is_empty())
                .unwrap_or(false)
            {
                let (nl, nr) =
                    self.mixer_chains[0].process(l, r, &self.working.mixer_effects[0], sr, spb);
                l = nl;
                r = nr;
            }
            let master = self.working.mixer_volume.first().copied().unwrap_or(1.0)
                * self.working.master_volume;
            let (gl, gr) = crate::audio::dsp::pan_gains(
                self.working.mixer_pan.first().copied().unwrap_or(0.0),
            );
            l *= master * gl * std::f32::consts::SQRT_2;
            r *= master * gr * std::f32::consts::SQRT_2;

            // A gentle safety clip, so a runaway patch can't blow up the output.
            l = crate::audio::dsp::soft_clip(l * 0.85) * 1.1;
            r = crate::audio::dsp::soft_clip(r * 0.85) * 1.1;
            if !l.is_finite() {
                l = 0.0;
            }
            if !r.is_finite() {
                r = 0.0;
            }

            self.meters[0].peak_left = self.meters[0].peak_left.max(l.abs());
            self.meters[0].peak_right = self.meters[0].peak_right.max(r.abs());

            out[f * 2] = l;
            out[f * 2 + 1] = r;
        }
    }
}

/// Write a normalised 0..1 value into a named effect parameter.
fn set_effect_normalised(effect: &mut Effect, param: &str, t: f32) {
    let Some(&(_, _, lo, hi)) = effect.kind.params().iter().find(|(n, _, _, _)| *n == param) else {
        return;
    };
    let value = lo + (hi - lo) * t.clamp(0.0, 1.0);
    let _ = effect.kind.set_param(param, value);
}
