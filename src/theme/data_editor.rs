//! Palettes taken from the editor's bundled colour themes.
//!
//! Sources, from github.com/microsoft/vscode (MIT):
//!   Dark Modern:  extensions/theme-defaults/themes/dark_modern.json
//!                 (includes dark_plus.json, which includes dark_vs.json)
//!   Light Modern: extensions/theme-defaults/themes/light_modern.json
//!                 (includes light_plus.json, which includes light_vs.json)
//!   Dark+:        extensions/theme-defaults/themes/dark_plus.json (+ dark_vs.json)
//!   Light+:       extensions/theme-defaults/themes/light_plus.json (+ light_vs.json)
//!   Monokai:      extensions/theme-monokai/themes/monokai-color-theme.json
//!                 (from Colorsublime-Themes, MIT)
//!   Solarized Dark:  extensions/theme-solarized-dark/themes/
//!                    solarized-dark-color-theme.json (from Colorsublime-Themes,
//!                    MIT; Solarized by Ethan Schoonover, MIT)
//!   Solarized Light: extensions/theme-solarized-light/themes/
//!                    solarized-light-color-theme.json (from Colorsublime-Themes,
//!                    MIT; Solarized by Ethan Schoonover, MIT)
//!   Abyss:        extensions/theme-abyss/themes/abyss-color-theme.json
//!                 (from Colorsublime-Themes, MIT)
//!   Kimbie Dark:  extensions/theme-kimbie-dark/themes/kimbie-dark-color-theme.json
//!                 (from Colorsublime-Themes, MIT)
//!   Quiet Light:  extensions/theme-quietlight/themes/quietlight-color-theme.json
//!                 (from Colorsublime-Themes, MIT)
//!   Red:          extensions/theme-red/themes/Red-color-theme.json
//!                 (from Colorsublime-Themes, MIT)
//!   Tomorrow Night Blue: extensions/theme-tomorrow-night-blue/themes/
//!                 tomorrow-night-blue-color-theme.json (from Colorsublime-Themes,
//!                 MIT; Tomorrow by Chris Kempson, MIT)
//!   High Contrast Dark:  extensions/theme-defaults/themes/hc_black.json
//!   High Contrast Light: extensions/theme-defaults/themes/hc_light.json
//! None of the files from Solarized Dark on has an include chain.
//! Keys missing from a file and its include chain use the workbench's built-in
//! default for that key, marked (default); the high contrast files define few
//! workbench colours, so most of their values are the workbench's high
//! contrast defaults. Values marked (blend) are the foreground mixed 70% into
//! the background, as the workbench derives `descriptionForeground`. Values
//! marked (alpha) come from a `#rrggbbaa` key composited over
//! editor.background, as the workbench paints it. Notes:
//!   Dark Modern: `border` is panel.border because widget.border equals
//!     input.background; `widget_bg` is checkbox.background (its secondary
//!     button is transparent); `selection` is menu.selectionBackground;
//!     accents are editorGutter added / textLink.foreground / gutter deleted.
//!   Light Modern: `selection` is the default editor selection (ADD6FF);
//!     `weak_fg` is a blend because its descriptionForeground equals the
//!     editor foreground; accents are editorGutter added / modified / deleted.
//!   Light palettes hover with the selection tint and black text instead of
//!     button.background, since hovered text also sits on the white panel.
//!   Dark+: `selection` is menu.selectionBackground from dark_vs.json; accents
//!     are the type, keyword and invalid token colours.
//!   Light+: `widget_bg` is list.hoverBackground; `selection_fg` is the
//!     0451A5 token colour; accents are the comment, constant and invalid
//!     token colours.
//!   Monokai: `widget_bg` is dropdown.background, `border` is panel.border,
//!     `input_border`, `hover_bg` and `selection` are its 75715E focus colour,
//!     `hover_fg` is the editor foreground, `weak_fg` is
//!     editorLineNumber.foreground, `link` is its cyan and `error` is
//!     inputValidation.errorBorder; accents are its green, cyan and pink.
//!   Where a dark theme leaves checkbox.border unset it defaults to
//!     dropdown.background, which would hide the outline of idle widgets;
//!     those palettes take `input_border` from agentsChatInput.border.
//!   Solarized Dark: `widget_bg` is dropdown.background, `input_border` is
//!     checkbox.border (its dropdown.border), `selection` is
//!     list.activeSelectionBackground (via menu.selectionBackground);
//!     accents are terminal.ansiGreen / ansiBlue / ansiRed.
//!   Solarized Light: `widget_bg` is peekViewEditor.background because
//!     dropdown.background (EEE8D5) gives the editor foreground under 4:1;
//!     `hover_bg` and `selection` are list.activeSelectionBackground since
//!     its editor selection equals sideBar.background; `selection_fg` is
//!     073642 (terminal.ansiBlack) because list.activeSelectionForeground
//!     (6C6C6C) reads under 4:1 on it; accents are terminal.ansiGreen /
//!     ansiBlue / ansiRed.
//!   Abyss: `widget_bg` is dropdown.background, `selection` is
//!     list.activeSelectionBackground; its terminal colours are pale, so
//!     accents are the string, function argument and diff deleted token
//!     colours.
//!   Kimbie Dark: `widget_bg` is dropdown.background, `border` is
//!     panelSection.border (no panel.border), `selection` is
//!     list.activeSelectionBackground; accents are the string, function and
//!     invalid token colours.
//!   Quiet Light: `fg` is the light default, which its token foreground
//!     repeats; `widget_bg` is list.hoverBackground, `border` is
//!     panelSection.border, `hover_bg` and `selection` are its editor
//!     selection; `selection_fg` is the editor foreground because
//!     list.activeSelectionForeground (6C6C6C) reads under 4:1 on the tint;
//!     accents are the string, keyword and invalid token colours.
//!   Red: `widget_bg` is dropdown.background, `border` is panelSection.border,
//!     `hover_bg` expands its #833 button.background, `selection` is
//!     list.activeSelectionBackground; accents are the diff inserted and
//!     keyword token colours and, as the theme has no blue, the default
//!     editorGutter.modifiedBackground.
//!   Tomorrow Night Blue: `widget_bg` is dropdown.background, `border` is
//!     panelSection.border, `selection` is list.activeSelectionBackground;
//!     accents are terminal.ansiGreen / ansiBlue / ansiRed.
//!   High Contrast Dark: `selection` is its selection.background (008000),
//!     since its white editor selection would hide the white text of pressed
//!     widgets; `selection_fg` is white because editor.selectionForeground
//!     (black) would hide the focus ring on the black input; hovered widgets
//!     keep the black button fill and show the focus outline, as the
//!     workbench does; accents are the type, keyword and invalid token
//!     colours.
//!   High Contrast Light: `hover_bg` and `selection` are the default
//!     list.activeSelectionBackground (0F4A85 at 10%); `selection_fg` is
//!     0F4A85 because editor.selectionForeground (white) cannot read on that
//!     tint; accents are the constant, diff changed and invalid token
//!     colours.

use std::borrow::Cow;

use super::{Palette, Source, accents, rgb};

/// Editor built-in palettes, in menu order. The first dark and the first
/// light entry also serve as fallbacks for malformed imported themes.
pub const PALETTES: &[Palette] = &[
    Palette {
        id: Cow::Borrowed("dark-modern"),
        label: Cow::Borrowed("Dark Modern"),
        source: Source::Editor,
        dark: true,
        accents: accents(0x2EA043, 0x4DAAFC, 0xF85149),
        bg: rgb(0x1F1F1F),
        faint_bg: rgb(0x181818),
        input_bg: rgb(0x313131),
        widget_bg: rgb(0x313131),
        border: rgb(0x2B2B2B),
        input_border: rgb(0x3C3C3C),
        hover_bg: rgb(0x0078D4),
        hover_fg: rgb(0xFFFFFF),
        focus: rgb(0x0078D4),
        selection: rgb(0x0078D4),
        selection_fg: rgb(0xFFFFFF),
        fg: rgb(0xCCCCCC),
        weak_fg: rgb(0x9D9D9D),
        link: rgb(0x4DAAFC),
        error: rgb(0xF85149),
    },
    Palette {
        id: Cow::Borrowed("light-modern"),
        label: Cow::Borrowed("Light Modern"),
        source: Source::Editor,
        dark: false,
        accents: accents(0x2EA043, 0x005FB8, 0xF85149),
        bg: rgb(0xFFFFFF),
        faint_bg: rgb(0xF8F8F8),
        input_bg: rgb(0xFFFFFF),
        widget_bg: rgb(0xE5E5E5),
        border: rgb(0xE5E5E5),
        input_border: rgb(0xCECECE),
        hover_bg: rgb(0xADD6FF), // (default)
        hover_fg: rgb(0x000000),
        focus: rgb(0x005FB8),
        selection: rgb(0xADD6FF), // (default)
        selection_fg: rgb(0x005FB8),
        fg: rgb(0x3B3B3B),
        weak_fg: rgb(0x757575), // (blend)
        link: rgb(0x005FB8),
        error: rgb(0xF85149),
    },
    Palette {
        id: Cow::Borrowed("dark-plus"),
        label: Cow::Borrowed("Dark+"),
        source: Source::Editor,
        dark: true,
        accents: accents(0x4EC9B0, 0x569CD6, 0xF44747),
        bg: rgb(0x1E1E1E),
        faint_bg: rgb(0x252526),  // (default)
        input_bg: rgb(0x3C3C3C),  // (default)
        widget_bg: rgb(0x3C3C3C), // (default) dropdown.background
        border: rgb(0x303031),
        input_border: rgb(0x6B6B6B), // checkbox.border
        hover_bg: rgb(0x0E639C),     // (default)
        hover_fg: rgb(0xFFFFFF),     // (default)
        focus: rgb(0x007FD4),        // (default)
        selection: rgb(0x0078D4),
        selection_fg: rgb(0xFFFFFF),
        fg: rgb(0xD4D4D4),
        weak_fg: rgb(0x9E9E9E), // (blend)
        link: rgb(0x3794FF),    // (default)
        error: rgb(0xF48771),   // (default)
    },
    Palette {
        id: Cow::Borrowed("light-plus"),
        label: Cow::Borrowed("Light+"),
        source: Source::Editor,
        dark: false,
        accents: accents(0x008000, 0x0070C1, 0xCD3131),
        bg: rgb(0xFFFFFF),
        faint_bg: rgb(0xF3F3F3), // (default)
        input_bg: rgb(0xFFFFFF), // (default)
        widget_bg: rgb(0xE8E8E8),
        border: rgb(0xD4D4D4),
        input_border: rgb(0x919191), // checkbox.border
        hover_bg: rgb(0xADD6FF),     // (default)
        hover_fg: rgb(0x000000),
        focus: rgb(0x0090F1),     // (default)
        selection: rgb(0xADD6FF), // (default)
        selection_fg: rgb(0x0451A5),
        fg: rgb(0x000000),
        weak_fg: rgb(0x717171), // (default)
        link: rgb(0x006AB1),    // (default)
        error: rgb(0xA1260D),   // (default)
    },
    Palette {
        id: Cow::Borrowed("monokai"),
        label: Cow::Borrowed("Monokai"),
        source: Source::Editor,
        dark: true,
        accents: accents(0xA6E22E, 0x66D9EF, 0xF92672),
        bg: rgb(0x272822),
        faint_bg: rgb(0x1E1F1C),
        input_bg: rgb(0x414339),
        widget_bg: rgb(0x414339),
        border: rgb(0x414339),
        input_border: rgb(0x75715E),
        hover_bg: rgb(0x75715E),
        hover_fg: rgb(0xF8F8F2),
        focus: rgb(0x99947C),
        selection: rgb(0x75715E),
        selection_fg: rgb(0xF8F8F2),
        fg: rgb(0xF8F8F2),
        weak_fg: rgb(0x90908A),
        link: rgb(0x66D9EF),
        error: rgb(0xF92672),
    },
    Palette {
        id: Cow::Borrowed("solarized-dark"),
        label: Cow::Borrowed("Solarized Dark"),
        source: Source::Editor,
        dark: true,
        accents: accents(0x859900, 0x268BD2, 0xDC322F),
        bg: rgb(0x002B36),
        faint_bg: rgb(0x00212B),
        input_bg: rgb(0x003847),
        widget_bg: rgb(0x00212B),
        border: rgb(0x2B2B4A),
        input_border: rgb(0x197271), // (alpha) checkbox.border
        hover_bg: rgb(0x197271),     // (alpha)
        hover_fg: rgb(0xFFFFFF),     // (default)
        focus: rgb(0x197271),        // (alpha)
        selection: rgb(0x005A6F),
        selection_fg: rgb(0xFFFFFF), // (default)
        fg: rgb(0x839496),
        weak_fg: rgb(0x5C7579), // (blend)
        link: rgb(0x3794FF),    // (default)
        error: rgb(0xFFEAEA),
    },
    Palette {
        id: Cow::Borrowed("solarized-light"),
        label: Cow::Borrowed("Solarized Light"),
        source: Source::Editor,
        dark: false,
        accents: accents(0x859900, 0x268BD2, 0xDC322F),
        bg: rgb(0xFDF6E3),
        faint_bg: rgb(0xEEE8D5),
        input_bg: rgb(0xDDD6C1),
        widget_bg: rgb(0xFFFBF2), // peekViewEditor.background, for contrast
        border: rgb(0xDDD6C1),
        input_border: rgb(0xD3AF86), // checkbox.border
        hover_bg: rgb(0xDFCA88),
        hover_fg: rgb(0x000000),
        focus: rgb(0xB49471),
        selection: rgb(0xDFCA88),
        selection_fg: rgb(0x073642), // terminal.ansiBlack, for contrast
        fg: rgb(0x657B83),
        weak_fg: rgb(0x717171), // (default)
        link: rgb(0x006AB1),    // (default)
        error: rgb(0xA1260D),   // (default)
    },
    Palette {
        id: Cow::Borrowed("abyss"),
        label: Cow::Borrowed("Abyss"),
        source: Source::Editor,
        dark: true,
        accents: accents(0x22AA44, 0x2277FF, 0xDC322F),
        bg: rgb(0x000C18),
        faint_bg: rgb(0x060621),
        input_bg: rgb(0x181F2F),
        widget_bg: rgb(0x181F2F),
        border: rgb(0x2B2B4A),
        input_border: rgb(0x2B2B4A), // agentsChatInput.border
        hover_bg: rgb(0x2B3C5D),
        hover_fg: rgb(0xFFFFFF), // (default)
        focus: rgb(0x596F99),
        selection: rgb(0x08286B),
        selection_fg: rgb(0xFFFFFF), // (default)
        fg: rgb(0x6688CC),
        weak_fg: rgb(0x486396), // (blend)
        link: rgb(0x3794FF),    // (default)
        error: rgb(0xF48771),   // (default)
    },
    Palette {
        id: Cow::Borrowed("kimbie-dark"),
        label: Cow::Borrowed("Kimbie Dark"),
        source: Source::Editor,
        dark: true,
        accents: accents(0x889B4A, 0x8AB1B0, 0xDC3958),
        bg: rgb(0x221A0F),
        faint_bg: rgb(0x362712),
        input_bg: rgb(0x51412C),
        widget_bg: rgb(0x51412C),
        border: rgb(0x5E452B),
        input_border: rgb(0x5E452B), // agentsChatInput.border
        hover_bg: rgb(0x6E583B),
        hover_fg: rgb(0xFFFFFF), // (default)
        focus: rgb(0xA57A4C),
        selection: rgb(0x7C5021),
        selection_fg: rgb(0xFFFFFF), // (default)
        fg: rgb(0xD3AF86),
        weak_fg: rgb(0x9E8363), // (blend)
        link: rgb(0x3794FF),    // (default)
        error: rgb(0xF48771),   // (default)
    },
    Palette {
        id: Cow::Borrowed("quiet-light"),
        label: Cow::Borrowed("Quiet Light"),
        source: Source::Editor,
        dark: false,
        accents: accents(0x448C27, 0x4B69C6, 0xCD3131),
        bg: rgb(0xF5F5F5),
        faint_bg: rgb(0xF2F2F2),
        input_bg: rgb(0xFFFFFF), // (default)
        widget_bg: rgb(0xE0E0E0),
        border: rgb(0xC9D0D9),
        input_border: rgb(0xCECECE), // (default) checkbox.border
        hover_bg: rgb(0xC9D0D9),
        hover_fg: rgb(0x000000),
        focus: rgb(0x9769DC),
        selection: rgb(0xC9D0D9),
        selection_fg: rgb(0x333333), // editor foreground, for contrast
        fg: rgb(0x333333),           // (default)
        weak_fg: rgb(0x717171),      // (default)
        link: rgb(0x006AB1),         // (default)
        error: rgb(0xF1897F),
    },
    Palette {
        id: Cow::Borrowed("red"),
        label: Cow::Borrowed("Red"),
        source: Source::Editor,
        dark: true,
        accents: accents(0x41A83E, 0x1B81A8, 0xF12727),
        bg: rgb(0x390000),
        faint_bg: rgb(0x330000),
        input_bg: rgb(0x580000),
        widget_bg: rgb(0x580000),
        border: rgb(0x580000),
        input_border: rgb(0x611414), // (alpha) agentsChatInput.border
        hover_bg: rgb(0x883333),
        hover_fg: rgb(0xFFFFFF), // (default)
        focus: rgb(0xBD4444),    // (alpha)
        selection: rgb(0x880000),
        selection_fg: rgb(0xFFFFFF), // (default)
        fg: rgb(0xF8F8F8),
        weak_fg: rgb(0xBFAEAE), // (blend)
        link: rgb(0x3794FF),    // (default)
        error: rgb(0xFFEAEA),
    },
    Palette {
        id: Cow::Borrowed("tomorrow-night-blue"),
        label: Cow::Borrowed("Tomorrow Night Blue"),
        source: Source::Editor,
        dark: true,
        accents: accents(0xD1F1A9, 0xBBDAFF, 0xFF9DA4),
        bg: rgb(0x002451),
        faint_bg: rgb(0x001C40),
        input_bg: rgb(0x001733),
        widget_bg: rgb(0x001733),
        border: rgb(0x404F7D),
        input_border: rgb(0x404F7D), // agentsChatInput.border
        hover_bg: rgb(0x0E639C),     // (default)
        hover_fg: rgb(0xFFFFFF),     // (default)
        focus: rgb(0xBBDAFF),
        selection: rgb(0x607693),    // (alpha)
        selection_fg: rgb(0xFFFFFF), // (default)
        fg: rgb(0xFFFFFF),
        weak_fg: rgb(0xB3BECB), // (blend)
        link: rgb(0x3794FF),    // (default)
        error: rgb(0xA92049),
    },
    Palette {
        id: Cow::Borrowed("hc-dark"),
        label: Cow::Borrowed("High Contrast Dark"),
        source: Source::Editor,
        dark: true,
        accents: accents(0x4EC9B0, 0x569CD6, 0xF44747),
        bg: rgb(0x000000),
        faint_bg: rgb(0x000000),     // (default)
        input_bg: rgb(0x000000),     // (default)
        widget_bg: rgb(0x000000),    // (default) dropdown.background
        border: rgb(0x6FC3DF),       // (default) contrastBorder
        input_border: rgb(0x6FC3DF), // (default) contrastBorder
        hover_bg: rgb(0x000000),     // (default)
        hover_fg: rgb(0xFFFFFF),     // (default)
        focus: rgb(0xF38518),        // (default)
        selection: rgb(0x008000),
        selection_fg: rgb(0xFFFFFF), // white, not black, for the focus ring
        fg: rgb(0xFFFFFF),
        weak_fg: rgb(0xB3B3B3), // (blend)
        link: rgb(0x21A6FF),    // (default)
        error: rgb(0xF48771),   // (default)
    },
    Palette {
        id: Cow::Borrowed("hc-light"),
        label: Cow::Borrowed("High Contrast Light"),
        source: Source::Editor,
        dark: false,
        accents: accents(0x096D48, 0x0451A5, 0xB5200D),
        bg: rgb(0xFFFFFF),           // (default)
        faint_bg: rgb(0xFFFFFF),     // (default)
        input_bg: rgb(0xFFFFFF),     // (default)
        widget_bg: rgb(0xFFFFFF),    // (default)
        border: rgb(0x0F4A85),       // (default) contrastBorder
        input_border: rgb(0x0F4A85), // (default) contrastBorder
        hover_bg: rgb(0xE7EDF3),     // (default) (alpha)
        hover_fg: rgb(0x000000),
        focus: rgb(0x006BBD),        // (default)
        selection: rgb(0xE7EDF3),    // (default) (alpha)
        selection_fg: rgb(0x0F4A85), // contrastBorder, for contrast
        fg: rgb(0x292929),           // (default)
        weak_fg: rgb(0x696969),      // (blend)
        link: rgb(0x0F4A85),         // (default)
        error: rgb(0xB5200D),        // (default)
    },
];
