//! App preferences: load/save `Settings` as JSON at
//! `%APPDATA%\Throttle\settings.json`.
//!
//! A missing file means defaults; parse errors are logged and also fall back to
//! defaults, so a corrupt file never stops the app from starting. Launch at
//! startup is not stored here: the scheduled task's existence is the source of
//! truth (see `autostart`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::appearance::Appearance;
use crate::backend::rules::config_dir;
use crate::theme::{self, CustomTheme};

/// User preferences for window, tray and theme. The tray flags default to
/// `false` and the theme to `theme::DEFAULT_ID`, so an upgrade keeps the old
/// "taskbar window, X quits, stock look" behaviour.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Minimising the window hides it to the tray instead of the taskbar.
    pub minimize_to_tray: bool,
    /// The window's X button hides it to the tray; File > Exit still quits.
    pub close_to_tray: bool,
    /// Start hidden in the tray when launched by hand.
    pub start_minimized: bool,
    /// Colour theme id (see `theme`). Kept as stored even if unknown, so a
    /// newer build's palette survives a downgrade; `theme::apply` falls back.
    pub theme: String,
    /// Corner rounding, density, scale, fonts and accent overrides.
    pub appearance: Appearance,
    /// Table columns, plot style, always on top and compact mode.
    pub view: View,
    /// Sort order and column widths of the process table.
    pub table: Table,
    /// Sizes of the resizable panels.
    pub panels: Panels,
    /// Palettes imported from colour theme files.
    pub custom_themes: Vec<CustomTheme>,
}

/// The process table's sort key. `Down` is the fallback for an unknown
/// value in the file, so it must stay the last variant (`serde(other)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortColumn {
    Name,
    Flows,
    Up,
    DownTotal,
    UpTotal,
    #[serde(other)]
    Down,
}

/// Panel sizes in points, as last dragged. `None` is the built in default.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Panels {
    /// Height of the header plot panel.
    pub plot_height: Option<f32>,
    /// Width of the detail panel on the right.
    pub detail_width: Option<f32>,
}

/// Process table state kept across restarts. The defaults are today's
/// start-up state: sorted by download rate, widest first, columns auto sized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Table {
    /// Column the rows are sorted by.
    pub sort: SortColumn,
    /// Sort largest (or Z) first.
    pub descending: bool,
    /// Column widths in points, keyed by column id, as last seen on screen.
    /// Only used when every visible column has one (see `gui`).
    pub widths: BTreeMap<String, f32>,
    /// The Active chip: only list processes moving data right now.
    pub active_only: bool,
}

impl Default for Table {
    fn default() -> Self {
        Self {
            sort: SortColumn::Down,
            descending: true,
            widths: BTreeMap::new(),
            active_only: false,
        }
    }
}

/// Layout preferences from the View menu. Every default is today's look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct View {
    /// Show the Flows column.
    pub show_flows: bool,
    /// Show the Down column.
    pub show_down: bool,
    /// Show the Up column.
    pub show_up: bool,
    /// Show the Total down column.
    pub show_down_total: bool,
    /// Show the Total up column.
    pub show_up_total: bool,
    /// Show the Rule column.
    pub show_rule: bool,
    /// Fill the area under the plot lines.
    pub plot_fill: bool,
    /// Plot line width in points.
    pub plot_line_width: f32,
    /// Header plot history in seconds (one sample per second).
    pub history_secs: usize,
    /// Keep the window above other windows.
    pub always_on_top: bool,
    /// Hide the header plot.
    pub compact: bool,
    /// Show the filter box above the process table.
    pub show_filter: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            show_flows: true,
            show_down: true,
            show_up: true,
            show_down_total: true,
            show_up_total: true,
            show_rule: true,
            plot_fill: false,
            plot_line_width: 1.0,
            history_secs: 120,
            always_on_top: false,
            compact: false,
            show_filter: true,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            minimize_to_tray: false,
            close_to_tray: false,
            start_minimized: false,
            theme: theme::DEFAULT_ID.to_string(),
            appearance: Appearance::default(),
            view: View::default(),
            table: Table::default(),
            panels: Panels::default(),
            custom_themes: Vec::new(),
        }
    }
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
    // Editors and shells often write a UTF-8 byte order mark; it is not JSON.
    serde_json::from_str(data.trim_start_matches('\u{feff}')).unwrap_or_else(|e| {
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
            theme: "dark".to_string(),
            ..Settings::default()
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

    #[test]
    fn missing_theme_defaults_to_system() {
        let back: Settings =
            serde_json::from_str(r#"{"minimize_to_tray": true, "close_to_tray": false}"#).unwrap();
        assert_eq!(back.theme, theme::DEFAULT_ID);
        assert!(back.minimize_to_tray);
    }

    #[test]
    fn unknown_theme_is_kept_as_stored() {
        let back: Settings = serde_json::from_str(r#"{"theme": "no-such-theme"}"#).unwrap();
        assert_eq!(back.theme, "no-such-theme");
    }

    /// Francesco's live file: tray flags on, a palette picked. A partial or
    /// odd `appearance` object must never reset those.
    #[test]
    fn bad_nested_values_keep_other_settings() {
        let json = r#"{
            "minimize_to_tray": true, "close_to_tray": true, "theme": "dark-plus",
            "appearance": {"corners": "weird", "scale": 1.25},
            "view": {"show_flows": false},
            "custom_themes": [{"id": "x", "label": "X", "bg": "oops"}]
        }"#;
        let back: Settings = serde_json::from_str(json).unwrap();
        assert!(back.minimize_to_tray && back.close_to_tray);
        assert_eq!(back.theme, "dark-plus");
        assert_eq!(back.appearance.scale, 1.25);
        assert!(!back.view.show_flows);
        assert!(back.view.show_down);
        assert_eq!(back.custom_themes.len(), 1);
    }

    #[test]
    fn custom_themes_round_trip_through_json() {
        let palette = theme::builtin().next().expect("bundled palettes exist");
        let mut custom = CustomTheme::from(palette);
        custom.id = "mine".to_string();
        custom.label = "Mine".to_string();
        let settings = Settings {
            theme: "mine".to_string(),
            custom_themes: vec![custom],
            ..Settings::default()
        };
        let json = serde_json::to_string_pretty(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(settings, back);
        let restored = back.custom_themes[0].to_palette();
        assert_eq!(restored.id, "mine");
        assert_eq!(restored.source, theme::Source::Custom);
        assert_eq!(restored.accents, palette.accents);
        assert_eq!(restored.bg, palette.bg);
    }

    #[test]
    fn view_defaults_show_everything() {
        let v = View::default();
        assert!(v.show_flows && v.show_down && v.show_up);
        assert!(v.show_down_total && v.show_up_total && v.show_rule);
        assert!(!v.plot_fill && !v.always_on_top && !v.compact);
        assert!(v.show_filter);
        assert_eq!(v.plot_line_width, 1.0);
        assert_eq!(v.history_secs, 120);
        // An empty object is the same as the defaults.
        let back: View = serde_json::from_str("{}").unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn byte_order_mark_is_ignored() {
        let json = "\u{feff}{\"close_to_tray\": true}";
        let back: Settings = serde_json::from_str(json.trim_start_matches('\u{feff}')).unwrap();
        assert!(back.close_to_tray);
    }

    #[test]
    fn table_state_round_trips_and_tolerates_unknown_sort() {
        let mut table = Table {
            sort: SortColumn::UpTotal,
            descending: false,
            ..Table::default()
        };
        table.widths.insert("name".to_string(), 240.5);
        let json = serde_json::to_string(&table).unwrap();
        assert!(json.contains("\"up_total\""));
        let back: Table = serde_json::from_str(&json).unwrap();
        assert_eq!(back, table);

        // A sort key from a newer build falls back to the default, and the
        // rest of the settings survive.
        let back: Settings =
            serde_json::from_str(r#"{"theme": "nord", "table": {"sort": "latency"}}"#).unwrap();
        assert_eq!(back.table.sort, SortColumn::Down);
        assert_eq!(back.theme, "nord");
        assert!(back.table.descending);
    }
}
