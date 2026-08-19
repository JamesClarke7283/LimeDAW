//! The desktop audio host: a cpal output stream driving the engine core.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use crate::audio::core::EngineCore;
use crate::audio::{EngineShared, pump};

/// The buffer size to ask the device for, in frames. About 21 ms at 48 kHz:
/// comfortably below the latency anyone notices while playing, and far enough
/// from the edge that an ordinary scheduling delay does not cause a dropout.
const PREFERRED_BUFFER: u32 = 1024;

pub struct NativeHost {
    _stream: cpal::Stream,
    pub device_name: String,
    pub sample_rate: f32,
    pub channels: u16,
    /// The buffer size the device actually agreed to, in frames.
    pub buffer_frames: u32,
}

impl NativeHost {
    pub fn start(shared: Arc<EngineShared>, requested_buffer: u32) -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no default output device"))?;
        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "output".into());
        let supported = device
            .default_output_config()
            .context("querying the default output config")?;

        let sample_format = supported.sample_format();
        let buffer_size = supported.buffer_size();
        let mut config: StreamConfig = supported.into();

        // Ask for a buffer big enough to ride out a scheduling hiccup. Left to
        // its own devices a modern server hands out a tiny quantum, and any
        // jitter at all then shows up as an underrun.
        let wanted = if requested_buffer == 0 {
            PREFERRED_BUFFER
        } else {
            requested_buffer
        };
        let agreed = match buffer_size {
            cpal::SupportedBufferSize::Range { min, max } => {
                let frames = wanted.clamp(*min, *max);
                config.buffer_size = cpal::BufferSize::Fixed(frames);
                frames
            }
            cpal::SupportedBufferSize::Unknown => {
                config.buffer_size = cpal::BufferSize::Default;
                0
            }
        };

        let sample_rate = config.sample_rate as f32;
        let channels = config.channels.max(1);

        let mut core = EngineCore::new(sample_rate);
        let mut scratch: Vec<f32> = Vec::with_capacity(4096);
        let status_shared = shared.clone();

        // The engine always renders interleaved stereo; this fans it out to
        // whatever channel count the device actually wants.
        macro_rules! build {
            ($sample:ty, $convert:expr) => {{
                let err_shared = shared.clone();
                device.build_output_stream(
                    config.clone(),
                    move |data: &mut [$sample], _: &cpal::OutputCallbackInfo| {
                        let started = Instant::now();
                        let frames = data.len() / channels as usize;
                        if scratch.len() < frames * 2 {
                            scratch.resize(frames * 2, 0.0);
                        }
                        pump(&status_shared, &mut core, &mut scratch[..frames * 2]);

                        let convert: fn(f32) -> $sample = $convert;
                        for (i, frame) in data.chunks_mut(channels as usize).enumerate() {
                            let l = scratch[i * 2];
                            let r = scratch[i * 2 + 1];
                            match frame.len() {
                                1 => frame[0] = convert((l + r) * 0.5),
                                _ => {
                                    frame[0] = convert(l);
                                    frame[1] = convert(r);
                                    for extra in frame.iter_mut().skip(2) {
                                        *extra = convert(0.0);
                                    }
                                }
                            }
                        }

                        let budget = frames as f32 / sample_rate;
                        let load = started.elapsed().as_secs_f32() / budget.max(1e-6);
                        status_shared.status.publish(&core, load);
                    },
                    {
                        // Device errors arrive in bursts. Logging every one of
                        // them costs more time than the dropout itself, so
                        // report the first and then summarise.
                        let mut seen: u64 = 0;
                        let mut last_report = Instant::now();
                        move |err| {
                            seen += 1;
                            if seen == 1 {
                                log::warn!("audio stream error: {err}");
                            } else if last_report.elapsed() >= Duration::from_secs(10) {
                                log::warn!("audio stream: {seen} device errors in the last 10s");
                                seen = 0;
                                last_report = Instant::now();
                            }
                            let _ = &err_shared;
                        }
                    },
                    None,
                )?
            }};
        }

        let stream = match sample_format {
            SampleFormat::F32 => build!(f32, |v| v),
            SampleFormat::I16 => build!(i16, |v: f32| (v.clamp(-1.0, 1.0) * 32_767.0) as i16),
            SampleFormat::U16 => {
                build!(
                    u16,
                    |v: f32| (((v.clamp(-1.0, 1.0) + 1.0) * 0.5) * 65_535.0) as u16
                )
            }
            SampleFormat::I32 => {
                build!(i32, |v: f32| (v.clamp(-1.0, 1.0) * 2_147_483_000.0) as i32)
            }
            other => return Err(anyhow!("unsupported sample format: {other:?}")),
        };

        stream.play().context("starting the output stream")?;

        log::info!(
            "audio: {device_name} at {sample_rate} Hz, {} frames",
            if agreed == 0 {
                "device default".to_string()
            } else {
                agreed.to_string()
            }
        );

        Ok(Self {
            _stream: stream,
            device_name,
            sample_rate,
            channels,
            buffer_frames: agreed,
        })
    }
}

/// Names of every output device the default host can see, for the settings UI.
pub fn output_devices() -> Vec<String> {
    let host = cpal::default_host();
    match host.output_devices() {
        Ok(devices) => devices
            .filter_map(|d| d.description().ok().map(|desc| desc.name().to_string()))
            .collect(),
        Err(_) => Vec::new(),
    }
}
