//! LimeDAW — a pattern-based digital audio workstation.
//!
//! The crate is laid out so the musical model, the audio engine and the
//! interface stay separable: `model` is plain data, `audio` turns it into
//! sound, `ui` edits it, and `mcp` exposes the whole thing to an assistant.

pub mod actions;
pub mod app;
pub mod audio;
pub mod config;
pub mod editing;
pub mod icon;
pub mod mcp;
pub mod mcp_dispatch;
pub mod model;
pub mod paths;
pub mod persist;
pub mod theme;
pub mod ui;

pub use app::{LimeDaw, Message};

/// The size the window should open at.
///
/// On the desktop this is a sensible default. In a browser the canvas has to
/// match the viewport or it is scaled by CSS and everything looks soft and
/// mispositioned, so the page's own dimensions are used instead.
fn initial_window_size() -> iced::Size {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(size) = browser_viewport() {
            return size;
        }
    }
    iced::Size::new(1440.0, 900.0)
}

/// The browser's inner dimensions, in logical pixels.
#[cfg(target_arch = "wasm32")]
pub fn browser_viewport() -> Option<iced::Size> {
    let window = web_sys::window()?;
    let width = window.inner_width().ok()?.as_f64()?;
    let height = window.inner_height().ok()?.as_f64()?;
    (width > 1.0 && height > 1.0).then(|| iced::Size::new(width as f32, height as f32))
}

/// Start the interface. Shared by the desktop binary and the wasm entry point.
pub fn run() -> iced::Result {
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);
    }

    let settings = config::Settings::load();

    // The renderer is chosen when the window is created, so the preference has
    // to be in place before iced starts.
    #[cfg(target_arch = "wasm32")]
    let _ = &settings;

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(backend) = settings.renderer.backend_env() {
        // SAFETY: called before any window or thread that reads the environment.
        unsafe { std::env::set_var("ICED_BACKEND", backend) };
    }

    iced::application(LimeDaw::new, LimeDaw::update, LimeDaw::view)
        .title(LimeDaw::title)
        .theme(LimeDaw::theme)
        .style(LimeDaw::style)
        .scale_factor(LimeDaw::scale_factor)
        .subscription(LimeDaw::subscription)
        .antialiasing(true)
        .window(iced::window::Settings {
            size: initial_window_size(),
            // The browser canvas is whatever the page gives it, so a minimum
            // would only fight the viewport there.
            min_size: (!cfg!(target_arch = "wasm32")).then(|| iced::Size::new(880.0, 560.0)),
            icon: icon::window_icon(),
            ..Default::default()
        })
        .run()
}
