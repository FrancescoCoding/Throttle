//! App preferences: load/save `Settings` as JSON at
//! `%APPDATA%\Throttle\settings.json`.
//!
//! A missing file means defaults; parse errors are logged and also fall back to
//! defaults, so a corrupt file never stops the app from starting. Launch at
//! startup is not stored here: the scheduled task's existence is the source of
//! truth (see `autostart`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::backend::rules::config_dir;

/// User preferences for window and tray behaviour. Every field defaults to
/// `false`, so an upgrade keeps the old "taskbar window, X quits" behaviour.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Minimising the window hides it to the tray instead of the taskbar.
    pub minimize_to_tray: bool,
    /// The window's X button hides it to the tray; File > Exit still quits.
    pub close_to_tray: bool,
    /// Start hidden in the tray when launched by hand.
    pub start_minimized: bool,
}

/// Full path to the settings file.
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// Load the persisted settings. Missing file or any error => defaults.
pub fn load() -> Settings {
    let path = settings_path();
    let data = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(e) => {
            tracing::warn!("failed to read {}: {e}", path.display());
            return Settings::default();
        }
    };
    serde_json::from_str(&data).unwrap_or_else(|e| {
        tracing::warn!("failed to parse {}: {e}; using defaults", path.display());
        Settings::default()
    })
}

/// Persist the settings as pretty JSON, creating the directory if needed.
pub fn save(settings: &Settings) -> anyhow::Result<()> {
    std::fs::create_dir_all(config_dir())?;
    let json = serde_json::to_string_pretty(settings)?;
    std::fs::write(settings_path(), json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_json() {
        let settings = Settings {
            minimize_to_tray: true,
            close_to_tray: false,
            start_minimized: true,
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(settings, back);
    }

    #[test]
    fn missing_fields_default_to_off() {
        let back: Settings = serde_json::from_str(r#"{"close_to_tray": true}"#).unwrap();
        assert_eq!(
            back,
            Settings {
                close_to_tray: true,
                ..Settings::default()
            }
        );
    }
}
