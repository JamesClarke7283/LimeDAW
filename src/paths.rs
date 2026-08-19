//! Where LimeDAW keeps its files.
//!
//! Config and data follow the platform conventions (`directories` is the Rust
//! equivalent of Python's platformdirs), while projects deliberately live in a
//! plain `LimeProjects` folder at the root of the user's home directory so
//! they are easy to find, back up and share.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::{Path, PathBuf};

    use directories::{ProjectDirs, UserDirs};

    pub const APP_NAME: &str = "LimeDAW";
    pub const PROJECTS_FOLDER: &str = "LimeProjects";

    fn project_dirs() -> Option<ProjectDirs> {
        // On Linux this resolves to ~/.config/LimeDAW and ~/.local/share/LimeDAW,
        // which is exactly where the MCP socket is expected to appear.
        ProjectDirs::from("", "", APP_NAME)
    }

    /// `~/.config/LimeDAW` (or the platform equivalent).
    pub fn config_dir() -> PathBuf {
        project_dirs()
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".limedaw"))
    }

    /// `~/.local/share/LimeDAW` (or the platform equivalent).
    pub fn data_dir() -> PathBuf {
        project_dirs()
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".limedaw/data"))
    }

    /// `~/.cache/LimeDAW` (or the platform equivalent).
    pub fn cache_dir() -> PathBuf {
        project_dirs()
            .map(|d| d.cache_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".limedaw/cache"))
    }

    pub fn home_dir() -> PathBuf {
        UserDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// `~/LimeProjects`, created on demand.
    pub fn projects_dir() -> PathBuf {
        home_dir().join(PROJECTS_FOLDER)
    }

    /// Where rendered audio lands by default.
    pub fn renders_dir() -> PathBuf {
        projects_dir().join("Renders")
    }

    /// Where the user's own soundfonts are kept if they import one.
    pub fn soundfonts_dir() -> PathBuf {
        data_dir().join("soundfonts")
    }

    /// Where sample packs are kept.
    pub fn samples_dir() -> PathBuf {
        projects_dir().join("Samples")
    }

    /// The settings file. It lives beside the MCP socket in the data directory
    /// so everything LimeDAW owns at runtime sits in one place.
    pub fn settings_file() -> PathBuf {
        data_dir().join("settings.toml")
    }

    /// Settings written by older builds, migrated on first load.
    pub fn legacy_settings_file() -> PathBuf {
        config_dir().join("settings.json")
    }

    /// The unix socket the MCP server listens on.
    pub fn mcp_socket() -> PathBuf {
        data_dir().join("mcp.sock")
    }

    /// The MCP server's log, useful when a client can't connect.
    pub fn mcp_log() -> PathBuf {
        data_dir().join("mcp.log")
    }

    /// Create every directory the app expects to exist.
    pub fn ensure_all() -> std::io::Result<()> {
        for dir in [
            config_dir(),
            data_dir(),
            cache_dir(),
            projects_dir(),
            renders_dir(),
            samples_dir(),
            soundfonts_dir(),
        ] {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Turn a path into something short enough to show in the UI.
    pub fn display(path: &Path) -> String {
        let home = home_dir();
        match path.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        }
    }

    /// Every `.sf2`/`.sf3` file in the places soundfonts usually live.
    pub fn discover_soundfonts() -> Vec<PathBuf> {
        let mut found = Vec::new();
        let candidates = [
            soundfonts_dir(),
            projects_dir().join("Soundfonts"),
            PathBuf::from("/usr/share/soundfonts"),
            PathBuf::from("/usr/share/sounds/sf2"),
            PathBuf::from("/usr/share/sounds/sf3"),
            PathBuf::from("/usr/local/share/soundfonts"),
            home_dir().join(".local/share/soundfonts"),
        ];
        for dir in candidates {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if ext == "sf2" || ext == "sf3" {
                    found.push(path);
                }
            }
        }
        found.sort();
        found.dedup();
        found
    }

    /// Every project file in `~/LimeProjects`.
    pub fn discover_projects() -> Vec<PathBuf> {
        let mut found = Vec::new();
        if let Ok(entries) = std::fs::read_dir(projects_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "lime").unwrap_or(false) {
                    found.push(path);
                }
            }
        }
        found.sort();
        found
    }

    /// Every readable audio file under `~/LimeProjects/Samples`.
    pub fn discover_samples() -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
            if depth > 4 {
                return;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out, depth + 1);
                } else if path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase() == "wav")
                    .unwrap_or(false)
                {
                    out.push(path);
                }
            }
        }
        let mut found = Vec::new();
        walk(&samples_dir(), &mut found, 0);
        found.sort();
        found
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::path::{Path, PathBuf};

    pub const APP_NAME: &str = "LimeDAW";
    pub const PROJECTS_FOLDER: &str = "LimeProjects";

    // In the browser there is no filesystem; these exist so the rest of the app
    // can talk about paths uniformly, and storage goes through `localStorage`.
    pub fn config_dir() -> PathBuf {
        PathBuf::from("/LimeDAW/config")
    }
    pub fn data_dir() -> PathBuf {
        PathBuf::from("/LimeDAW/data")
    }
    pub fn cache_dir() -> PathBuf {
        PathBuf::from("/LimeDAW/cache")
    }
    pub fn home_dir() -> PathBuf {
        PathBuf::from("/")
    }
    pub fn projects_dir() -> PathBuf {
        PathBuf::from("/LimeProjects")
    }
    pub fn renders_dir() -> PathBuf {
        projects_dir().join("Renders")
    }
    pub fn soundfonts_dir() -> PathBuf {
        data_dir().join("soundfonts")
    }
    pub fn samples_dir() -> PathBuf {
        projects_dir().join("Samples")
    }
    /// The settings file. It lives beside the MCP socket in the data directory
    /// so everything LimeDAW owns at runtime sits in one place.
    pub fn settings_file() -> PathBuf {
        data_dir().join("settings.toml")
    }

    /// Settings written by older builds, migrated on first load.
    pub fn legacy_settings_file() -> PathBuf {
        config_dir().join("settings.json")
    }
    pub fn mcp_socket() -> PathBuf {
        data_dir().join("mcp.sock")
    }
    pub fn mcp_log() -> PathBuf {
        data_dir().join("mcp.log")
    }
    pub fn ensure_all() -> std::io::Result<()> {
        Ok(())
    }
    pub fn display(path: &Path) -> String {
        path.display().to_string()
    }
    pub fn discover_soundfonts() -> Vec<PathBuf> {
        Vec::new()
    }
    pub fn discover_projects() -> Vec<PathBuf> {
        Vec::new()
    }
    pub fn discover_samples() -> Vec<PathBuf> {
        Vec::new()
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::*;
