//! Appearance options layered on top of the active theme: corner rounding,
//! density, UI scale, shadows, button frames, font and accent overrides.
//!
//! Every default is a no-op, so `Appearance::default()` leaves egui's stock
//! style exactly as it is. `apply` must run after `theme::apply`, which
//! resets both stock visuals.

use eframe::egui::{
    self, CornerRadius, FontDefinitions, FontFamily, Id, Shadow, Spacing, Ui, vec2,
};
use serde::{Deserialize, Serialize};

use crate::theme::{self, Accents};

/// Corner rounding of widgets, windows and menus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Corners {
    /// Square corners.
    None,
    /// Clearly rounded.
    Round,
    /// Fully rounded buttons and inputs.
    Pill,
    /// egui's stock radii (no change). Last so unknown values land here.
    #[default]
    #[serde(other)]
    Subtle,
}

impl Corners {
    /// Menu order.
    pub const ALL: [Corners; 4] = [
        Corners::None,
        Corners::Subtle,
        Corners::Round,
        Corners::Pill,
    ];
}

/// Spacing and row height preset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    /// Tighter padding and shorter rows.
    Compact,
    /// Looser padding and taller rows.
    Spacious,
    /// egui's stock spacing (no change). Last so unknown values land here.
    #[default]
    #[serde(other)]
    Normal,
}

impl Density {
    /// Menu order.
    pub const ALL: [Density; 3] = [Density::Compact, Density::Normal, Density::Spacious];

    /// Height of a table row and of the table header.
    pub fn row_height(self) -> f32 {
        match self {
            Density::Compact => 16.0,
            Density::Normal => 20.0,
            Density::Spacious => 26.0,
        }
    }
}

/// Which font family the whole UI uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Font {
    /// The bundled monospace font everywhere.
    Monospace,
    /// egui's stock proportional font (no change). Last so unknown values
    /// land here.
    #[default]
    #[serde(other)]
    Default,
}

impl Font {
    /// Menu order.
    pub const ALL: [Font; 2] = [Font::Default, Font::Monospace];
}

/// Custom accent colours as `#rrggbb` strings, overriding the theme's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccentOverride {
    /// Download traffic colour.
    pub down: String,
    /// Upload traffic colour.
    pub up: String,
    /// Blocked traffic and error colour.
    pub block: String,
}

/// The scale steps offered in the Customise window, as zoom factors.
pub const SCALE_STEPS: [f32; 6] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];

/// User appearance preferences, persisted in `Settings::appearance`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// Corner rounding of widgets, windows and menus.
    pub corners: Corners,
    /// Spacing and table row height preset.
    pub density: Density,
    /// UI zoom factor; 1.0 is stock. Clamped to `0.8..=1.5` on apply.
    pub scale: f32,
    /// Shadows under menus, popups and windows.
    pub shadows: bool,
    /// Alternating table row fill.
    pub stripes: bool,
    /// Framed buttons (false draws flat text buttons).
    pub button_frames: bool,
    /// Font family for the whole UI.
    pub font: Font,
    /// Draw the rate and total columns in the monospace font.
    pub numbers_monospace: bool,
    /// Custom accents; `None` uses the theme's.
    pub accent_override: Option<AccentOverride>,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            corners: Corners::Subtle,
            density: Density::Normal,
            scale: 1.0,
            shadows: true,
            stripes: true,
            button_frames: true,
            font: Font::Default,
            numbers_monospace: false,
            accent_override: None,
        }
    }
}

impl Appearance {
    /// The accent override as colours, if set and fully parseable.
    pub fn accents(&self) -> Option<Accents> {
        let o = self.accent_override.as_ref()?;
        Some(Accents {
            down: theme::parse_hex(&o.down)?,
            up: theme::parse_hex(&o.up)?,
            block: theme::parse_hex(&o.block)?,
        })
    }
}

/// Apply `a` on top of the current visuals of both egui themes and set the
/// zoom factor. With `Appearance::default()` this changes nothing.
pub fn apply(ctx: &egui::Context, a: &Appearance) {
    // `theme::apply` resets the visuals but not the spacing, so `Normal`
    // writes egui's stock spacing back. When nothing was changed before,
    // that rewrites the same values and stays a no-op.
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        let radii = match a.corners {
            Corners::None => Some((CornerRadius::ZERO, CornerRadius::ZERO)),
            Corners::Subtle => None,
            Corners::Round => Some((CornerRadius::same(6), CornerRadius::same(10))),
            Corners::Pill => Some((CornerRadius::same(12), CornerRadius::same(12))),
        };
        if let Some((widget, window)) = radii {
            let w = &mut v.widgets;
            for state in [
                &mut w.noninteractive,
                &mut w.inactive,
                &mut w.hovered,
                &mut w.active,
                &mut w.open,
            ] {
                state.corner_radius = widget;
            }
            v.window_corner_radius = window;
            v.menu_corner_radius = window;
        }
        if !a.shadows {
            v.window_shadow = Shadow::NONE;
            v.popup_shadow = Shadow::NONE;
        }
        if !a.button_frames {
            v.button_frame = false;
        }

        let stock = Spacing::default();
        let (item, button, row, menu) = match a.density {
            Density::Compact => (vec2(6.0, 2.0), vec2(3.0, 0.0), 16.0, 1.0),
            Density::Normal => (
                stock.item_spacing,
                stock.button_padding,
                stock.interact_size.y,
                stock.menu_spacing,
            ),
            Density::Spacious => (vec2(10.0, 6.0), vec2(8.0, 4.0), 26.0, 4.0),
        };
        let sp = &mut style.spacing;
        sp.item_spacing = item;
        sp.button_padding = button;
        sp.interact_size.y = row;
        sp.menu_spacing = menu;
    });

    apply_font(ctx, a.font);

    let zoom = a.scale.clamp(0.8, 1.5);
    if (zoom - ctx.zoom_factor()).abs() > f32::EPSILON {
        ctx.set_zoom_factor(zoom);
    }
}

/// Memory id remembering which `Font` is installed.
fn font_id() -> Id {
    Id::new("throttle_appearance_font")
}

/// Switch the UI font family, remembering what was installed so that the
/// default never calls `set_fonts` (which compares the font data).
fn apply_font(ctx: &egui::Context, font: Font) {
    let current = ctx
        .data(|d| d.get_temp::<Font>(font_id()))
        .unwrap_or(Font::Default);
    if current == font {
        return;
    }
    let mut fonts = FontDefinitions::default();
    if font == Font::Monospace {
        let mono = fonts
            .families
            .get(&FontFamily::Monospace)
            .cloned()
            .unwrap_or_default();
        fonts.families.insert(FontFamily::Proportional, mono);
    }
    ctx.set_fonts(fonts);
    ctx.data_mut(|d| d.insert_temp(font_id(), font));
}

/// The stock accents offered when custom accents are switched on.
const STOCK_ACCENTS: [&str; 3] = ["#4caf50", "#2196f3", "#e53935"];

/// The body of the Customise window. Returns true when anything changed,
/// so the caller can re-apply and save.
pub fn customise_ui(ui: &mut Ui, a: &mut Appearance) -> bool {
    let mut changed = false;

    egui::Grid::new("customise_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Corners")
                .on_hover_text("How rounded buttons, menus and windows are.");
            ui.horizontal(|ui| {
                for c in Corners::ALL {
                    changed |= ui
                        .radio_value(&mut a.corners, c, corners_label(c))
                        .changed();
                }
            });
            ui.end_row();

            ui.label("Density")
                .on_hover_text("Spacing between widgets and the height of table rows.");
            ui.horizontal(|ui| {
                for d in Density::ALL {
                    changed |= ui
                        .radio_value(&mut a.density, d, density_label(d))
                        .changed();
                }
            });
            ui.end_row();

            ui.label("Scale")
                .on_hover_text("Size of everything in the window.");
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    for step in SCALE_STEPS {
                        let selected = (a.scale - step).abs() < 0.001;
                        let label = format!("{}%", (step * 100.0).round() as i32);
                        if ui.selectable_label(selected, label).clicked() && !selected {
                            a.scale = step;
                            changed = true;
                        }
                    }
                });
                ui.weak("Ctrl + and Ctrl - also zoom; the setting is remembered.");
            });
            ui.end_row();

            ui.label("Font")
                .on_hover_text("The typeface used for the whole window.");
            ui.horizontal(|ui| {
                for f in Font::ALL {
                    changed |= ui.radio_value(&mut a.font, f, font_label(f)).changed();
                }
            });
            ui.end_row();

            ui.label("Details");
            ui.vertical(|ui| {
                changed |= ui
                    .checkbox(&mut a.shadows, "Shadows")
                    .on_hover_text("Drop shadows under menus, popups and windows.")
                    .changed();
                changed |= ui
                    .checkbox(&mut a.stripes, "Table stripes")
                    .on_hover_text("Shade every other row of the process table.")
                    .changed();
                changed |= ui
                    .checkbox(&mut a.button_frames, "Button frames")
                    .on_hover_text("Draw a frame around buttons; off shows flat text buttons.")
                    .changed();
                changed |= ui
                    .checkbox(&mut a.numbers_monospace, "Monospace numbers")
                    .on_hover_text("Line up the rate and total columns in a fixed width font.")
                    .changed();
            });
            ui.end_row();

            ui.label("Accents");
            ui.vertical(|ui| {
                let mut custom = a.accent_override.is_some();
                if ui
                    .checkbox(&mut custom, "Custom accents")
                    .on_hover_text("Pick your own download, upload and blocked colours.")
                    .changed()
                {
                    a.accent_override = custom.then(|| {
                        let [down, up, block] = STOCK_ACCENTS.map(String::from);
                        AccentOverride { down, up, block }
                    });
                    changed = true;
                }
                if let Some(o) = a.accent_override.as_mut() {
                    for ((label, value), stock) in [
                        ("Download", &mut o.down),
                        ("Upload", &mut o.up),
                        ("Blocked", &mut o.block),
                    ]
                    .into_iter()
                    .zip(STOCK_ACCENTS)
                    {
                        ui.horizontal(|ui| {
                            let mut colour = theme::parse_hex(value)
                                .or_else(|| theme::parse_hex(stock))
                                .unwrap_or_default();
                            if ui.color_edit_button_srgba(&mut colour).changed() {
                                *value = theme::to_hex(colour);
                                changed = true;
                            }
                            ui.label(label);
                        });
                    }
                }
            });
            ui.end_row();
        });

    ui.separator();
    if ui
        .button("Reset to defaults")
        .on_hover_text("Go back to the stock look.")
        .clicked()
        && *a != Appearance::default()
    {
        *a = Appearance::default();
        changed = true;
    }
    changed
}

fn corners_label(c: Corners) -> &'static str {
    match c {
        Corners::None => "Square",
        Corners::Subtle => "Subtle",
        Corners::Round => "Round",
        Corners::Pill => "Pill",
    }
}

fn density_label(d: Density) -> &'static str {
    match d {
        Density::Compact => "Compact",
        Density::Normal => "Normal",
        Density::Spacious => "Spacious",
    }
}

fn font_label(f: Font) -> &'static str {
    match f {
        Font::Default => "Default",
        Font::Monospace => "Monospace",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Theme;

    #[test]
    fn default_apply_is_a_no_op() {
        let ctx = egui::Context::default();
        apply(&ctx, &Appearance::default());
        // `Style` as a whole never compares equal (its number formatter is a
        // function pointer), so compare the parts the options touch.
        for theme in [Theme::Dark, Theme::Light] {
            let got = ctx.style_of(theme);
            let want = theme.default_style();
            assert_eq!(got.visuals, want.visuals);
            assert_eq!(got.spacing, want.spacing);
            assert_eq!(got.text_styles, want.text_styles);
        }
        assert_eq!(ctx.zoom_factor(), 1.0);
    }

    #[test]
    fn options_reach_both_themes_and_normal_restores_spacing() {
        let ctx = egui::Context::default();
        let a = Appearance {
            corners: Corners::None,
            density: Density::Compact,
            shadows: false,
            button_frames: false,
            ..Appearance::default()
        };
        apply(&ctx, &a);
        for theme in [Theme::Dark, Theme::Light] {
            let s = ctx.style_of(theme);
            assert_eq!(s.visuals.widgets.inactive.corner_radius, CornerRadius::ZERO);
            assert_eq!(s.visuals.window_corner_radius, CornerRadius::ZERO);
            assert_eq!(s.visuals.window_shadow, Shadow::NONE);
            assert_eq!(s.visuals.popup_shadow, Shadow::NONE);
            assert!(!s.visuals.button_frame);
            assert_eq!(s.spacing.item_spacing, vec2(6.0, 2.0));
            assert_eq!(s.spacing.interact_size.y, 16.0);
        }
        // Switching back: theme::apply resets the visuals, apply the spacing.
        crate::theme::apply(&ctx, crate::theme::DEFAULT_ID, &[]);
        apply(&ctx, &Appearance::default());
        for theme in [Theme::Dark, Theme::Light] {
            let got = ctx.style_of(theme);
            let want = theme.default_style();
            assert_eq!(got.visuals, want.visuals);
            assert_eq!(got.spacing, want.spacing);
        }
    }

    /// A palette plus corner, shadow and font changes, then back to the
    /// default theme and appearance: both styles must be stock again.
    #[test]
    fn default_theme_and_appearance_restore_stock_after_a_palette() {
        use eframe::egui::{ThemePreference, Visuals};
        let ctx = egui::Context::default();
        let palette = crate::theme::builtin()
            .next()
            .expect("bundled palettes exist");
        crate::theme::apply(&ctx, &palette.id, &[]);
        let a = Appearance {
            corners: Corners::Pill,
            shadows: false,
            font: Font::Monospace,
            ..Appearance::default()
        };
        apply(&ctx, &a);
        let changed = if palette.dark {
            Theme::Dark
        } else {
            Theme::Light
        };
        assert_ne!(
            ctx.style_of(changed).visuals,
            changed.default_style().visuals
        );
        assert_ne!(ctx.style_of(Theme::Light).visuals, Visuals::light());

        crate::theme::apply(&ctx, crate::theme::DEFAULT_ID, &[]);
        apply(&ctx, &Appearance::default());
        for theme in [Theme::Dark, Theme::Light] {
            let got = ctx.style_of(theme);
            let want = theme.default_style();
            assert_eq!(got.visuals, want.visuals);
            assert_eq!(got.spacing, want.spacing);
            assert_eq!(got.text_styles, want.text_styles);
        }
        assert_eq!(ctx.options(|o| o.theme_preference), ThemePreference::System);
        assert_eq!(
            ctx.data(|d| d.get_temp::<Font>(font_id())),
            Some(Font::Default),
            "the stock font family is installed again"
        );
    }

    #[test]
    fn pill_corners_and_spacious_density() {
        let ctx = egui::Context::default();
        let a = Appearance {
            corners: Corners::Pill,
            density: Density::Spacious,
            ..Appearance::default()
        };
        apply(&ctx, &a);
        for theme in [Theme::Dark, Theme::Light] {
            let s = ctx.style_of(theme);
            assert_eq!(
                s.visuals.widgets.hovered.corner_radius,
                CornerRadius::same(12)
            );
            assert_eq!(s.visuals.menu_corner_radius, CornerRadius::same(12));
            assert_eq!(s.spacing.button_padding, vec2(8.0, 4.0));
            assert_eq!(s.spacing.menu_spacing, 4.0);
        }
    }

    #[test]
    fn font_flag_tracks_the_installed_family() {
        let ctx = egui::Context::default();
        let flag = |ctx: &egui::Context| ctx.data(|d| d.get_temp::<Font>(font_id()));
        apply_font(&ctx, Font::Default);
        assert_eq!(flag(&ctx), None);
        apply_font(&ctx, Font::Monospace);
        assert_eq!(flag(&ctx), Some(Font::Monospace));
        apply_font(&ctx, Font::Default);
        assert_eq!(flag(&ctx), Some(Font::Default));
    }

    #[test]
    fn scale_is_clamped() {
        let ctx = egui::Context::default();
        let a = Appearance {
            scale: 3.0,
            ..Appearance::default()
        };
        apply(&ctx, &a);
        // The new zoom factor takes effect at the start of the next pass.
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        assert_eq!(ctx.zoom_factor(), 1.5);
    }

    #[test]
    fn unknown_enum_values_fall_back_to_defaults() {
        let a: Appearance =
            serde_json::from_str(r#"{"corners":"hexagonal","density":"huge","font":"comic"}"#)
                .unwrap();
        assert_eq!(a, Appearance::default());
    }

    #[test]
    fn partial_object_keeps_other_defaults() {
        let a: Appearance = serde_json::from_str(r#"{"shadows":false}"#).unwrap();
        assert!(!a.shadows);
        assert_eq!(a.scale, 1.0);
        assert!(a.stripes);
    }

    #[test]
    fn accent_override_parses_or_is_none() {
        let mut a = Appearance::default();
        assert!(a.accents().is_none());
        a.accent_override = Some(AccentOverride {
            down: "#00ff00".into(),
            up: "#0000ff".into(),
            block: "#ff0000".into(),
        });
        assert!(a.accents().is_some());
        a.accent_override.as_mut().unwrap().up = "blue".into();
        assert!(a.accents().is_none());
    }
}
