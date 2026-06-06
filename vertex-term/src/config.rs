use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub theme: String,
    pub font_size: f32,
    pub font_family: String,
    pub shell: String,
    pub scrollback_lines: usize,
    /// true = let the compositor/WM draw borders (SSD); false = draw our own titlebar (CSD)
    pub system_borders: bool,
    pub opacity: f32,
    pub cursor_blink: bool,
    pub enable_blur: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            font_size: 14.0,
            font_family: "monospace".into(),
            shell: "/usr/bin/bash".into(),
            scrollback_lines: 5000,
            system_borders: true,
            opacity: 1.0,
            cursor_blink: true,
            enable_blur: false,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        match Self::path().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(s) => toml::from_str(&s).unwrap_or_default(),
            None => Self::default(),
        }
    }

    pub fn save(&self) {
        if let Some(path) = Self::path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(s) = toml::to_string_pretty(self) {
                let _ = std::fs::write(path, s);
            }
        }
    }

    fn path() -> Option<PathBuf> {
        dirs_next::home_dir().map(|h| h.join(".vertex-term").join("config.toml"))
    }
}
