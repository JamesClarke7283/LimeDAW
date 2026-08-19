//! The desktop audio host: a cpal output stream driving the engine core.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use crate::audio::core::EngineCore;
use crate::audio::{EngineShared, pump};

pub struct NativeHost {
    _stream: cpal::Stream,
    pub device_name: String,
    pub sample_rate: f32,
    pub channels: u16,
}

impl NativeHost {
    pub fn start(shared: Arc<EngineShared>) -> anyhow::Result<Self> {
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
        let config: StreamConfig = supported.into();
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
                    move |err| {
                        log::error!("audio stream error: {err}");
                        let _ = &err_shared;
                    },
                    None,
                )?
            }};
        }

        let stream = match sample_format {
            SampleFormat::F32 => build!(f32, |v| v),
            SampleFormat::I16 => build!(i16, |v: f32| (v.clamp(-1.0, 1.0) * 32_767.0) as i16),
            SampleFormat::U16 => {
                build!(u16, |v: f32| (((v.clamp(-1.0, 1.0) + 1.0) * 0.5) * 65_535.0) as u16)
            }
            SampleFormat::I32 => {
                build!(i32, |v: f32| (v.clamp(-1.0, 1.0) * 2_147_483_000.0) as i32)
            }
            other => return Err(anyhow!("unsupported sample format: {other:?}")),
        };

        stream.play().context("starting the output stream")?;

        Ok(Self { _stream: stream, device_name, sample_rate, channels })
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
