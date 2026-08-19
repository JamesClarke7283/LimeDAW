//! Persisted application settings.

use serde::{Deserialize, Serialize};

use crate::theme::ThemeChoice;

/// Which renderer iced should use. Both are compiled in; the choice is applied
/// at start-up because the backend is picked when the window is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RendererChoice {
    /// Let iced decide: GPU where available, software as a fallback.
    #[default]
    Auto,
    /// Force the GPU-accelerated wgpu backend (Vulkan/Metal/DX12/GL).
    Gpu,
    /// Force the CPU renderer. Slower, but works over remote desktops and on
    /// machines with broken drivers.
    Software,
}

impl RendererChoice {
    pub const ALL: [RendererChoice; 3] =
        [RendererChoice::Auto, RendererChoice::Gpu, RendererChoice::Software];

    pub fn label(self) -> &'static str {
        match self {
            RendererChoice::Auto => "Automatic",
            RendererChoice::Gpu => "GPU accelerated",
            RendererChoice::Software => "Software",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            RendererChoice::Auto => "Use the GPU when it works, fall back to software.",
            RendererChoice::Gpu => "Force hardware acceleration through wgpu.",
            RendererChoice::Software => "Force CPU rendering — safest over remote sessions.",
        }
    }

    /// The value iced expects in `ICED_BACKEND`.
    pub fn backend_env(self) -> Option<&'static str> {
        match self {
            RendererChoice::Auto => None,
            RendererChoice::Gpu => Some("wgpu"),
            RendererChoice::Software => Some("tiny-skia"),
        }
    }
}

impl std::fmt::Display for RendererChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A short comment at the top of the settings file, so anyone opening it in an
/// editor knows what they are looking at.
const SETTINGS_HEADER: &str = "# LimeDAW settings\n# Edited by the app; hand edits are picked up on the next start.\n\n";

/// Everything the app remembers between runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeChoice,
    pub renderer: RendererChoice,
    /// Set once the first-run wizard has been dismissed.
    pub setup_complete: bool,
    /// Serve the MCP server on the unix socket at start-up.
    pub mcp_enabled: bool,
    /// Snap resolution in beats for the piano roll and playlist.
    pub grid_snap: f64,
    pub metronome: bool,
    /// Interface scale, applied to the whole window.
    pub ui_scale: f32,
    /// Keep the browser panel open.
    pub show_browser: bool,
    /// Keep the instrument panel open.
    pub show_instrument_panel: bool,
    /// Default velocity for notes drawn in the piano roll.
    pub default_velocity: f32,
    /// The soundfont to load at start-up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soundfont: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_project: Option<String>,
    /// Recently opened project paths, most recent first.
    pub recent_projects: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::System,
            renderer: RendererChoice::Auto,
            setup_complete: false,
            soundfont: None,
            last_project: None,
            mcp_enabled: true,
            grid_snap: 0.25,
            metronome: false,
            ui_scale: 1.0,
            show_browser: true,
            show_instrument_panel: true,
            default_velocity: 0.78,
            recent_projects: Vec::new(),
        }
    }
}

impl Settings {
    pub fn sanitise(&mut self) {
        self.ui_scale = self.ui_scale.clamp(0.6, 2.5);
        self.default_velocity = self.default_velocity.clamp(0.05, 1.0);
        if !(1.0 / 64.0..=8.0).contains(&self.grid_snap) {
            self.grid_snap = 0.25;
        }
        self.recent_projects.truncate(12);
    }

    pub fn remember_project(&mut self, path: &str) {
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.to_string());
        self.recent_projects.truncate(12);
        self.last_project = Some(path.to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn load() -> Self {
        let path = crate::paths::settings_file();
        let mut settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| toml::from_str::<Settings>(&raw).ok())
            .or_else(|| {
                // Migrate the JSON file older builds wrote into the config dir.
                let legacy = crate::paths::legacy_settings_file();
                let parsed = std::fs::read_to_string(&legacy)
                    .ok()
                    .and_then(|raw| serde_json::from_str::<Settings>(&raw).ok());
                if parsed.is_some() {
                    let _ = std::fs::remove_file(&legacy);
                }
                parsed
            })
            .unwrap_or_default();
        settings.sanitise();
        settings
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&self) -> anyhow::Result<()> {
        let path = crate::paths::settings_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)?;
        std::fs::write(&path, format!("{SETTINGS_HEADER}{body}"))?;
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    pub fn load() -> Self {
        let mut settings = web_storage()
            .and_then(|s| s.get_item("limedaw.settings").ok().flatten())
            .and_then(|raw| toml::from_str::<Settings>(&raw).ok())
            .unwrap_or_default();
        settings.sanitise();
        settings
    }

    #[cfg(target_arch = "wasm32")]
    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(storage) = web_storage() {
            let raw = toml::to_string_pretty(self)?;
            let _ = storage.set_item("limedaw.settings", &raw);
        }
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
fn web_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}
