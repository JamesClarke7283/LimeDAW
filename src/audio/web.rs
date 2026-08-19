//! The browser audio host.
//!
//! WebAudio's worklets need shared memory to reach a single-threaded wasm
//! module, so this uses a `ScriptProcessorNode` instead: deprecated, but it
//! runs the engine on the main thread and works in every current browser.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{AudioContext, AudioProcessingEvent, ScriptProcessorNode};

use crate::audio::core::EngineCore;
use crate::audio::{EngineShared, pump};

const BUFFER_FRAMES: u32 = 2048;

pub struct WebHost {
    context: AudioContext,
    _node: ScriptProcessorNode,
    _callback: Closure<dyn FnMut(AudioProcessingEvent)>,
}

impl WebHost {
    pub fn start(shared: Arc<EngineShared>) -> Result<Self, JsValue> {
        let context = AudioContext::new()?;
        let sample_rate = context.sample_rate();

        let node = context.create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
            BUFFER_FRAMES,
            0,
            2,
        )?;

        let core = Rc::new(RefCell::new(EngineCore::new(sample_rate)));
        let scratch = Rc::new(RefCell::new(vec![0.0f32; BUFFER_FRAMES as usize * 2]));

        let cb_core = core.clone();
        let cb_scratch = scratch.clone();
        let cb_shared = shared.clone();
        let callback =
            Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |event: AudioProcessingEvent| {
                let Ok(buffer) = event.output_buffer() else {
                    return;
                };
                let frames = buffer.length() as usize;
                let mut core = cb_core.borrow_mut();
                let mut scratch = cb_scratch.borrow_mut();
                if scratch.len() < frames * 2 {
                    scratch.resize(frames * 2, 0.0);
                }
                pump(&cb_shared, &mut core, &mut scratch[..frames * 2]);
                cb_shared.status.publish(&core, 0.0);

                let mut left = vec![0.0f32; frames];
                let mut right = vec![0.0f32; frames];
                for i in 0..frames {
                    left[i] = scratch[i * 2];
                    right[i] = scratch[i * 2 + 1];
                }
                let _ = buffer.copy_to_channel(&left, 0);
                let _ = buffer.copy_to_channel(&right, 1);
            });

        node.set_onaudioprocess(Some(callback.as_ref().unchecked_ref()));
        node.connect_with_audio_node(&context.destination())?;

        Ok(Self {
            context,
            _node: node,
            _callback: callback,
        })
    }

    /// Browsers start an `AudioContext` suspended until the user interacts.
    pub fn resume(&self) {
        let _ = self.context.resume();
    }
}
