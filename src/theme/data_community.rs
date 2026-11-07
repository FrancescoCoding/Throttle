//! Palettes taken from popular third party colour themes, each MIT licensed;
//! see `THIRD_PARTY_NOTICES.md` for the copyright lines. Every entry uses
//! `Source::Community`; `dark` decides whether it appears in the Dark or the
//! Light submenu.
//!
//! Sources (repository, file, commit or tag):
//!   Dracula: github.com/dracula/visual-studio-code, src/dracula.yml @ b50e62e32090
//!   One Dark Pro: github.com/Binaryify/OneDark-Pro, themes/OneDark-Pro.json @ 75c80ab18d90
//!   Catppuccin Mocha, Macchiato, Frappe, Latte: github.com/catppuccin/vscode,
//!     release catppuccin-vsc-v3.19.0, themes/{mocha,macchiato,frappe,latte}.json
//!     in the published package (the repository generates them)
//!   Nord: github.com/nordtheme/visual-studio-code, themes/nord-color-theme.json
//!     @ 8ead09822c02
//!   Tokyo Night, Storm, Light: github.com/tokyo-night/tokyo-night-vscode-theme,
//!     themes/tokyo-night{,-storm,-light}-color-theme.json @ 7c0f11eaef32
//!   GitHub Dark, Light: github.com/primer/github-vscode-theme, src/theme.js and
//!     src/colors.js @ cd78e5e4e7bc, evaluated by hand against
//!     dist/json/colors/{dark,light}.json of the @primer/primitives 7.10.0 npm
//!     package (github.com/primer/primitives), the version that commit pins
//!   Gruvbox Dark, Light: github.com/jdinhify/vscode-theme-gruvbox, src/shared.ts
//!     and src/colors/base.ts @ 5538f16b660b, medium contrast
//!   Ayu Dark, Mirage, Light: github.com/ayu-theme/vscode-ayu,
//!     ayu-{dark,mirage,light}.json @ 605c9c850468
//!   Night Owl: github.com/sdras/night-owl-vscode-theme,
//!     themes/Night Owl-color-theme.json @ cc291eba7976
//!   Rose Pine: github.com/rose-pine/vscode, themes/rose-pine-color-theme.json
//!     @ d8f5ebe8e096
//!   Everforest Dark: github.com/sainnhe/everforest-vscode (archived),
//!     themes/everforest-dark.json @ b17f8affe909, medium contrast
//!   Kanagawa: github.com/paccodes/kanagawa-vscode-theme,
//!     themes/kanagawa-wave-color-theme.json @ 60f23d7520c8, a port of
//!     github.com/rebelot/kanagawa.nvim (Wave variant)
//!   Cobalt2: github.com/wesbos/cobalt2-vscode, theme/cobalt2.json @ c4e9574372b8
//!   SynthWave '84: github.com/robb0wen/synthwave-vscode,
//!     themes/synthwave-color-theme.json @ ecfa2fe1279f
//!
//! Each field takes the workbench key named in its `Palette` doc. Unless a line
//! says otherwise, `selection` is list.activeSelectionBackground, `selection_fg`
//! is list.activeSelectionForeground, and light palettes hover with
//! list.hoverBackground and black text. Translucent values are composited onto
//! editor.background. Missing keys use the workbench default, marked
//! (default); (blend) is the foreground mixed 70% into the background, used
//! when descriptionForeground is missing, equals the foreground or reads below
//! 3:1. Lines marked (adjusted) were changed so that widget text reaches 4:1 on
//! its fill and on the panel (see `widget_text_reads_on_fill_and_panel`).
//! Notes:
//!   Accents are the theme's ANSI green, blue and red unless noted. Dracula:
//!     cyan stands in for blue (its ANSI blue is purple). Nord: frost blue
//!     (textLink). Rose Pine: pine, foam and love, its ANSI green, blue and red.
//!     Tokyo Night (all three): the string token green. Gruvbox and Ayu: the
//!     editorGutter added, modified and deleted colours.
//!   Dark palettes whose primary button is a bright accent with dark text
//!     (Catppuccin, Nord, Ayu, Rose Pine, Everforest) hover with their
//!     selection.background tint and the editor foreground instead, since the
//!     dark button text cannot read on the panel. The Frappe, Nord and Cobalt2
//!     hover fills and the Nord selection are darkened toward the background
//!     until their text reaches 4:1.
//!   Catppuccin: `widget_bg` is checkbox.background because Frappe's
//!     button.secondaryBackground misses 4:1; `input_border` is panel.border
//!     because input.border and checkbox.border are transparent.
//!   Nord: its selection is a bright frost fill meant for dark text, so
//!     `selection` is selection.background darkened and `selection_fg` is
//!     list.hoverForeground.
//!   Tokyo Night (all three): errorForeground is a grey, so `error` is
//!     editorError.foreground. Tokyo Night Light: its file says "dark" but is a
//!     light theme; `widget_bg` is list.hoverBackground (its secondary button
//!     misses 4:1), and list.activeSelectionBackground equals the background,
//!     so `selection` is editor.selectionBackground and the hover uses
//!     selection.background.
//!   Borders: Dracula uses tab.border and Cobalt2 sideBar.border, because their
//!     panel.border is a saturated accent; Rose Pine (transparent panel.border)
//!     and Everforest (panel.border equal to the background) use
//!     editorWidget.border.
//!   Everforest: input.background and focusBorder are transparent, so
//!     `input_bg` is the background and `focus` is its green.
//!   Cobalt2: list.activeSelectionBackground equals the background, so
//!     `selection` is editor.selectionBackground with menu.selectionForeground.
//!   GitHub: built from the primitives with the overrides in src/colors.js;
//!     editor.selectionBackground ends up unset for both palettes.
//!   Dracula (button.secondaryBackground), Gruvbox, Night Owl and Cobalt2
//!     (dropdown.background) draw idle buttons in the background colour,
//!     outlined by `input_border`, as the themes do.

use std::borrow::Cow;

use super::{Palette, Source, accents, rgb};

/// Community palettes, in menu order: dark ones first, then light.
pub const PALETTES: &[Palette] = &[
    Palette {
        id: Cow::Borrowed("dracula"),
        label: Cow::Borrowed("Dracula"),
        source: Source::Community,
        dark: true,
        accents: accents(0x50FA7B, 0x8BE9FD, 0xFF5555),
        bg: rgb(0x282A36),
        faint_bg: rgb(0x21222C),
        input_bg: rgb(0x282A36),
        widget_bg: rgb(0x282A36),
        border: rgb(0x191A21), // tab.border
        input_border: rgb(0x191A21),
        hover_bg: rgb(0x44475A),
        hover_fg: rgb(0xF8F8F2),
        focus: rgb(0x6272A4),
        selection: rgb(0x44475A),
        selection_fg: rgb(0xF8F8F2),
        fg: rgb(0xF8F8F2),
        weak_fg: rgb(0xBABABA), // (blend)
        link: rgb(0x3794FF),    // (default)
        error: rgb(0xFF5555),
    },
    Palette {
        id: Cow::Borrowed("one-dark-pro"),
        label: Cow::Borrowed("One Dark Pro"),
        source: Source::Community,
        dark: true,
        accents: accents(0x8CC265, 0x4AA5F0, 0xE05561),
        bg: rgb(0x282C34),
        faint_bg: rgb(0x21252B),
        input_bg: rgb(0x1D1F23),
        widget_bg: rgb(0x30333D),
        border: rgb(0x3E4452),
        input_border: rgb(0x404754),
        hover_bg: rgb(0x404754),
        hover_fg: rgb(0xFFFFFF), // (default)
        focus: rgb(0x3E4452),
        selection: rgb(0x2C313A),
        selection_fg: rgb(0xD7DAE0),
        fg: rgb(0xABB2BF),
        weak_fg: rgb(0x848A95), // (blend)
        link: rgb(0x61AFEF),
        error: rgb(0xF48771), // (default)
    },
    Palette {
        id: Cow::Borrowed("catppuccin-mocha"),
        label: Cow::Borrowed("Catppuccin Mocha"),
        source: Source::Community,
        dark: true,
        accents: accents(0xA6E3A1, 0x89B4FA, 0xF38BA8),
        bg: rgb(0x1E1E2E),
        faint_bg: rgb(0x181825),
        input_bg: rgb(0x313244),
        widget_bg: rgb(0x45475A), // checkbox.background
        border: rgb(0x585B70),
        input_border: rgb(0x585B70), // panel.border
        hover_bg: rgb(0x63547E),     // selection.background (adjusted)
        hover_fg: rgb(0xCDD6F4),     // editor.foreground (adjusted)
        focus: rgb(0xCBA6F7),
        selection: rgb(0x313244),
        selection_fg: rgb(0xCDD6F4),
        fg: rgb(0xCDD6F4),
        weak_fg: rgb(0x989FB9), // (blend)
        link: rgb(0x89B4FA),
        error: rgb(0xF38BA8),
    },
    Palette {
        id: Cow::Borrowed("catppuccin-macchiato"),
        label: Cow::Borrowed("Catppuccin Macchiato"),
        source: Source::Community,
        dark: true,
        accents: accents(0xA6DA95, 0x8AADF4, 0xED8796),
        bg: rgb(0x24273A),
        faint_bg: rgb(0x1E2030),
        input_bg: rgb(0x363A4F),
        widget_bg: rgb(0x494D64), // checkbox.background
        border: rgb(0x5B6078),
        input_border: rgb(0x5B6078), // panel.border
        hover_bg: rgb(0x655785),     // selection.background (adjusted)
        hover_fg: rgb(0xCAD3F5),     // editor.foreground (adjusted)
        focus: rgb(0xC6A0F6),
        selection: rgb(0x363A4F),
        selection_fg: rgb(0xCAD3F5),
        fg: rgb(0xCAD3F5),
        weak_fg: rgb(0x989FBD), // (blend)
        link: rgb(0x8AADF4),
        error: rgb(0xED8796),
    },
    Palette {
        id: Cow::Borrowed("catppuccin-frappe"),
        label: Cow::Borrowed("Catppuccin Frappe"),
        source: Source::Community,
        dark: true,
        accents: accents(0xA6D189, 0x8CAAEE, 0xE78284),
        bg: rgb(0x303446),
        faint_bg: rgb(0x292C3C),
        input_bg: rgb(0x414559),
        widget_bg: rgb(0x51576D), // checkbox.background
        border: rgb(0x626880),
        input_border: rgb(0x626880), // panel.border
        hover_bg: rgb(0x685A80),     // selection.background, darkened (adjusted)
        hover_fg: rgb(0xC6D0F5),     // editor.foreground (adjusted)
        focus: rgb(0xCA9EE6),
        selection: rgb(0x414559),
        selection_fg: rgb(0xC6D0F5),
        fg: rgb(0xC6D0F5),
        weak_fg: rgb(0x99A1C0), // (blend)
        link: rgb(0x8CAAEE),
        error: rgb(0xE78284),
    },
    Palette {
        id: Cow::Borrowed("nord"),
        label: Cow::Borrowed("Nord"),
        source: Source::Community,
        dark: true,
        accents: accents(0xA3BE8C, 0x88C0D0, 0xBF616A),
        bg: rgb(0x2E3440),
        faint_bg: rgb(0x2E3440),
        input_bg: rgb(0x3B4252),
        widget_bg: rgb(0x434C5E),
        border: rgb(0x3B4252),
        input_border: rgb(0x3B4252),
        hover_bg: rgb(0x5A7886), // selection.background, darkened (adjusted)
        hover_fg: rgb(0xECEFF4), // list.hoverForeground (adjusted)
        focus: rgb(0x3B4252),
        selection: rgb(0x5A7886), // selection.background, darkened (adjusted)
        selection_fg: rgb(0xECEFF4), // list.hoverForeground (adjusted)
        fg: rgb(0xD8DEE9),
        weak_fg: rgb(0xC7CDD8),
        link: rgb(0x88C0D0),
        error: rgb(0xBF616A),
    },
    Palette {
        id: Cow::Borrowed("tokyo-night"),
        label: Cow::Borrowed("Tokyo Night"),
        source: Source::Community,
        dark: true,
        accents: accents(0x9ECE6A, 0x7AA2F7, 0xF7768E),
        bg: rgb(0x1A1B26),
        faint_bg: rgb(0x16161E),
        input_bg: rgb(0x14141B),
        widget_bg: rgb(0x3B3E52),
        border: rgb(0x101014),
        input_border: rgb(0x0F0F14),
        hover_bg: rgb(0x385191),
        hover_fg: rgb(0xFFFFFF),
        focus: rgb(0x262838),
        selection: rgb(0x202330),
        selection_fg: rgb(0xA9B1D6),
        fg: rgb(0xA9B1D6),
        weak_fg: rgb(0x7E84A1), // (blend)
        link: rgb(0x6183BB),
        error: rgb(0xDB4B4B), // editorError.foreground
    },
    Palette {
        id: Cow::Borrowed("tokyo-night-storm"),
        label: Cow::Borrowed("Tokyo Night Storm"),
        source: Source::Community,
        dark: true,
        accents: accents(0x9ECE6A, 0x7AA2F7, 0xF7768E),
        bg: rgb(0x24283B),
        faint_bg: rgb(0x1F2335),
        input_bg: rgb(0x1B1E2E),
        widget_bg: rgb(0x41496B),
        border: rgb(0x1B1E2E),
        input_border: rgb(0x282E44),
        hover_bg: rgb(0x3A5293),
        hover_fg: rgb(0xFFFFFF),
        focus: rgb(0x2E3248),
        selection: rgb(0x2C324A),
        selection_fg: rgb(0xA9B1D6),
        fg: rgb(0xA9B1D6),
        weak_fg: rgb(0x8188A8), // (blend)
        link: rgb(0x668AC4),
        error: rgb(0xDB4B4B), // editorError.foreground
    },
    Palette {
        id: Cow::Borrowed("github-dark"),
        label: Cow::Borrowed("GitHub Dark"),
        source: Source::Community,
        dark: true,
        accents: accents(0x3FB950, 0x58A6FF, 0xFF7B72),
        bg: rgb(0x0D1117),
        faint_bg: rgb(0x010409),
        input_bg: rgb(0x0D1117),
        widget_bg: rgb(0x282E33),
        border: rgb(0x30363D),
        input_border: rgb(0x30363D),
        hover_bg: rgb(0x238636),
        hover_fg: rgb(0xFFFFFF),
        focus: rgb(0x1F6FEB),
        selection: rgb(0x343941),
        selection_fg: rgb(0xE6EDF3),
        fg: rgb(0xE6EDF3),
        weak_fg: rgb(0x7D8590),
        link: rgb(0x2F81F7),
        error: rgb(0xF85149),
    },
    Palette {
        id: Cow::Borrowed("gruvbox-dark"),
        label: Cow::Borrowed("Gruvbox Dark"),
        source: Source::Community,
        dark: true,
        accents: accents(0xB8BB26, 0x83A598, 0xFB4934),
        bg: rgb(0x282828),
        faint_bg: rgb(0x282828),
        input_bg: rgb(0x282828),
        widget_bg: rgb(0x282828), // dropdown.background
        border: rgb(0x3C3836),
        input_border: rgb(0x3C3836),
        hover_bg: rgb(0x375758),
        hover_fg: rgb(0xEBDBB2),
        focus: rgb(0x3C3836),
        selection: rgb(0x32302F),
        selection_fg: rgb(0x8EC07C),
        fg: rgb(0xEBDBB2),
        weak_fg: rgb(0xB0A589), // (blend)
        link: rgb(0x83A598),
        error: rgb(0xFB4934),
    },
    Palette {
        id: Cow::Borrowed("ayu-dark"),
        label: Cow::Borrowed("Ayu Dark"),
        source: Source::Community,
        dark: true,
        accents: accents(0x70BF56, 0x73B8FF, 0xF26D78),
        bg: rgb(0x10141C),
        faint_bg: rgb(0x0D1017),
        input_bg: rgb(0x10141C),
        widget_bg: rgb(0x1F242F),
        border: rgb(0x1B1F29),
        input_border: rgb(0x1F242F),
        hover_bg: rgb(0x193155), // selection.background (adjusted)
        hover_fg: rgb(0xBFBDB6), // editor.foreground (adjusted)
        focus: rgb(0xE6B450),
        selection: rgb(0x1E242F),
        selection_fg: rgb(0xBFBDB6),
        fg: rgb(0xBFBDB6),
        weak_fg: rgb(0x5C657B),
        link: rgb(0xE6B450),
        error: rgb(0xD95757),
    },
    Palette {
        id: Cow::Borrowed("ayu-mirage"),
        label: Cow::Borrowed("Ayu Mirage"),
        source: Source::Community,
        dark: true,
        accents: accents(0x87D96C, 0x80BFFF, 0xF27983),
        bg: rgb(0x242936),
        faint_bg: rgb(0x1F2430),
        input_bg: rgb(0x242936),
        widget_bg: rgb(0x343A48),
        border: rgb(0x171B24),
        input_border: rgb(0x343A48),
        hover_bg: rgb(0x2B4768), // selection.background (adjusted)
        hover_fg: rgb(0xCCCAC2), // editor.foreground (adjusted)
        focus: rgb(0xFFCC66),
        selection: rgb(0x2D3445),
        selection_fg: rgb(0xCCCAC2),
        fg: rgb(0xCCCAC2),
        weak_fg: rgb(0x737D8F),
        link: rgb(0xFFCC66),
        error: rgb(0xFF6666),
    },
    Palette {
        id: Cow::Borrowed("night-owl"),
        label: Cow::Borrowed("Night Owl"),
        source: Source::Community,
        dark: true,
        accents: accents(0x22DA6E, 0x82AAFF, 0xEF5350),
        bg: rgb(0x011627),
        faint_bg: rgb(0x011627),
        input_bg: rgb(0x0B253A),
        widget_bg: rgb(0x011627), // dropdown.background
        border: rgb(0x5F7E97),
        input_border: rgb(0x5F7E97),
        hover_bg: rgb(0x654AA3),
        hover_fg: rgb(0xCCD0D4),
        focus: rgb(0x122D42),
        selection: rgb(0x14344F),
        selection_fg: rgb(0xFFFFFF),
        fg: rgb(0xD6DEEB),
        weak_fg: rgb(0x96A2B0), // (blend)
        link: rgb(0x3794FF),    // (default)
        error: rgb(0xEF5350),
    },
    Palette {
        id: Cow::Borrowed("rose-pine"),
        label: Cow::Borrowed("Rose Pine"),
        source: Source::Community,
        dark: true,
        accents: accents(0x31748F, 0x9CCFD8, 0xEB6F92),
        bg: rgb(0x191724),
        faint_bg: rgb(0x191724),
        input_bg: rgb(0x232034),
        widget_bg: rgb(0x1F1D2E),
        border: rgb(0x26233A), // editorWidget.border
        input_border: rgb(0x2A2838),
        hover_bg: rgb(0x3B384B), // selection.background (adjusted)
        hover_fg: rgb(0xE0DEF4), // editor.foreground (adjusted)
        focus: rgb(0x2A2838),
        selection: rgb(0x2A2838),
        selection_fg: rgb(0xE0DEF4),
        fg: rgb(0xE0DEF4),
        weak_fg: rgb(0x908CAA),
        link: rgb(0xC4A7E7),
        error: rgb(0xEB6F92),
    },
    Palette {
        id: Cow::Borrowed("everforest-dark"),
        label: Cow::Borrowed("Everforest Dark"),
        source: Source::Community,
        dark: true,
        accents: accents(0xA7C080, 0x7FBBB3, 0xE67E80),
        bg: rgb(0x2D353B),
        faint_bg: rgb(0x2D353B),
        input_bg: rgb(0x2D353B), // (transparent)
        widget_bg: rgb(0x3D484D),
        border: rgb(0x4F585E), // editorWidget.border
        input_border: rgb(0x4F585E),
        hover_bg: rgb(0x444E54), // selection.background (adjusted)
        hover_fg: rgb(0xD3C6AA), // editor.foreground (adjusted)
        focus: rgb(0xA7C080),    // textLink.foreground (adjusted)
        selection: rgb(0x3A444A),
        selection_fg: rgb(0xD3C6AA),
        fg: rgb(0xD3C6AA),
        weak_fg: rgb(0x859289),
        link: rgb(0xA7C080),
        error: rgb(0xE67E80),
    },
    Palette {
        id: Cow::Borrowed("kanagawa"),
        label: Cow::Borrowed("Kanagawa"),
        source: Source::Community,
        dark: true,
        accents: accents(0x76946A, 0x7E9CD8, 0xC34043),
        bg: rgb(0x1F1F28),
        faint_bg: rgb(0x1F1F28),
        input_bg: rgb(0x16161D),
        widget_bg: rgb(0x223249),
        border: rgb(0x16161D),
        input_border: rgb(0x223249),
        hover_bg: rgb(0x2A2A37),
        hover_fg: rgb(0xC8C093),
        focus: rgb(0x223249),
        selection: rgb(0x363646),
        selection_fg: rgb(0xDCD7BA),
        fg: rgb(0xDCD7BA),
        weak_fg: rgb(0xA3A08E), // (blend)
        link: rgb(0x6A9589),
        error: rgb(0xF48771), // (default)
    },
    Palette {
        id: Cow::Borrowed("cobalt2"),
        label: Cow::Borrowed("Cobalt2"),
        source: Source::Community,
        dark: true,
        accents: accents(0x3AD900, 0x0088FF, 0xFF628C),
        bg: rgb(0x193549),
        faint_bg: rgb(0x15232D),
        input_bg: rgb(0x193549),
        widget_bg: rgb(0x193549), // dropdown.background
        border: rgb(0x0D3A58),    // sideBar.border
        input_border: rgb(0x0D3A58),
        hover_bg: rgb(0x037CE5), // button.background, darkened (adjusted)
        hover_fg: rgb(0xFFFFFF),
        focus: rgb(0x0D3A58),
        selection: rgb(0x0050A4),    // editor.selectionBackground (adjusted)
        selection_fg: rgb(0xFFFFFF), // menu.selectionForeground (adjusted)
        fg: rgb(0xFFFFFF),
        weak_fg: rgb(0xAAAAAA),
        link: rgb(0x0088FF),
        error: rgb(0xA22929),
    },
    Palette {
        id: Cow::Borrowed("synthwave-84"),
        label: Cow::Borrowed("SynthWave '84"),
        source: Source::Community,
        dark: true,
        accents: accents(0x72F1B8, 0x03EDF9, 0xFE4450),
        bg: rgb(0x262335),
        faint_bg: rgb(0x241B2F),
        input_bg: rgb(0x2A2139),
        widget_bg: rgb(0x232530),    // dropdown.background
        border: rgb(0x45434F),       // (default)
        input_border: rgb(0x3C3C3C), // (default)
        hover_bg: rgb(0x614D85),
        hover_fg: rgb(0xFFFFFF), // (default)
        focus: rgb(0x1F212B),
        selection: rgb(0x413F4E),
        selection_fg: rgb(0xFFFFFF),
        fg: rgb(0xFFFFFF),      // foreground
        weak_fg: rgb(0xBEBDC2), // (blend)
        link: rgb(0xF97E72),
        error: rgb(0xFE4450),
    },
    Palette {
        id: Cow::Borrowed("catppuccin-latte"),
        label: Cow::Borrowed("Catppuccin Latte"),
        source: Source::Community,
        dark: false,
        accents: accents(0x40A02B, 0x1E66F5, 0xD20F39),
        bg: rgb(0xEFF1F5),
        faint_bg: rgb(0xE6E9EF),
        input_bg: rgb(0xCCD0DA),
        widget_bg: rgb(0xBCC0CC), // checkbox.background
        border: rgb(0xACB0BE),
        input_border: rgb(0xACB0BE), // panel.border
        hover_bg: rgb(0xDDE0E7),
        hover_fg: rgb(0x000000),
        focus: rgb(0x8839EF),
        selection: rgb(0xCCD0DA),
        selection_fg: rgb(0x4C4F69),
        fg: rgb(0x4C4F69),
        weak_fg: rgb(0x7D8093), // (blend)
        link: rgb(0x1E66F5),
        error: rgb(0xD20F39),
    },
    Palette {
        id: Cow::Borrowed("tokyo-night-light"),
        label: Cow::Borrowed("Tokyo Night Light"),
        source: Source::Community,
        dark: false,
        accents: accents(0x385F0D, 0x2959AA, 0x8C4351),
        bg: rgb(0xE6E7ED),
        faint_bg: rgb(0xD6D8DF),
        input_bg: rgb(0xE6E7ED),
        widget_bg: rgb(0xE1E2E8), // list.hoverBackground
        border: rgb(0xC1C2C7),
        input_border: rgb(0xC1C2C7),
        hover_bg: rgb(0xD3D5DE), // selection.background (adjusted)
        hover_fg: rgb(0x000000),
        focus: rgb(0xCED0D7),
        selection: rgb(0xD7D9E1), // editor.selectionBackground (adjusted)
        selection_fg: rgb(0x363C4D),
        fg: rgb(0x343B59),
        weak_fg: rgb(0x707280),
        link: rgb(0x2959AA),
        error: rgb(0xBD4040), // editorError.foreground
    },
    Palette {
        id: Cow::Borrowed("github-light"),
        label: Cow::Borrowed("GitHub Light"),
        source: Source::Community,
        dark: false,
        accents: accents(0x116329, 0x0969DA, 0xCF222E),
        bg: rgb(0xFFFFFF),
        faint_bg: rgb(0xF6F8FA),
        input_bg: rgb(0xFFFFFF),
        widget_bg: rgb(0xEBECF0),
        border: rgb(0xD0D7DE),
        input_border: rgb(0xD0D7DE),
        hover_bg: rgb(0xF4F6F8),
        hover_fg: rgb(0x000000),
        focus: rgb(0x0969DA),
        selection: rgb(0xEFF1F3),
        selection_fg: rgb(0x1F2328),
        fg: rgb(0x1F2328),
        weak_fg: rgb(0x656D76),
        link: rgb(0x0969DA),
        error: rgb(0xCF222E),
    },
    Palette {
        id: Cow::Borrowed("gruvbox-light"),
        label: Cow::Borrowed("Gruvbox Light"),
        source: Source::Community,
        dark: false,
        accents: accents(0x79740E, 0x076678, 0x9D0006),
        bg: rgb(0xFBF1C7),
        faint_bg: rgb(0xFBF1C7),
        input_bg: rgb(0xFBF1C7),
        widget_bg: rgb(0xFBF1C7), // dropdown.background
        border: rgb(0xEBDBB2),
        input_border: rgb(0xEBDBB2),
        hover_bg: rgb(0xF3E6BC),
        hover_fg: rgb(0x000000),
        focus: rgb(0xEBDBB2),
        selection: rgb(0xF3E6BC),
        selection_fg: rgb(0x427B58),
        fg: rgb(0x3C3836),
        weak_fg: rgb(0x757062), // (blend)
        link: rgb(0x076678),
        error: rgb(0x9D0006),
    },
    Palette {
        id: Cow::Borrowed("ayu-light"),
        label: Cow::Borrowed("Ayu Light"),
        source: Source::Community,
        dark: false,
        accents: accents(0x6CBF43, 0x478ACC, 0xFF7383),
        bg: rgb(0xFCFCFC),
        faint_bg: rgb(0xF8F9FA),
        input_bg: rgb(0xFCFCFC),
        widget_bg: rgb(0xE4E7EA),
        border: rgb(0xEAEDEF),
        input_border: rgb(0xE4E7EA),
        hover_bg: rgb(0xE8EAED),
        hover_fg: rgb(0x000000),
        focus: rgb(0xF29718),
        selection: rgb(0xE8EAED),
        selection_fg: rgb(0x5C6166),
        fg: rgb(0x5C6166),
        weak_fg: rgb(0x8490A0),
        link: rgb(0xF29718),
        error: rgb(0xE65050),
    },
];
