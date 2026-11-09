//! Colour themes for the GUI.
//!
//! The default (`system`) keeps egui's stock visuals and follows the OS light
//! or dark preference, so an existing install looks exactly as before. Named
//! palettes are opt-in via the Themes menu and persisted in `Settings::theme`.
//! Palettes come in three groups: the editor built-ins in `data_editor`, the
//! community themes in `data_community`, and palettes the user imported from
//! a colour theme JSON file (`import`), which are stored in `Settings` as
//! `CustomTheme`s.
//!
//! Bundled palette colours come from MIT licensed colour theme files; see
//! `THIRD_PARTY_NOTICES.md` for attribution.

mod data_community;
mod data_editor;
/// Importing colour theme JSON files as palettes.
pub mod import;

use std::borrow::Cow;

use eframe::egui::{self, Color32, Theme, ThemePreference, Visuals};
use serde::{Deserialize, Serialize};

/// Theme id used when `Settings::theme` is missing or unknown.
pub const DEFAULT_ID: &str = "system";

/// Built-in entries that are not palettes: `(id, menu label)`.
/// `system` follows the OS, `light` and `dark` pin egui's stock visuals.
pub const BUILTIN: [(&str, &str); 3] = [
    (DEFAULT_ID, "Default (follow system)"),
    ("light", "Light"),
    ("dark", "Dark"),
];

/// Accent colours used by the plots and the table, per theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accents {
    /// Download traffic.
    pub down: Color32,
    /// Upload traffic.
    pub up: Color32,
    /// Blocked traffic and error text.
    pub block: Color32,
}

impl Accents {
    /// The accents of the stock look (Material green, blue and red).
    pub const DEFAULT: Accents = Accents {
        down: Color32::from_rgb(0x4c, 0xaf, 0x50),
        up: Color32::from_rgb(0x21, 0x96, 0xf3),
        block: Color32::from_rgb(0xe5, 0x39, 0x35),
    };
}

/// Where a palette comes from; decides which Themes submenu lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Shipped with the editor the palettes were taken from (`data_editor`).
    Editor,
    /// A popular third party theme (`data_community`).
    Community,
    /// Imported by the user from a colour theme JSON file.
    Custom,
}

/// A named colour palette. Each colour names the workbench key it was taken
/// from; see the comments above the palette data for keys a theme lacks.
/// The field set is a frozen contract shared by the data files, the importer
/// and `CustomTheme`.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    /// Stable id stored in `settings.json`; lowercase, no spaces.
    pub id: Cow<'static, str>,
    /// Label shown in the Themes menu.
    pub label: Cow<'static, str>,
    /// Which Themes submenu lists the palette.
    pub source: Source,
    /// Whether the palette is dark (light text on a dark background).
    pub dark: bool,
    /// Plot and table accents, picked from the theme's own colours.
    pub accents: Accents,
    /// `editor.background`: panel and window fill.
    pub bg: Color32,
    /// `sideBar.background`: faint fill (table stripes).
    pub faint_bg: Color32,
    /// `input.background`: text edits, plot and code backgrounds.
    pub input_bg: Color32,
    /// `button.secondaryBackground` or similar: idle buttons and checkboxes.
    pub widget_bg: Color32,
    /// `widget.border` or `panel.border`: separators and window outline.
    pub border: Color32,
    /// `input.border` or `checkbox.border`: outline of idle widgets.
    pub input_border: Color32,
    /// Fill of hovered widgets: `button.background` in dark palettes, a
    /// light selection tint in light ones.
    pub hover_bg: Color32,
    /// Text of hovered widgets. egui paints it on `hover_bg` (buttons) and on
    /// the bare panel (checkbox and radio labels), so it must read on both.
    pub hover_fg: Color32,
    /// `focusBorder`: outline of hovered and pressed widgets.
    pub focus: Color32,
    /// Selection fill (selected text, selected items, pressed widgets).
    pub selection: Color32,
    /// Text on `selection`; also the focused text edit outline.
    pub selection_fg: Color32,
    /// `editor.foreground`: normal text.
    pub fg: Color32,
    /// `descriptionForeground` or a blend of `fg` into `bg`: weak text.
    pub weak_fg: Color32,
    /// `textLink.foreground`: hyperlinks.
    pub link: Color32,
    /// `errorForeground`: error text.
    pub error: Color32,
}

/// `0xRRGGBB` to an opaque colour.
pub(crate) const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Three `0xRRGGBB` values to the down, up and block accents.
pub(crate) const fn accents(down: u32, up: u32, block: u32) -> Accents {
    Accents {
        down: rgb(down),
        up: rgb(up),
        block: rgb(block),
    }
}

/// Parse `#rrggbb`, `#rrggbbaa`, `rrggbb` or `rrggbbaa` (alpha ignored).
pub fn parse_hex(s: &str) -> Option<Color32> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 && s.len() != 8 {
        return None;
    }
    let v = u32::from_str_radix(&s[..6], 16).ok()?;
    if s.len() == 8 {
        u8::from_str_radix(&s[6..], 16).ok()?;
    }
    Some(rgb(v))
}

/// Format a colour as `#rrggbb` (alpha dropped).
pub fn to_hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// A user imported palette as stored in `settings.json`. Every colour is a
/// `#rrggbb` string; a missing or malformed one falls back to the matching
/// colour of the stock dark or light palette so an edited file never fails
/// to load.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomTheme {
    /// Stable id, as `Palette::id`.
    pub id: String,
    /// Menu label, as `Palette::label`.
    pub label: String,
    /// Whether the palette is dark, as `Palette::dark`.
    pub dark: bool,
    /// Download accent (`Accents::down`).
    pub accent_down: String,
    /// Upload accent (`Accents::up`).
    pub accent_up: String,
    /// Blocked accent (`Accents::block`).
    pub accent_block: String,
    /// Panel and window fill (`Palette::bg`).
    pub bg: String,
    /// Faint fill for table stripes (`Palette::faint_bg`).
    pub faint_bg: String,
    /// Text edit and plot background (`Palette::input_bg`).
    pub input_bg: String,
    /// Idle button and checkbox fill (`Palette::widget_bg`).
    pub widget_bg: String,
    /// Separators and window outline (`Palette::border`).
    pub border: String,
    /// Outline of idle widgets (`Palette::input_border`).
    pub input_border: String,
    /// Fill of hovered widgets (`Palette::hover_bg`).
    pub hover_bg: String,
    /// Text of hovered widgets (`Palette::hover_fg`).
    pub hover_fg: String,
    /// Outline of hovered and pressed widgets (`Palette::focus`).
    pub focus: String,
    /// Selection fill (`Palette::selection`).
    pub selection: String,
    /// Text on the selection fill (`Palette::selection_fg`).
    pub selection_fg: String,
    /// Normal text (`Palette::fg`).
    pub fg: String,
    /// Weak text (`Palette::weak_fg`).
    pub weak_fg: String,
    /// Hyperlinks (`Palette::link`).
    pub link: String,
    /// Error text (`Palette::error`).
    pub error: String,
}

impl From<&Palette> for CustomTheme {
    fn from(p: &Palette) -> Self {
        CustomTheme {
            id: p.id.to_string(),
            label: p.label.to_string(),
            dark: p.dark,
            accent_down: to_hex(p.accents.down),
            accent_up: to_hex(p.accents.up),
            accent_block: to_hex(p.accents.block),
            bg: to_hex(p.bg),
            faint_bg: to_hex(p.faint_bg),
            input_bg: to_hex(p.input_bg),
            widget_bg: to_hex(p.widget_bg),
            border: to_hex(p.border),
            input_border: to_hex(p.input_border),
            hover_bg: to_hex(p.hover_bg),
            hover_fg: to_hex(p.hover_fg),
            focus: to_hex(p.focus),
            selection: to_hex(p.selection),
            selection_fg: to_hex(p.selection_fg),
            fg: to_hex(p.fg),
            weak_fg: to_hex(p.weak_fg),
            link: to_hex(p.link),
            error: to_hex(p.error),
        }
    }
}

impl CustomTheme {
    /// Convert to a runtime palette with `Source::Custom`. Unparseable
    /// colours fall back to the first bundled dark or light editor palette.
    pub fn to_palette(&self) -> Palette {
        let base = data_editor::PALETTES
            .iter()
            .find(|p| p.dark == self.dark)
            .expect("bundled dark and light palettes exist");
        let c = |s: &str, fallback: Color32| parse_hex(s).unwrap_or(fallback);
        Palette {
            id: Cow::Owned(self.id.clone()),
            label: Cow::Owned(self.label.clone()),
            source: Source::Custom,
            dark: self.dark,
            accents: Accents {
                down: c(&self.accent_down, base.accents.down),
                up: c(&self.accent_up, base.accents.up),
                block: c(&self.accent_block, base.accents.block),
            },
            bg: c(&self.bg, base.bg),
            faint_bg: c(&self.faint_bg, base.faint_bg),
            input_bg: c(&self.input_bg, base.input_bg),
            widget_bg: c(&self.widget_bg, base.widget_bg),
            border: c(&self.border, base.border),
            input_border: c(&self.input_border, base.input_border),
            hover_bg: c(&self.hover_bg, base.hover_bg),
            hover_fg: c(&self.hover_fg, base.hover_fg),
            focus: c(&self.focus, base.focus),
            selection: c(&self.selection, base.selection),
            selection_fg: c(&self.selection_fg, base.selection_fg),
            fg: c(&self.fg, base.fg),
            weak_fg: c(&self.weak_fg, base.weak_fg),
            link: c(&self.link, base.link),
            error: c(&self.error, base.error),
        }
    }
}

/// All bundled palettes: editor built-ins first, then community themes.
pub fn builtin() -> impl Iterator<Item = &'static Palette> {
    data_editor::PALETTES
        .iter()
        .chain(data_community::PALETTES.iter())
}

/// Look up a palette by id among the bundled ones and `custom`.
pub fn find<'a>(id: &str, custom: &'a [Palette]) -> Option<&'a Palette> {
    builtin()
        .find(|p| p.id == id)
        .or_else(|| custom.iter().find(|p| p.id == id))
}

/// Apply the theme `id` to `ctx` and return its accents. Handles the
/// `BUILTIN` ids, every bundled palette id and the ids in `custom`; an
/// unknown id applies the default. Applying `system` must restore egui's
/// stock dark and light visuals, so switching away from a palette leaves
/// nothing behind. `appearance::apply` must run after this, since this
/// resets both stock visuals.
pub fn apply(ctx: &egui::Context, id: &str, custom: &[Palette]) -> Accents {
    // Start every switch from stock visuals for both modes, so whatever the
    // previous theme changed is undone before the new one is applied.
    ctx.set_visuals_of(Theme::Dark, Visuals::dark());
    ctx.set_visuals_of(Theme::Light, Visuals::light());

    match id {
        "light" => ctx.set_theme(Theme::Light),
        "dark" => ctx.set_theme(Theme::Dark),
        _ => match find(id, custom) {
            Some(palette) => {
                let theme = if palette.dark {
                    Theme::Dark
                } else {
                    Theme::Light
                };
                ctx.set_visuals_of(theme, visuals(palette));
                ctx.set_theme(theme);
                return palette.accents;
            }
            // `system` and unknown ids.
            None => ctx.set_theme(ThemePreference::System),
        },
    }
    Accents::DEFAULT
}

/// Build the egui visuals for `p` on top of the stock dark or light visuals.
/// Only colours change; stroke widths and corner radii stay stock.
pub(crate) fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    // Strong text (`RichText::strong`) uses the pressed widget's text colour.
    let strong = if p.dark {
        Color32::WHITE
    } else {
        Color32::BLACK
    };

    v.panel_fill = p.bg;
    v.window_fill = p.bg;
    v.window_stroke.color = p.border;
    v.faint_bg_color = p.faint_bg;
    v.extreme_bg_color = p.input_bg;
    v.text_edit_bg_color = Some(p.input_bg);
    v.code_bg_color = p.input_bg;
    v.hyperlink_color = p.link;
    v.error_fg_color = p.error;
    // Text colour comes from the widget states below; `override_text_color`
    // would also override the text on hovered and selected widgets.
    v.weak_text_color = Some(p.weak_fg);
    v.selection.bg_fill = p.selection;
    v.selection.stroke.color = p.selection_fg;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke.color = p.border;
    w.noninteractive.fg_stroke.color = p.fg;

    w.inactive.bg_fill = p.widget_bg;
    w.inactive.weak_bg_fill = p.widget_bg;
    w.inactive.bg_stroke = egui::Stroke::new(1.0, p.input_border);
    w.inactive.fg_stroke.color = p.fg;

    w.hovered.bg_fill = p.hover_bg;
    w.hovered.weak_bg_fill = p.hover_bg;
    w.hovered.bg_stroke.color = p.focus;
    w.hovered.fg_stroke.color = p.hover_fg;

    w.active.bg_fill = p.selection;
    w.active.weak_bg_fill = p.selection;
    w.active.bg_stroke.color = p.focus;
    w.active.fg_stroke.color = strong;

    w.open.bg_fill = p.bg;
    w.open.weak_bg_fill = p.widget_bg;
    w.open.bg_stroke.color = p.border;
    w.open.fg_stroke.color = p.fg;

    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_lowercase_without_spaces() {
        let ids: Vec<&str> = builtin().map(|p| p.id.as_ref()).collect();
        for (i, id) in ids.iter().enumerate() {
            assert!(!id.is_empty());
            assert_eq!(*id, id.to_lowercase(), "{id} is not lowercase");
            assert!(!id.contains(char::is_whitespace), "{id} has whitespace");
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{id} is not a slug"
            );
            assert!(!ids[i + 1..].contains(id), "{id} is duplicated");
        }
    }

    #[test]
    fn find_every_palette() {
        for p in builtin() {
            assert_eq!(find(&p.id, &[]).map(|f| f.id.as_ref()), Some(p.id.as_ref()));
        }
        assert!(find("nope", &[]).is_none());
    }

    #[test]
    fn find_custom_palette() {
        let mut custom = data_editor::PALETTES[0].clone();
        custom.id = Cow::Owned("mine".to_string());
        custom.source = Source::Custom;
        let list = vec![custom];
        assert_eq!(find("mine", &list).map(|p| p.source), Some(Source::Custom));
    }

    #[test]
    fn builtin_ids_do_not_collide_with_palettes() {
        for (id, _) in BUILTIN {
            assert!(find(id, &[]).is_none(), "{id} is both builtin and palette");
        }
    }

    #[test]
    fn sources_match_data_file() {
        assert!(
            data_editor::PALETTES
                .iter()
                .all(|p| p.source == Source::Editor)
        );
        assert!(
            data_community::PALETTES
                .iter()
                .all(|p| p.source == Source::Community)
        );
    }

    #[test]
    fn accents_differ_from_panel_fill() {
        for p in builtin() {
            let Accents { down, up, block } = p.accents;
            for c in [down, up, block] {
                assert_ne!(c, p.bg, "{} accent equals its background", p.id);
            }
            assert!(down != up && up != block && down != block, "{}", p.id);
        }
    }

    #[test]
    fn custom_theme_round_trip() {
        let p = &data_editor::PALETTES[0];
        let custom = CustomTheme::from(p);
        let back = custom.to_palette();
        assert_eq!(back.source, Source::Custom);
        assert_eq!(back.id, p.id);
        assert_eq!(back.accents, p.accents);
        assert_eq!(back.bg, p.bg);
        assert_eq!(back.error, p.error);
    }

    #[test]
    fn custom_theme_bad_colours_fall_back() {
        let custom = CustomTheme {
            id: "x".into(),
            dark: true,
            bg: "not a colour".into(),
            ..CustomTheme::default()
        };
        let p = custom.to_palette();
        let base = data_editor::PALETTES.iter().find(|p| p.dark).unwrap();
        assert_eq!(p.bg, base.bg);
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(parse_hex("#1F1F1F"), Some(rgb(0x1F1F1F)));
        assert_eq!(parse_hex("1f1f1f"), Some(rgb(0x1F1F1F)));
        assert_eq!(parse_hex("#1F1F1F80"), Some(rgb(0x1F1F1F)));
        assert_eq!(parse_hex("#fff"), None);
        assert_eq!(to_hex(rgb(0x0A0B0C)), "#0a0b0c");
    }

    /// WCAG contrast ratio between two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let lin = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn widget_text_reads_on_fill_and_panel() {
        for p in builtin() {
            let v = visuals(p);
            let w = &v.widgets;
            for (state, s) in [
                ("noninteractive", w.noninteractive),
                ("inactive", w.inactive),
                ("hovered", w.hovered),
                ("active", w.active),
                ("open", w.open),
            ] {
                let fg = s.fg_stroke.color;
                // Checkbox and radio labels sit on the bare panel in every state.
                for (what, bg) in [("fill", s.weak_bg_fill), ("panel", v.panel_fill)] {
                    let c = contrast(fg, bg);
                    assert!(c >= 4.0, "{} {state} text on {what}: {c:.2}", p.id);
                }
            }
            let sel = contrast(v.selection.stroke.color, v.selection.bg_fill);
            assert!(sel >= 4.0, "{} selection: {sel:.2}", p.id);
            let ring = contrast(v.selection.stroke.color, v.text_edit_bg_color());
            assert!(ring >= 3.0, "{} focus ring: {ring:.2}", p.id);
        }
    }

    #[test]
    fn apply_every_id() {
        let ctx = egui::Context::default();
        assert_eq!(apply(&ctx, "nope", &[]), Accents::DEFAULT);
        assert_eq!(ctx.options(|o| o.theme_preference), ThemePreference::System);
        for (id, _) in BUILTIN {
            assert_eq!(apply(&ctx, id, &[]), Accents::DEFAULT);
        }
        for p in builtin() {
            assert_eq!(apply(&ctx, &p.id, &[]), p.accents);
            let theme = if p.dark { Theme::Dark } else { Theme::Light };
            assert_eq!(ctx.style_of(theme).visuals.panel_fill, p.bg);
        }
        // Back to the default restores stock visuals for both modes.
        apply(&ctx, DEFAULT_ID, &[]);
        assert_eq!(ctx.style_of(Theme::Dark).visuals, Visuals::dark());
        assert_eq!(ctx.style_of(Theme::Light).visuals, Visuals::light());
    }
}
