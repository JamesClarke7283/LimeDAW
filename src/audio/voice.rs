//! Voices: one sounding note. The pool is preallocated so note-on never
//! allocates on the audio thread.

use std::sync::Arc;

use crate::audio::dsp::*;
use crate::audio::samples::Sample;
use crate::model::{FilterKind, Id, LfoTarget, SamplerPatch, SynthPatch};

const MAX_UNISON: usize = 8;

/// The built-in subtractive synth voice.
pub struct SynthVoice {
    oscs: [[Osc; MAX_UNISON]; 3],
    amp_env: Adsr,
    filter_env: Adsr,
    filter_l: Svf,
    filter_r: Svf,
    lfo: Osc,
    lfo_delay_elapsed: f32,
    /// Glide target and current value, in MIDI key units.
    key_current: f32,
    key_target: f32,
    glide_rate: f32,
}

impl Default for SynthVoice {
    fn default() -> Self {
        Self {
            // Each oscillator gets its own noise seed so unison voices do not
            // share a waveform. Wrapping is deliberate: this is a hash, not a
            // quantity, and plain multiplication overflows here.
            oscs: std::array::from_fn(|o| {
                std::array::from_fn(|u| {
                    let seed = ((o * 31 + u) as u32).wrapping_mul(2_654_435_761);
                    Osc::with_seed(0x1234_5678 ^ seed)
                })
            }),
            amp_env: Adsr::default(),
            filter_env: Adsr::default(),
            filter_l: Svf::default(),
            filter_r: Svf::default(),
            lfo: Osc::default(),
            lfo_delay_elapsed: 0.0,
            key_current: 60.0,
            key_target: 60.0,
            glide_rate: 0.0,
        }
    }
}

impl SynthVoice {
    fn start(&mut self, patch: &SynthPatch, key: f32, sr: f32, glide_from: Option<f32>) {
        self.amp_env.configure(&patch.amp_env, sr);
        self.filter_env.configure(&patch.filter_env, sr);
        self.amp_env.retrigger_from_zero();
        self.filter_env.retrigger_from_zero();
        self.filter_l.reset();
        self.filter_r.reset();
        self.lfo_delay_elapsed = 0.0;
        self.lfo.reset(0.0);

        self.key_target = key;
        self.key_current = glide_from.unwrap_or(key);
        self.glide_rate = if patch.glide > 0.0 {
            1.0 / (patch.glide * sr)
        } else {
            1.0
        };

        for (o, osc_patch) in patch.osc.iter().enumerate() {
            if osc_patch.phase > 0.0 {
                for u in 0..MAX_UNISON {
                    let spread = u as f32 / MAX_UNISON as f32;
                    self.oscs[o][u].reset(osc_patch.phase + spread * 0.01);
                }
            }
        }
    }

    /// Retrigger for a legato note without restarting the envelopes.
    fn glide_to(&mut self, key: f32, patch: &SynthPatch, sr: f32) {
        self.key_target = key;
        self.glide_rate = if patch.glide > 0.0 {
            1.0 / (patch.glide * sr)
        } else {
            1.0
        };
    }

    fn release(&mut self) {
        self.amp_env.release();
        self.filter_env.release();
    }

    fn is_active(&self) -> bool {
        self.amp_env.is_active()
    }

    #[inline]
    fn render(&mut self, patch: &SynthPatch, velocity: f32, sr: f32) -> (f32, f32) {
        // Glide.
        if (self.key_target - self.key_current).abs() > 1e-4 {
            let step = (self.key_target - self.key_current).signum() * self.glide_rate * 12.0;
            self.key_current += step;
            if (self.key_target - self.key_current).abs() < step.abs() {
                self.key_current = self.key_target;
            }
        }

        // LFO.
        let mut lfo_value = 0.0;
        if patch.lfo.enabled {
            let raw = self.lfo.next(patch.lfo.waveform, patch.lfo.rate / sr, 0.5);
            let fade = if patch.lfo.delay > 0.0 {
                self.lfo_delay_elapsed += 1.0 / sr;
                (self.lfo_delay_elapsed / patch.lfo.delay).min(1.0)
            } else {
                1.0
            };
            lfo_value = raw * patch.lfo.depth * fade;
        }

        let pitch_mod = if matches!(patch.lfo.target, LfoTarget::Pitch) {
            lfo_value * 2.0
        } else {
            0.0
        };
        let pw_mod = if matches!(patch.lfo.target, LfoTarget::PulseWidth) {
            lfo_value * 0.4
        } else {
            0.0
        };
        let amp_mod = if matches!(patch.lfo.target, LfoTarget::Amplitude) {
            1.0 - lfo_value.abs()
        } else {
            1.0
        };
        let pan_mod = if matches!(patch.lfo.target, LfoTarget::Pan) {
            lfo_value
        } else {
            0.0
        };

        let base_key = self.key_current + pitch_mod;

        let mut left = 0.0f32;
        let mut right = 0.0f32;
        for (o, osc_patch) in patch.osc.iter().enumerate() {
            if !osc_patch.enabled || osc_patch.level <= 0.0 {
                continue;
            }
            let unison = osc_patch.unison.clamp(1, MAX_UNISON as u8) as usize;
            let level = osc_patch.level / (unison as f32).sqrt();
            for u in 0..unison {
                // Spread voices symmetrically around the centre pitch.
                let offset = if unison == 1 {
                    0.0
                } else {
                    (u as f32 / (unison - 1) as f32) * 2.0 - 1.0
                };
                let cents = osc_patch.cents + offset * osc_patch.spread;
                let key = base_key + osc_patch.semitones + cents / 100.0;
                let inc = key_to_hz(key) / sr;
                if inc >= 0.5 {
                    continue; // above Nyquist, skip rather than alias
                }
                let s = self.oscs[o][u].next(osc_patch.waveform, inc, 0.5 + pw_mod) * level;
                // Unison width pans the outer voices apart.
                let pan = offset * osc_patch.width;
                let (gl, gr) = pan_gains(pan);
                left += s * gl;
                right += s * gr;
            }
        }

        // Filter, with envelope, key tracking and optional LFO.
        let filter_env = self.filter_env.next();
        let mut cutoff = patch.cutoff;
        if patch.filter_env_amount != 0.0 {
            cutoff *= (patch.filter_env_amount * filter_env).exp2();
        }
        if patch.filter_key_track > 0.0 {
            cutoff *= 1.0 + (velocity - 0.5) * patch.filter_key_track * 2.0;
        }
        if matches!(patch.lfo.target, LfoTarget::FilterCutoff) {
            cutoff *= (lfo_value * 3.0).exp2();
        }
        let cutoff = cutoff.clamp(20.0, sr * 0.45);
        if !matches!(patch.filter, FilterKind::Off) {
            left = self
                .filter_l
                .process(left, patch.filter, cutoff, patch.resonance, sr);
            right = self
                .filter_r
                .process(right, patch.filter, cutoff, patch.resonance, sr);
        }

        if patch.drive > 0.0 {
            let d = 1.0 + patch.drive * 8.0;
            left = soft_clip(left * d) / (1.0 + patch.drive * 2.0);
            right = soft_clip(right * d) / (1.0 + patch.drive * 2.0);
        }

        let amp = self.amp_env.next()
            * patch.gain
            * amp_mod
            * (1.0 - patch.velocity_amount + patch.velocity_amount * velocity);

        let (pl, pr) = pan_gains(pan_mod);
        (
            left * amp * pl * std::f32::consts::SQRT_2,
            right * amp * pr * std::f32::consts::SQRT_2,
        )
    }
}

/// A sampler voice: plays one [`Sample`] with pitch, envelope and filtering.
#[derive(Default)]
pub struct SamplerVoice {
    sample: Option<Arc<Sample>>,
    position: f32,
    step: f32,
    env: Adsr,
    filter_l: Svf,
    filter_r: Svf,
    start_frame: f32,
    end_frame: f32,
    reverse: bool,
    looping: bool,
    finished: bool,
}

impl SamplerVoice {
    fn start(
        &mut self,
        patch: &SamplerPatch,
        sample: Option<Arc<Sample>>,
        key: u8,
        fine: f32,
        sr: f32,
    ) {
        self.env.configure(&patch.envelope, sr);
        self.env.retrigger_from_zero();
        self.filter_l.reset();
        self.filter_r.reset();
        self.finished = false;
        self.reverse = patch.reverse;
        self.looping = patch.loop_sample;

        let len = sample.as_ref().map(|s| s.len()).unwrap_or(0) as f32;
        self.start_frame = (patch.start.clamp(0.0, 1.0) * len).floor();
        self.end_frame = (patch.end.clamp(0.0, 1.0) * len)
            .ceil()
            .max(self.start_frame + 1.0);
        self.position = if self.reverse {
            self.end_frame - 1.0
        } else {
            self.start_frame
        };

        let semis = key as f32 - patch.root_key as f32 + patch.tune + (patch.fine + fine) / 100.0;
        let ratio = (semis / 12.0).exp2();
        let src_rate = sample.as_ref().map(|s| s.sample_rate).unwrap_or(sr);
        self.step = ratio * src_rate / sr;
        self.sample = sample;
    }

    fn release(&mut self, gated: bool) {
        // A looping voice has no natural end, so it must always honour the
        // note's release or it would sound forever.
        if gated || self.looping {
            self.env.release();
        }
    }

    fn is_active(&self) -> bool {
        !self.finished && self.env.is_active() && self.sample.is_some()
    }

    #[inline]
    fn render(&mut self, patch: &SamplerPatch, velocity: f32, sr: f32) -> (f32, f32) {
        let Some(sample) = self.sample.as_ref() else {
            self.finished = true;
            return (0.0, 0.0);
        };

        let (mut l, mut r) = sample.read(self.position);

        if self.reverse {
            self.position -= self.step;
            if self.position <= self.start_frame {
                if self.looping {
                    self.position = self.end_frame - 1.0;
                } else {
                    self.finished = true;
                }
            }
        } else {
            self.position += self.step;
            if self.position >= self.end_frame {
                if self.looping {
                    self.position = self.start_frame;
                } else {
                    self.finished = true;
                }
            }
        }

        if !matches!(patch.filter, FilterKind::Off) {
            l = self
                .filter_l
                .process(l, patch.filter, patch.cutoff, patch.resonance, sr);
            r = self
                .filter_r
                .process(r, patch.filter, patch.cutoff, patch.resonance, sr);
        }

        let amp = self.env.next()
            * patch.gain
            * (1.0 - patch.velocity_amount + patch.velocity_amount * velocity);
        (l * amp, r * amp)
    }
}

/// The sound-producing half of a voice.
///
/// The synth arm is much the larger, but the pool is preallocated and a voice
/// switches between the two in place, so boxing would only add an indirection
/// on the audio thread.
#[allow(clippy::large_enum_variant)]
pub enum VoiceKind {
    Synth(SynthVoice),
    Sampler(SamplerVoice),
}

/// One allocated voice in the pool.
pub struct Voice {
    pub active: bool,
    pub channel: Id,
    /// Index of the owning channel in the current snapshot. Kept in step with
    /// the snapshot so the per-sample loop never searches for it.
    pub slot: usize,
    pub key: u8,
    /// The note that spawned this voice, so the right voice gets released.
    pub note: Option<Id>,
    /// Monotonic counter used for voice stealing.
    pub age: u64,
    pub velocity: f32,
    pub pan: f32,
    pub fine: f32,
    /// True once the note has been released but the tail is still ringing.
    pub releasing: bool,
    pub kind: VoiceKind,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            channel: 0,
            slot: 0,
            key: 60,
            note: None,
            age: 0,
            velocity: 0.0,
            pan: 0.0,
            fine: 0.0,
            releasing: false,
            kind: VoiceKind::Synth(SynthVoice::default()),
        }
    }
}

impl Voice {
    /// Note-on for a synth voice. Everything a voice needs arrives at once
    /// because the audio thread has no room to look any of it up again.
    #[allow(clippy::too_many_arguments)]
    pub fn start_synth(
        &mut self,
        channel: Id,
        slot: usize,
        note: Option<Id>,
        key: u8,
        velocity: f32,
        pan: f32,
        fine: f32,
        transpose: f32,
        patch: &SynthPatch,
        sr: f32,
        age: u64,
        glide_from: Option<f32>,
    ) {
        if !matches!(self.kind, VoiceKind::Synth(_)) {
            self.kind = VoiceKind::Synth(SynthVoice::default());
        }
        let target_key = key as f32 + transpose + fine / 100.0;
        if let VoiceKind::Synth(v) = &mut self.kind {
            v.start(patch, target_key, sr, glide_from);
        }
        self.active = true;
        self.channel = channel;
        self.slot = slot;
        self.key = key;
        self.note = note;
        self.velocity = velocity;
        self.pan = pan;
        self.fine = fine;
        self.age = age;
        self.releasing = false;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_sampler(
        &mut self,
        channel: Id,
        slot: usize,
        note: Option<Id>,
        key: u8,
        velocity: f32,
        pan: f32,
        fine: f32,
        transpose: f32,
        patch: &SamplerPatch,
        sample: Option<Arc<Sample>>,
        sr: f32,
        age: u64,
    ) {
        if !matches!(self.kind, VoiceKind::Sampler(_)) {
            self.kind = VoiceKind::Sampler(SamplerVoice::default());
        }
        let key_with_transpose = (key as f32 + transpose).clamp(0.0, 127.0) as u8;
        if let VoiceKind::Sampler(v) = &mut self.kind {
            v.start(patch, sample, key_with_transpose, fine, sr);
        }
        self.active = true;
        self.channel = channel;
        self.slot = slot;
        self.key = key;
        self.note = note;
        self.velocity = velocity;
        self.pan = pan;
        self.fine = fine;
        self.age = age;
        self.releasing = false;
    }

    /// Move an already-sounding mono voice to a new pitch.
    pub fn glide_to(&mut self, key: u8, transpose: f32, fine: f32, patch: &SynthPatch, sr: f32) {
        if let VoiceKind::Synth(v) = &mut self.kind {
            v.glide_to(key as f32 + transpose + fine / 100.0, patch, sr);
        }
        self.key = key;
        self.releasing = false;
    }

    pub fn release(&mut self, gated: bool) {
        self.releasing = true;
        match &mut self.kind {
            VoiceKind::Synth(v) => v.release(),
            VoiceKind::Sampler(v) => v.release(gated),
        }
    }

    pub fn kill(&mut self) {
        self.active = false;
        self.note = None;
        match &mut self.kind {
            VoiceKind::Synth(v) => v.amp_env.kill(),
            VoiceKind::Sampler(v) => {
                v.env.kill();
                v.finished = true;
                v.sample = None;
            }
        }
    }

    /// Approximate current loudness, for choosing a voice to steal.
    pub fn loudness(&self) -> f32 {
        match &self.kind {
            VoiceKind::Synth(v) => v.amp_env.level(),
            VoiceKind::Sampler(v) => v.env.level(),
        }
    }

    #[inline]
    pub fn render(&mut self, instrument: &crate::model::Instrument, sr: f32) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }
        let (l, r) = match (&mut self.kind, instrument) {
            (VoiceKind::Synth(v), crate::model::Instrument::Synth(p)) => {
                let out = v.render(p, self.velocity, sr);
                if !v.is_active() {
                    self.active = false;
                }
                out
            }
            (VoiceKind::Sampler(v), crate::model::Instrument::Sampler(p)) => {
                let out = v.render(p, self.velocity, sr);
                if !v.is_active() {
                    self.active = false;
                }
                out
            }
            // The channel's instrument changed under the voice; let it go.
            _ => {
                self.active = false;
                (0.0, 0.0)
            }
        };
        if self.pan.abs() > 1e-4 {
            let (gl, gr) = pan_gains(self.pan);
            (
                l * gl * std::f32::consts::SQRT_2,
                r * gr * std::f32::consts::SQRT_2,
            )
        } else {
            (l, r)
        }
    }
}

/// A fixed-size voice pool with oldest-and-quietest stealing.
pub struct VoicePool {
    pub voices: Vec<Voice>,
    counter: u64,
}

impl VoicePool {
    pub fn new(size: usize) -> Self {
        Self {
            voices: (0..size).map(|_| Voice::default()).collect(),
            counter: 0,
        }
    }

    pub fn tick(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }

    pub fn active_count(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Find a slot: a free voice, otherwise the least useful sounding one.
    pub fn allocate(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return i;
        }
        // Prefer stealing something already in its release tail.
        let mut best = 0usize;
        let mut best_score = f32::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            let score = v.loudness() + if v.releasing { 0.0 } else { 1.0 } + (v.age as f32) * 1e-9;
            if score < best_score {
                best_score = score;
                best = i;
            }
        }
        self.voices[best].kill();
        best
    }

    pub fn release_note(&mut self, channel: Id, note: Id, gated: bool) {
        for v in &mut self.voices {
            if v.active && v.channel == channel && v.note == Some(note) && !v.releasing {
                v.release(gated);
            }
        }
    }

    pub fn release_key(&mut self, channel: Id, key: u8, gated: bool) {
        for v in &mut self.voices {
            if v.active && v.channel == channel && v.key == key && !v.releasing {
                v.release(gated);
            }
        }
    }

    pub fn release_channel(&mut self, channel: Id, gated: bool) {
        for v in &mut self.voices {
            if v.active && v.channel == channel {
                v.release(gated);
            }
        }
    }

    pub fn kill_all(&mut self) {
        for v in &mut self.voices {
            v.kill();
        }
    }

    /// The sounding voice on a mono channel, if any.
    pub fn mono_voice(&mut self, channel: Id) -> Option<usize> {
        self.voices
            .iter()
            .position(|v| v.active && v.channel == channel && !v.releasing)
    }
}
