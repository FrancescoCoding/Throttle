//! Import a colour theme JSON file (the editor's `color-theme` format) as a
//! `Palette`, using the same key mapping as the bundled data files.
//!
//! The file is untrusted data: it is size capped, parsed with serde_json and
//! never executed. An `include` entry is resolved relative to the file and
//! silently skipped when the included file is missing; missing keys fall
//! back to the workbench defaults for the theme's `type` (`dark`, `light`,
//! `hc` or `hcLight`), so an import never fails over an absent colour.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use anyhow::Context as _;
use eframe::egui::Color32;
use serde_json::Value;

use super::{Accents, Palette, Source, data_editor};

/// Largest theme file we are willing to read, in bytes.
pub const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024;

/// How many `include` hops are followed before the chain is cut off.
const MAX_INCLUDE_DEPTH: u32 = 3;

/// Minimum WCAG contrast of an accent against the background.
const MIN_ACCENT_CONTRAST: f32 = 3.0;

/// Parse `path` into a palette with `Source::Custom`. The id is derived
/// from the theme's `name` (or the file stem), lowercased, with runs of
/// non-alphanumerics replaced by `-` and prefixed with `custom-`.
pub fn import(path: &Path) -> anyhow::Result<Palette> {
    let top = read_json(path)?;
    let theme = load(path, &top, 0)?;

    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let label = top
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or(stem);
    let label = if label.is_empty() {
        "Imported theme".to_string()
    } else {
        label
    };

    Ok(theme.to_palette(format!("custom-{}", slug(&label)), label))
}

/// Read and parse one theme file, enforcing the size cap. Comments and
/// trailing commas are allowed, as the editor's own theme files use them.
fn read_json(path: &Path) -> anyhow::Result<Value> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("cannot read {}", path.display()))?
        .len();
    if size > MAX_FILE_SIZE {
        anyhow::bail!(
            "{} is {size} bytes; theme files larger than {MAX_FILE_SIZE} bytes are not imported",
            path.display()
        );
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let text = text.trim_start_matches('\u{feff}');
    let value: Value = serde_json::from_str(&strip_jsonc(text))
        .with_context(|| format!("{} is not a colour theme JSON file", path.display()))?;
    if !value.is_object() {
        anyhow::bail!("{} is not a colour theme JSON file", path.display());
    }
    Ok(value)
}

/// Colours and token rules gathered from a theme file and its includes.
#[derive(Default)]
struct Theme {
    /// Workbench colours; the including file's keys win.
    colors: HashMap<String, String>,
    /// `(scopes, foreground)` token rules, the including file's first.
    tokens: Vec<(Vec<String>, String)>,
    /// The `type` entry, from the nearest file that has one.
    kind: Option<String>,
}

/// Gather `value` (read from `path`) and its include chain. The included
/// file is loaded first so that this file's colours override its colours
/// and this file's token rules are searched before its rules.
fn load(path: &Path, value: &Value, depth: u32) -> anyhow::Result<Theme> {
    let mut theme = Theme::default();
    if let Some(include) = value.get("include").and_then(Value::as_str)
        && depth < MAX_INCLUDE_DEPTH
        && Path::new(include).is_relative()
    {
        let included = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(include);
        if included.is_file() {
            let inner = read_json(&included)?;
            theme = load(&included, &inner, depth + 1)?;
        }
    }

    if let Some(colors) = value.get("colors").and_then(Value::as_object) {
        for (key, v) in colors {
            if let Some(s) = v.as_str() {
                theme.colors.insert(key.clone(), s.to_string());
            }
        }
    }
    let mut tokens = Vec::new();
    if let Some(rules) = value.get("tokenColors").and_then(Value::as_array) {
        for rule in rules {
            let fg = rule
                .get("settings")
                .and_then(|s| s.get("foreground"))
                .and_then(Value::as_str);
            let Some(fg) = fg else { continue };
            let scopes = match rule.get("scope") {
                Some(Value::String(s)) => split_scopes(s),
                Some(Value::Array(list)) => list
                    .iter()
                    .filter_map(Value::as_str)
                    .flat_map(split_scopes)
                    .collect(),
                _ => continue,
            };
            tokens.push((scopes, fg.to_string()));
        }
    }
    tokens.append(&mut theme.tokens);
    theme.tokens = tokens;
    if let Some(kind) = value.get("type").and_then(Value::as_str) {
        theme.kind = Some(kind.to_string());
    }
    Ok(theme)
}

/// Split a comma separated scope selector into trimmed scopes.
fn split_scopes(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

impl Theme {
    /// Map the gathered keys onto a palette, filling gaps from the stock
    /// dark or light editor palette.
    fn to_palette(&self, id: String, label: String) -> Palette {
        let dark = match self.kind.as_deref() {
            Some("dark" | "hc") => true,
            Some("light" | "hcLight") => false,
            _ => self
                .colors
                .get("editor.background")
                .and_then(|s| parse_rgba(s))
                .is_none_or(|[r, g, b, _]| luminance(Color32::from_rgb(r, g, b)) < 0.5),
        };
        let base = base_palette(dark);

        let bg = self
            .colors
            .get("editor.background")
            .and_then(|s| parse_rgba(s))
            .filter(|c| c[3] > 0)
            .map_or(base.bg, |c| over(c, base.bg));
        // The first of `keys` that is set and not fully transparent,
        // composited onto the background.
        let pick = |keys: &[&str]| {
            keys.iter().find_map(|k| {
                let c = parse_rgba(self.colors.get(*k)?)?;
                (c[3] > 0).then(|| over(c, bg))
            })
        };
        let token = |matches: &dyn Fn(&str) -> bool| {
            self.tokens.iter().find_map(|(scopes, fg)| {
                if !scopes.iter().any(|s| matches(s)) {
                    return None;
                }
                let c = parse_rgba(fg)?;
                (c[3] > 0).then(|| over(c, bg))
            })
        };
        let scope_is = |name: &'static str| {
            move |s: &str| {
                s == name
                    || s.strip_prefix(name)
                        .is_some_and(|rest| rest.starts_with('.'))
            }
        };

        let fg = pick(&["editor.foreground", "foreground"]).unwrap_or(base.fg);
        let (contrast_text, hover_bg, hover_fg) = if dark {
            (
                Color32::WHITE,
                pick(&["button.hoverBackground", "button.background"]).unwrap_or(base.hover_bg),
                pick(&["button.foreground"]).unwrap_or(base.hover_fg),
            )
        } else {
            (
                Color32::BLACK,
                pick(&["list.hoverBackground"]).unwrap_or(base.hover_bg),
                Color32::BLACK,
            )
        };

        let down = pick(&["editorGutter.addedBackground", "terminal.ansiGreen"])
            .or_else(|| token(&|s: &str| s.contains("string")))
            .unwrap_or(base.accents.down);
        let up = pick(&["textLink.foreground", "terminal.ansiBlue"])
            .or_else(|| token(&scope_is("keyword")))
            .unwrap_or(base.accents.up);
        let block = pick(&["errorForeground", "terminal.ansiRed"])
            .or_else(|| token(&scope_is("invalid")))
            .unwrap_or(base.accents.block);

        Palette {
            id: Cow::Owned(id),
            label: Cow::Owned(label),
            source: Source::Custom,
            dark,
            accents: Accents {
                down: readable(down, bg, dark),
                up: readable(up, bg, dark),
                block: readable(block, bg, dark),
            },
            bg,
            faint_bg: pick(&["sideBar.background"]).unwrap_or(base.faint_bg),
            input_bg: pick(&["input.background"]).unwrap_or(base.input_bg),
            widget_bg: pick(&[
                "button.secondaryBackground",
                "checkbox.background",
                "dropdown.background",
            ])
            .unwrap_or(base.widget_bg),
            border: pick(&["panel.border", "widget.border"]).unwrap_or(base.border),
            input_border: pick(&["input.border", "checkbox.border"]).unwrap_or(base.input_border),
            hover_bg,
            hover_fg,
            focus: pick(&["focusBorder"]).unwrap_or(base.focus),
            selection: pick(&[
                "list.activeSelectionBackground",
                "editor.selectionBackground",
            ])
            .unwrap_or(base.selection),
            selection_fg: pick(&["list.activeSelectionForeground"]).unwrap_or(contrast_text),
            fg,
            weak_fg: pick(&["descriptionForeground"]).unwrap_or_else(|| blend(fg, bg, 0.7)),
            link: pick(&["textLink.foreground"]).unwrap_or(base.link),
            error: pick(&["errorForeground"]).unwrap_or(base.error),
        }
    }
}

/// The bundled palette whose values stand in for missing keys: Dark+ or
/// Light+, whose unset keys hold the workbench defaults.
fn base_palette(dark: bool) -> &'static Palette {
    let id = if dark { "dark-plus" } else { "light-plus" };
    data_editor::PALETTES
        .iter()
        .find(|p| p.id == id)
        .or_else(|| data_editor::PALETTES.iter().find(|p| p.dark == dark))
        .expect("bundled dark and light palettes exist")
}

/// Parse `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` into `[r, g, b, a]`.
fn parse_rgba(s: &str) -> Option<[u8; 4]> {
    let hex = s.trim().strip_prefix('#')?;
    if !hex.is_ascii() {
        return None;
    }
    let nibble = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok().map(|v| v * 17);
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => Some([nibble(0)?, nibble(1)?, nibble(2)?, 255]),
        4 => Some([nibble(0)?, nibble(1)?, nibble(2)?, nibble(3)?]),
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

/// Composite a possibly translucent colour onto an opaque background.
fn over([r, g, b, a]: [u8; 4], bg: Color32) -> Color32 {
    blend(Color32::from_rgb(r, g, b), bg, f32::from(a) / 255.0)
}

/// `t` of `fg` mixed into `bg` (1.0 is pure `fg`).
fn blend(fg: Color32, bg: Color32, t: f32) -> Color32 {
    let mix = |f: u8, b: u8| (f32::from(b) + (f32::from(f) - f32::from(b)) * t).round() as u8;
    Color32::from_rgb(
        mix(fg.r(), bg.r()),
        mix(fg.g(), bg.g()),
        mix(fg.b(), bg.b()),
    )
}

/// WCAG relative luminance of an opaque colour.
fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// WCAG contrast ratio between two opaque colours.
fn contrast(a: Color32, b: Color32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Lighten (dark themes) or darken (light themes) `c` in 10% steps until it
/// reaches `MIN_ACCENT_CONTRAST` on `bg`, or white or black is reached.
fn readable(mut c: Color32, bg: Color32, dark: bool) -> Color32 {
    let target = if dark { Color32::WHITE } else { Color32::BLACK };
    for _ in 0..30 {
        if contrast(c, bg) >= MIN_ACCENT_CONTRAST || c == target {
            break;
        }
        let next = blend(target, c, 0.1);
        // Rounding can stall a channel one step short of the target.
        c = if next == c { target } else { next };
    }
    c
}

/// Lowercase ASCII slug: runs of anything else become a single `-`.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "theme".to_string()
    } else {
        out.to_string()
    }
}

/// Remove `//` and `/* */` comments and trailing commas outside strings,
/// so the JSON-with-comments theme files parse with serde_json.
fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut no_comments = Vec::with_capacity(chars.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            no_comments.push(ch);
            if ch == '\\' {
                if let Some(&next) = chars.get(i + 1) {
                    no_comments.push(next);
                    i += 1;
                }
            } else if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match (ch, chars.get(i + 1)) {
            ('"', _) => {
                in_string = true;
                no_comments.push(ch);
                i += 1;
            }
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
                no_comments.push(' ');
            }
            _ => {
                no_comments.push(ch);
                i += 1;
            }
        }
    }

    let mut out = String::with_capacity(no_comments.len());
    let mut in_string = false;
    let mut escaped = false;
    for (i, &ch) in no_comments.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == ',' {
            let next = no_comments[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}' | ']')) {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::to_hex;
    use std::path::PathBuf;

    /// A fresh directory per test, since tests run in parallel.
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("throttle-import-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const DARK: &str = r##"{
        // Comments and trailing commas appear in real theme files.
        "name": "My Theme (v2)!",
        "type": "dark",
        "include": "./this-file-does-not-exist.json",
        "colors": {
            "editor.background": "#101010",
            "editor.foreground": "#e0e0e0",
            "button.background": "#0a64a0",
            "button.secondaryBackground": "#00000000",
            "dropdown.background": "#303030",
            "list.activeSelectionBackground": "#ffffff80", /* half white */
            "focusBorder": 42,
        },
        "tokenColors": [
            { "settings": { "foreground": "#abcdef" } },
            { "scope": "comment", "settings": { "foreground": "#6a9955" } },
            { "scope": ["string.quoted", "string"], "settings": { "foreground": "#ce9178" } },
            { "scope": "keyword.control, storage", "settings": { "foreground": "#569cd6" } },
            { "scope": "keywordish", "settings": { "foreground": "#ff00ff" } },
            { "scope": "invalid", "settings": { "foreground": "#101010" } },
        ],
    }"##;

    const LIGHT: &str = r##"{
        "colors": {
            "editor.background": "#fafafa",
            "editor.foreground": "#333333",
            "textLink.foreground": "#0066cc",
            "terminal.ansiGreen": "#e0ffe0"
        }
    }"##;

    #[test]
    fn dark_theme_maps_keys_tokens_and_defaults() {
        let dir = temp_dir("dark");
        let path = dir.join("mine.json");
        std::fs::write(&path, DARK).unwrap();
        let p = import(&path).unwrap();
        let base = base_palette(true);

        assert_eq!(p.id, "custom-my-theme-v2");
        assert_eq!(p.label, "My Theme (v2)!");
        assert_eq!(p.source, Source::Custom);
        assert!(p.dark);
        assert_eq!(to_hex(p.bg), "#101010");
        assert_eq!(to_hex(p.fg), "#e0e0e0");
        // No hoverBackground: button.background stands in.
        assert_eq!(to_hex(p.hover_bg), "#0a64a0");
        // A transparent secondary button is skipped for the next key.
        assert_eq!(to_hex(p.widget_bg), "#303030");
        // Translucent colours are composited onto the background.
        assert_eq!(to_hex(p.selection), "#888888");
        assert_eq!(p.selection_fg, Color32::WHITE);
        // Missing or malformed keys use the workbench defaults.
        assert_eq!(p.faint_bg, base.faint_bg);
        assert_eq!(p.focus, base.focus);
        assert_eq!(p.link, base.link);
        assert_eq!(p.hover_fg, base.hover_fg);
        assert_eq!(p.weak_fg, blend(p.fg, p.bg, 0.7));
        // Accents from the token rules.
        assert_eq!(to_hex(p.accents.down), "#ce9178");
        assert_eq!(to_hex(p.accents.up), "#569cd6");
        // The invalid token equals the background, so it gets lightened.
        assert!(contrast(p.accents.block, p.bg) >= MIN_ACCENT_CONTRAST);
        assert_ne!(p.accents.block, p.bg);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn light_theme_without_type_or_name() {
        let dir = temp_dir("light");
        let path = dir.join("Solar Flare.json");
        std::fs::write(&path, LIGHT).unwrap();
        let p = import(&path).unwrap();
        let base = base_palette(false);

        assert_eq!(p.id, "custom-solar-flare");
        assert_eq!(p.label, "Solar Flare");
        assert!(!p.dark, "luminance of editor.background decides");
        assert_eq!(to_hex(p.bg), "#fafafa");
        assert_eq!(to_hex(p.link), "#0066cc");
        assert_eq!(p.hover_fg, Color32::BLACK);
        assert_eq!(p.selection_fg, Color32::BLACK);
        assert_eq!(p.hover_bg, base.hover_bg);
        assert_eq!(p.faint_bg, base.faint_bg);
        assert_eq!(p.error, base.error);
        assert_eq!(to_hex(p.accents.up), "#0066cc");
        assert_eq!(p.accents.block, base.accents.block);
        // The pale green is darkened until it reads on the background.
        assert_ne!(to_hex(p.accents.down), "#e0ffe0");
        assert!(contrast(p.accents.down, p.bg) >= MIN_ACCENT_CONTRAST);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn including_file_wins_over_included() {
        let dir = temp_dir("include");
        std::fs::write(
            dir.join("base.json"),
            r##"{ "type": "light", "colors": { "editor.background": "#202020", "focusBorder": "#123456" },
                 "tokenColors": [{ "scope": "string", "settings": { "foreground": "#00aa00" } }] }"##,
        )
        .unwrap();
        let path = dir.join("top.json");
        std::fs::write(
            &path,
            r##"{ "name": "Top", "include": "base.json", "colors": { "editor.background": "#ffffff" },
                 "tokenColors": [{ "scope": "string.quoted", "settings": { "foreground": "#008800" } }] }"##,
        )
        .unwrap();
        let p = import(&path).unwrap();
        assert!(!p.dark, "type comes from the included file");
        assert_eq!(to_hex(p.bg), "#ffffff");
        assert_eq!(to_hex(p.focus), "#123456");
        assert_eq!(to_hex(p.accents.down), "#008800");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn oversized_or_invalid_files_error() {
        let dir = temp_dir("bad");
        let big = dir.join("big.json");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(MAX_FILE_SIZE + 1)
            .unwrap();
        assert!(import(&big).is_err());

        let junk = dir.join("junk.json");
        std::fs::write(&junk, "this is not json").unwrap();
        assert!(import(&junk).is_err());

        let array = dir.join("array.json");
        std::fs::write(&array, "[1, 2, 3]").unwrap();
        assert!(import(&array).is_err());

        assert!(import(&dir.join("missing.json")).is_err());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn slugs_and_hex_forms() {
        assert_eq!(slug("My Theme (v2)!"), "my-theme-v2");
        assert_eq!(slug("  --  "), "theme");
        assert_eq!(slug("Ros\u{e9} Pine"), "ros-pine");
        assert_eq!(parse_rgba("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(parse_rgba("#0008"), Some([0, 0, 0, 0x88]));
        assert_eq!(parse_rgba("#12345678"), Some([0x12, 0x34, 0x56, 0x78]));
        assert_eq!(parse_rgba("red"), None);
        assert_eq!(parse_rgba("#12345"), None);
    }

    #[test]
    fn jsonc_keeps_strings_intact() {
        let src = r#"{ "a": "http://x/*y*/", "b": [1, 2,], // tail
            "c": "q\"//", }"#;
        let v: Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["a"], "http://x/*y*/");
        assert_eq!(v["b"], serde_json::json!([1, 2]));
        assert_eq!(v["c"], "q\"//");
    }
}
