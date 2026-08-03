//! Cross-platform settings, persisted as a small TOML file.
//!
//! Replaces the macOS-only `UserDefaults`/`AppSettings.swift` with a plain
//! config file that works identically on every OS.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use vnkey_core::{Config as CoreConfig, InputMethod, TonePlacementMode};

/// Input method convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Telex,
    Vni,
}

/// Tone-mark placement strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Modern,
    Classic,
}

/// User-facing settings. Owned by the shell, mapped into [`CoreConfig`] for the
/// engine on every change.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Master on/off for Vietnamese typing.
    pub enabled: bool,
    /// Telex or VNI.
    pub method: Method,
    /// Modern (`hòa`) or Classic (`hoà`) tone placement.
    pub placement: Placement,
    /// Restore raw keystrokes for non-Vietnamese (e.g. English) words.
    pub auto_restore: bool,
    /// Download and install new releases automatically. Set false to go back to
    /// being notified only.
    pub auto_update: bool,
    /// Version that last ran. Compared against the running build at startup to
    /// notice that a self-update happened, so the user can be told.
    pub last_seen_version: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            method: Method::Telex,
            placement: Placement::Modern,
            auto_restore: true,
            auto_update: true,
            last_seen_version: None,
        }
    }
}

impl Settings {
    /// `~/.config/vnkey/config.toml` (or the OS equivalent).
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vnkey")
            .join("config.toml")
    }

    /// Load settings from disk, falling back to defaults on any error.
    pub fn load() -> Self {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist settings to disk. Errors are logged, never fatal.
    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match toml::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    eprintln!("[vnkey] failed to save settings to {}: {e}", path.display());
                }
            }
            Err(e) => eprintln!("[vnkey] failed to serialize settings: {e}"),
        }
    }

    /// Map the shell settings onto the engine's config type.
    pub fn to_core(&self) -> CoreConfig {
        CoreConfig {
            method: match self.method {
                Method::Telex => InputMethod::Telex,
                Method::Vni => InputMethod::Vni,
            },
            placement: match self.placement {
                Placement::Modern => TonePlacementMode::Modern,
                Placement::Classic => TonePlacementMode::Classic,
            },
            enabled: self.enabled,
            auto_restore: self.auto_restore,
        }
    }
}
