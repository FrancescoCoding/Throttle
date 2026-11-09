//! egui/eframe front-end for Throttle.
//!
//! Consumes `Snapshot`s from the backend (~1/sec) and emits `Command`s back.
//! This file is built against the shared contract in `crate::types` and must
//! not depend on any backend internals beyond the `snapshot_rx` receiver.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::plot::{self, PlotHint};
use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use eframe::egui::Widget;
use tray_icon::{Icon, TrayIcon};

use crate::tray::{self, TrayAction, TrayRule};

use crate::backend::rules;
use crate::backend::synthetic::{is_shapable, is_system};
use crate::settings::{self, Settings, SortColumn, View};
use crate::types::{Command, ProcessStats, Rule, Snapshot};
use crate::units::{GB, KB, MB, humanize_bytes, humanize_rate, settle};
use crate::update_check::{self, UpdateInfo};
use crate::{appearance, autostart, icon, single_instance, theme};

/// How many total-rate samples we keep for the header graph by default; the
/// View menu changes it through `View::history_secs`.
const TOTAL_HISTORY_LEN: usize = 120;

/// Header plot history choices in the View menu: `(seconds, label)`.
const HISTORY_CHOICES: [(usize, &str); 4] = [
    (60, "1 minute"),
    (120, "2 minutes"),
    (300, "5 minutes"),
    (600, "10 minutes"),
];

/// Plot line width choices in the View menu: `(points, label)`.
const LINE_WIDTH_CHOICES: [(f32, &str); 3] = [(1.0, "1"), (2.0, "2"), (3.0, "3")];

/// Project links used by the Help menu and the About dialog.
const REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");
/// Where to support the project, linked from Help and About.
const SUPPORT_URL: &str = "https://buymeacoffee.com/franwbu";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Version of the bundled WinDivert driver (see `vendor/`).
const WINDIVERT_VERSION: &str = "2.2.2";

/// How long a start in the tray waits for the taskbar before staying visible.
const START_HIDDEN_WAIT: Duration = Duration::from_secs(60);

/// Width reserved for the folder button in the detail panel header.
const FOLDER_BUTTON_WIDTH: f32 = 44.0;

// ---------------------------------------------------------------------------
// Formatting / parsing helpers
// ---------------------------------------------------------------------------

/// Rate unit for the limit dialog's dropdown (base 1024).
#[derive(Clone, Copy, PartialEq, Eq)]
enum RateUnit {
    Bps,
    KBps,
    MBps,
    GBps,
}

impl RateUnit {
    const ALL: [RateUnit; 4] = [
        RateUnit::Bps,
        RateUnit::KBps,
        RateUnit::MBps,
        RateUnit::GBps,
    ];

    fn label(self) -> &'static str {
        match self {
            RateUnit::Bps => "B/s",
            RateUnit::KBps => "KB/s",
            RateUnit::MBps => "MB/s",
            RateUnit::GBps => "GB/s",
        }
    }

    fn multiplier(self) -> f64 {
        match self {
            RateUnit::Bps => 1.0,
            RateUnit::KBps => KB,
            RateUnit::MBps => MB,
            RateUnit::GBps => GB,
        }
    }

    /// Split a byte rate into (value, unit) using the largest unit >= 1.
    fn decompose(bps: u64) -> (f64, RateUnit) {
        let b = bps as f64;
        if b >= GB {
            (b / GB, RateUnit::GBps)
        } else if b >= MB {
            (b / MB, RateUnit::MBps)
        } else if b >= KB {
            (b / KB, RateUnit::KBps)
        } else {
            (b, RateUnit::Bps)
        }
    }
}

/// What may stay in the limit field: digits and one decimal separator (a
/// dot or a comma), so letters typed or pasted in disappear at once.
fn numeric_input(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut separator = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            out.push(c);
        } else if (c == '.' || c == ',') && !separator {
            separator = true;
            out.push(c);
        }
    }
    out
}

/// One-line summary of a rule for the table column. A disabled rule reads
/// "Off: " plus what it would do when switched back on.
pub(crate) fn rule_summary(rule: Option<&Rule>) -> String {
    let Some(r) = rule else {
        return "-".to_string();
    };
    let summary = if r.blocked {
        "BLOCKED".to_string()
    } else {
        let mut parts = Vec::new();
        if let Some(d) = r.down_limit {
            parts.push(format!("Dn {}", humanize_rate(d)));
        }
        if let Some(u) = r.up_limit {
            parts.push(format!("Up {}", humanize_rate(u)));
        }
        if parts.is_empty() {
            return "-".to_string();
        }
        parts.join("  ")
    };
    if r.disabled {
        format!("Off: {summary}")
    } else {
        summary
    }
}

/// Whether the process moved any data in the last second.
fn is_active(stats: &ProcessStats) -> bool {
    stats.down_rate > 0 || stats.up_rate > 0
}

/// Whether `stats` matches the filter box text: a case-insensitive
/// substring of the process name or its path. Blank matches everything.
fn matches_filter(stats: &ProcessStats, filter: &str) -> bool {
    let needle = filter.trim().to_lowercase();
    needle.is_empty()
        || stats.name.to_lowercase().contains(&needle)
        || stats.exe_path.to_lowercase().contains(&needle)
}

// ---------------------------------------------------------------------------
// Sorting / modal state
// ---------------------------------------------------------------------------

/// A column of the process table, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableColumn {
    Name,
    Flows,
    Down,
    Up,
    DownTotal,
    UpTotal,
    Rule,
}

impl TableColumn {
    const ALL: [TableColumn; 7] = [
        TableColumn::Name,
        TableColumn::Flows,
        TableColumn::Down,
        TableColumn::Up,
        TableColumn::DownTotal,
        TableColumn::UpTotal,
        TableColumn::Rule,
    ];

    fn label(self) -> &'static str {
        match self {
            TableColumn::Name => "Process",
            TableColumn::Flows => "Flows",
            TableColumn::Down => "Down",
            TableColumn::Up => "Up",
            TableColumn::DownTotal => "Total down",
            TableColumn::UpTotal => "Total up",
            TableColumn::Rule => "Rule",
        }
    }

    /// Stable key for the column's saved width in the settings file.
    fn id(self) -> &'static str {
        match self {
            TableColumn::Name => "name",
            TableColumn::Flows => "flows",
            TableColumn::Down => "down",
            TableColumn::Up => "up",
            TableColumn::DownTotal => "down_total",
            TableColumn::UpTotal => "up_total",
            TableColumn::Rule => "rule",
        }
    }

    /// The sort key behind the header, if the column sorts.
    fn sort(self) -> Option<SortColumn> {
        match self {
            TableColumn::Name => Some(SortColumn::Name),
            TableColumn::Flows => Some(SortColumn::Flows),
            TableColumn::Down => Some(SortColumn::Down),
            TableColumn::Up => Some(SortColumn::Up),
            TableColumn::DownTotal => Some(SortColumn::DownTotal),
            TableColumn::UpTotal => Some(SortColumn::UpTotal),
            TableColumn::Rule => None,
        }
    }

    /// Whether the View menu shows the column. The process name always shows.
    fn visible(self, view: &View) -> bool {
        match self {
            TableColumn::Name => true,
            TableColumn::Flows => view.show_flows,
            TableColumn::Down => view.show_down,
            TableColumn::Up => view.show_up,
            TableColumn::DownTotal => view.show_down_total,
            TableColumn::UpTotal => view.show_up_total,
            TableColumn::Rule => view.show_rule,
        }
    }
}

/// The table columns `view` shows, in display order.
fn visible_columns(view: &View) -> Vec<TableColumn> {
    TableColumn::ALL
        .into_iter()
        .filter(|c| c.visible(view))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Down,
    Up,
}

/// State for the small "set limit" modal window.
struct LimitModal {
    exe_path: String,
    name: String,
    direction: Direction,
    /// Numeric value as typed (empty = unlimited).
    value: String,
    unit: RateUnit,
    error: Option<String>,
}

// ---------------------------------------------------------------------------
// The app
// ---------------------------------------------------------------------------

pub struct ThrottleApp {
    snapshot_rx: Receiver<Snapshot>,
    cmd_tx: Sender<Command>,

    last_snapshot: Snapshot,
    total_down_hist: Vec<f64>,
    total_up_hist: Vec<f64>,

    selected: Option<String>,
    /// Text of the filter box above the table. Not persisted.
    filter: String,
    /// Ctrl+F was pressed: focus the filter box when it is drawn.
    focus_filter: bool,

    limit_modal: Option<LimitModal>,
    about_open: bool,
    shutdown_sent: bool,

    settings: Settings,
    /// Accent colours of the active theme (plots, rates, blocked rules).
    accents: theme::Accents,
    /// Palettes imported from colour theme files (`settings.custom_themes`).
    custom_palettes: Vec<theme::Palette>,
    /// The Customise window is open.
    customise_open: bool,
    /// Why the last theme import failed; shown until dismissed.
    import_error: Option<String>,
    /// No frame has run yet, so the window level is still to be applied.
    first_frame: bool,
    /// Last frame's state of the header plot, for the hint in the totals line.
    total_plot_hint: PlotHint,
    /// Last frame's state of the detail plot, for the hint by its heading.
    detail_plot_hint: PlotHint,
    /// `None` until the background check of the startup task reports back.
    autostart: Option<bool>,
    autostart_rx: Receiver<bool>,
    autostart_error: Option<String>,

    /// `None` if the tray icon could not be created; the window then never hides.
    tray: Option<TrayIcon>,
    tray_rx: Receiver<TrayAction>,
    /// The rule toggles the tray menu currently shows, to rebuild it on change.
    tray_rules: Vec<TrayRule>,
    /// The tray tooltip currently set, to update it only when the totals change.
    tray_tooltip: String,
    /// The window is hidden to the tray (no taskbar button).
    hidden: bool,
    /// Whether the window was minimised last frame, to catch the moment it is.
    was_minimized: bool,
    /// Hide to the tray once the taskbar exists (`--tray` or "Start minimised"),
    /// unless it has not appeared by this deadline.
    start_hidden: Option<Instant>,
    /// A real exit is under way, so a close request must not go to the tray.
    exiting: bool,

    update_rx: Receiver<UpdateInfo>,
    update: Option<UpdateInfo>,
    update_dismissed: bool,
    update_install: UpdateInstall,
}

/// Progress of an in-app update started from the banner.
enum UpdateInstall {
    Idle,
    Running(Receiver<Result<std::path::PathBuf, String>>),
    Failed(String),
}

impl ThrottleApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        snapshot_rx: Receiver<Snapshot>,
        cmd_tx: Sender<Command>,
        settings: Settings,
        start_hidden: bool,
    ) -> Self {
        let (tray_tx, tray_rx) = crossbeam_channel::unbounded();
        // The tray starts with the stock icon; `restyle` below recolours it
        // when the theme's accents differ from the stock ones.
        let tray = tray::create_tray(&cc.egui_ctx, tray_tx.clone());
        let ctx = cc.egui_ctx.clone();
        single_instance::on_show_request(move || {
            let _ = tray_tx.send(TrayAction::Show);
            ctx.request_repaint();
        });
        let custom_palettes = settings
            .custom_themes
            .iter()
            .map(theme::CustomTheme::to_palette)
            .collect();
        let mut app = Self {
            snapshot_rx,
            cmd_tx,
            last_snapshot: Snapshot::default(),
            total_down_hist: Vec::with_capacity(TOTAL_HISTORY_LEN),
            total_up_hist: Vec::with_capacity(TOTAL_HISTORY_LEN),
            selected: None,
            filter: String::new(),
            focus_filter: false,
            limit_modal: None,
            about_open: false,
            shutdown_sent: false,
            settings,
            accents: theme::Accents::DEFAULT,
            custom_palettes,
            customise_open: false,
            import_error: None,
            first_frame: true,
            total_plot_hint: PlotHint::default(),
            detail_plot_hint: PlotHint::default(),
            autostart: None,
            autostart_rx: spawn_autostart_check(),
            autostart_error: None,
            tray,
            tray_rx,
            tray_rules: Vec::new(),
            tray_tooltip: String::new(),
            hidden: false,
            was_minimized: false,
            start_hidden: start_hidden.then(|| Instant::now() + START_HIDDEN_WAIT),
            exiting: false,
            update_rx: update_check::spawn(APP_VERSION),
            update: None,
            update_dismissed: false,
            update_install: UpdateInstall::Idle,
        };
        app.restyle(&cc.egui_ctx);
        app
    }

    /// Apply the theme and appearance settings to `ctx`, take the accents
    /// they give and recolour the tray icon to match. Runs at startup and
    /// after every change from the Themes menu, the Customise window or an
    /// import.
    fn restyle(&mut self, ctx: &egui::Context) {
        let mut accents = theme::apply(ctx, &self.settings.theme, &self.custom_palettes);
        // Always after `theme::apply`, which resets both stock visuals.
        appearance::apply(ctx, &self.settings.appearance);
        if let Some(o) = self.settings.appearance.accents() {
            accents = o;
        }
        let icon_changed = (accents.down, accents.up) != (self.accents.down, self.accents.up);
        self.accents = accents;
        if icon_changed {
            self.refresh_tray_icon();
        }
    }

    /// Redraw the tray icon in the current accents. Best effort: a failure
    /// is logged and the old icon stays.
    fn refresh_tray_icon(&self) {
        let Some(tray) = &self.tray else {
            return;
        };
        let result = Icon::from_rgba(tray::icon_rgba(self.accents), icon::SIZE, icon::SIZE)
            .map_err(|e| e.to_string())
            .and_then(|icon| tray.set_icon(Some(icon)).map_err(|e| e.to_string()));
        if let Err(e) = result {
            tracing::warn!("failed to recolour the tray icon: {e}");
        }
    }

    /// After a new snapshot: put the live totals in the tray tooltip and
    /// rebuild the tray menu when the rules it lists changed. Runs while
    /// hidden too, which is when the tray is what the user sees.
    fn refresh_tray(&mut self, ctx: &egui::Context) {
        let Some(tray) = &self.tray else {
            return;
        };
        let snap = &self.last_snapshot;
        let tooltip = tray::tooltip(snap);
        if tooltip != self.tray_tooltip {
            if let Err(e) = tray.set_tooltip(Some(&tooltip)) {
                tracing::warn!("failed to update the tray tooltip: {e}");
            }
            self.tray_tooltip = tooltip;
        }

        let rules = tray::rules(snap);
        if rules != self.tray_rules {
            tray.set_menu(Some(Box::new(tray::menu(&rules))));
            self.tray_rules = rules;
            ctx.request_repaint();
        }
    }

    /// Send the window level the View menu asks for.
    fn send_window_level(&self, ctx: &egui::Context) {
        let level = if self.settings.view.always_on_top {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
    }

    fn send_shutdown(&mut self) {
        if !self.shutdown_sent {
            let _ = self.cmd_tx.send(Command::Shutdown);
            self.shutdown_sent = true;
        }
    }

    /// Quit for real, even if "Close to tray" is on.
    fn exit(&mut self, ctx: &egui::Context) {
        self.exiting = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        // Without a tray icon there would be no way back to a hidden window.
        if self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.hidden = true;
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        // Showing a hidden window can drop it back to the normal level.
        if self.settings.view.always_on_top {
            self.send_window_level(ctx);
        }
        self.hidden = false;
    }

    fn save_settings(&self) {
        if let Err(e) = settings::save(&self.settings) {
            tracing::warn!("failed to persist settings: {e:#}");
        }
    }

    fn set_autostart(&mut self, enabled: bool) {
        let result = if enabled {
            std::env::current_exe()
                .map_err(anyhow::Error::from)
                .and_then(|exe| autostart::enable(&exe))
        } else {
            autostart::disable()
        };
        match result {
            Ok(()) => {
                self.autostart = Some(enabled);
                self.autostart_error = None;
            }
            Err(e) => {
                tracing::warn!("failed to update the startup task: {e:#}");
                self.autostart_error = Some(format!("Could not update the startup task: {e:#}"));
            }
        }
    }

    /// The current rule for `key`, or a fresh default rule bound to that path.
    fn base_rule(&self, key: &str) -> Rule {
        self.last_snapshot
            .rules
            .get(key)
            .cloned()
            .unwrap_or_else(|| Rule {
                exe_path: key.to_string(),
                ..Rule::default()
            })
    }

    fn send_rule(&mut self, rule: Rule) {
        let _ = self.cmd_tx.send(Command::SetRule(rule));
    }

    /// Block or unblock; either switches a disabled rule back on, since the
    /// user just asked for it to do something.
    fn set_blocked(&mut self, key: &str, blocked: bool) {
        let mut rule = self.base_rule(key);
        rule.exe_path = key.to_string();
        rule.blocked = blocked;
        rule.disabled = false;
        self.send_rule(rule);
    }

    /// Switch a rule off (kept, not applied) or back on. No rule: nothing.
    fn set_rule_disabled(&mut self, key: &str, disabled: bool) {
        if let Some(rule) = self.last_snapshot.rules.get(key) {
            let mut rule = rule.clone();
            rule.disabled = disabled;
            self.send_rule(rule);
        }
    }

    fn open_limit_modal(&mut self, key: &str, name: &str, direction: Direction) {
        let existing = self.last_snapshot.rules.get(key);
        let current = existing.and_then(|r| match direction {
            Direction::Down => r.down_limit,
            Direction::Up => r.up_limit,
        });
        let (value, unit) = match current {
            Some(bps) => {
                let (v, u) = RateUnit::decompose(bps);
                // Trim trailing zeros for a clean editable number.
                let s = format!("{v:.2}");
                let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
                (s, u)
            }
            None => (String::new(), RateUnit::MBps),
        };
        self.limit_modal = Some(LimitModal {
            exe_path: key.to_string(),
            name: name.to_string(),
            direction,
            value,
            unit,
            error: None,
        });
    }

    /// Adjust the style to make sort headers look less like regular buttons.
    /// This only affects the local Ui (the Grid), not Menus or Modals.
    /// TODO: Looks ugly and could affect unrelated buttons, replace with "classes" in egui 0.37+:
    ///   https://github.com/emilk/egui/pull/8153
    ///   https://github.com/emilk/egui/blob/7ba3db/examples/styling_engine/src/main.rs
    fn sort_header_button_style(&self, ui: &mut egui::Ui, col_selected: bool) {
        let style = ui.style_mut();
        let widgets_style = &mut style.visuals.widgets;

        let transparent = egui::Color32::TRANSPARENT;
        // Use a gray-ish luminance (0.3) with variable opacity -> works for light and dark theme
        let selected = egui::Rgba::from_luminance_alpha(0.3, 0.15).into();
        let hovered = egui::Rgba::from_luminance_alpha(0.3, 0.25).into();

        widgets_style.active.weak_bg_fill = hovered; // "active" = while being clicked
        widgets_style.hovered.weak_bg_fill = hovered;
        widgets_style.inactive.weak_bg_fill = if col_selected { selected } else { transparent };

        widgets_style.active.bg_stroke = egui::Stroke::NONE;
        widgets_style.hovered.bg_stroke = egui::Stroke::NONE;
        widgets_style.inactive.bg_stroke = egui::Stroke::NONE;
    }

    /// Adjust the sort state when a header is clicked.
    fn sort_header(&mut self, ui: &mut egui::Ui, label: &str, col: SortColumn) {
        let text = egui::RichText::new(label).strong();
        let grow = egui::Atom::grow();
        let col_selected = self.settings.table.sort == col;

        let direction_text = if !col_selected {
            ""
        } else if self.settings.table.descending {
            "\u{25bc}"
        } else {
            "\u{25b2}"
        };
        // The default font is missing the arrows, use the bundled monospace font (Hack) instead
        let direction = egui::RichText::new(direction_text).monospace();

        self.sort_header_button_style(ui, col_selected);
        let button = egui::Button::new((text, grow))
            .right_text(direction)
            .min_size(ui.available_size())
            .corner_radius(0);

        if ui.add(button).clicked() {
            let table = &mut self.settings.table;
            if col_selected {
                table.descending = !table.descending;
            } else {
                table.sort = col;
                table.descending = true;
            }
            self.save_settings();
        }
    }

    fn draw_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Import theme...").clicked() {
                        ui.close();
                        self.import_theme(ui.ctx());
                    }
                    ui.separator();
                    if ui.button("Open rules file").clicked() {
                        open_path(&rules::rules_path());
                        ui.close();
                    }
                    if ui.button("Open log file").clicked() {
                        open_path(&rules::config_dir().join("throttle.log"));
                        ui.close();
                    }
                    if ui.button("Open data folder").clicked() {
                        open_path(&rules::config_dir());
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        self.exit(ui.ctx());
                        ui.close();
                    }
                });
                ui.menu_button("Settings", |ui| self.draw_settings_menu(ui));
                // Menus close on any click by default; this one stays open
                // so palettes can be cycled through, and its submenus inherit
                // that. Surprise me and Customise close it explicitly.
                let stay_open = egui::containers::menu::MenuConfig::new()
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                egui::containers::menu::MenuButton::new("Themes")
                    .config(stay_open)
                    .ui(ui, |ui| self.draw_themes_menu(ui));
                ui.menu_button("View", |ui| self.draw_view_menu(ui));
                ui.menu_button("Help", |ui| {
                    if ui.button("Documentation").clicked() {
                        open_url(ui.ctx(), &format!("{REPO_URL}#readme"));
                        ui.close();
                    }
                    if ui.button("Report issue").clicked() {
                        open_url(ui.ctx(), &format!("{REPO_URL}/issues/new"));
                        ui.close();
                    }
                    let update_label = match &self.update {
                        Some(u) => format!("Update available: v{}\u{2026}", u.version),
                        None => "Check for updates\u{2026}".to_string(),
                    };
                    if ui.button(update_label).clicked() {
                        open_url(ui.ctx(), &format!("{REPO_URL}/releases/latest"));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("View license").clicked() {
                        open_url(ui.ctx(), &format!("{REPO_URL}/blob/main/LICENSE"));
                        ui.close();
                    }
                    if ui.button("Third party notices").clicked() {
                        open_url(
                            ui.ctx(),
                            &format!("{REPO_URL}/blob/main/THIRD_PARTY_NOTICES.md"),
                        );
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Buy me a coffee").clicked() {
                        open_url(ui.ctx(), SUPPORT_URL);
                        ui.close();
                    }
                    if ui.button("About Throttle").clicked() {
                        self.about_open = true;
                        ui.close();
                    }
                });
            });
        });
    }

    fn draw_settings_menu(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.add_enabled_ui(self.tray.is_some(), |ui| {
            changed |= ui
                .checkbox(&mut self.settings.minimize_to_tray, "Minimise to tray")
                .on_hover_text("Minimising hides Throttle to the tray instead of the taskbar.")
                .changed();
            changed |= ui
                .checkbox(&mut self.settings.close_to_tray, "Close to tray")
                .on_hover_text("The X button hides Throttle to the tray. File > Exit still quits.")
                .changed();
            changed |= ui
                .checkbox(&mut self.settings.start_minimized, "Start minimised")
                .on_hover_text("Open in the tray instead of a window.")
                .changed();
        })
        .response
        .on_disabled_hover_text("The system tray is unavailable.");
        if changed {
            self.save_settings();
        }

        ui.separator();
        let mut autostart = self.autostart.unwrap_or(false);
        let toggled = ui
            .add_enabled(
                self.autostart.is_some(),
                egui::Checkbox::new(&mut autostart, "Launch at startup"),
            )
            .on_hover_text("Start Throttle in the tray when you sign in to Windows.")
            .changed();
        if toggled {
            self.set_autostart(autostart);
        }
        if let Some(e) = &self.autostart_error {
            ui.label(egui::RichText::new(e).small().color(self.accents.block));
        }
    }

    fn draw_themes_menu(&mut self, ui: &mut egui::Ui) {
        for (id, label) in theme::BUILTIN {
            self.theme_row(ui, id, label, None);
        }
        ui.separator();
        type InGroup = fn(&theme::Palette) -> bool;
        let groups: [(&str, InGroup); 3] = [
            ("Editor", |p| p.source == theme::Source::Editor),
            ("Dark", |p| p.source == theme::Source::Community && p.dark),
            ("Light", |p| p.source == theme::Source::Community && !p.dark),
        ];
        for (title, in_group) in groups {
            ui.menu_button(title, |ui| {
                for palette in theme::builtin().filter(|p| in_group(p)) {
                    self.theme_row(ui, &palette.id, &palette.label, Some(palette));
                }
            });
        }
        if !self.custom_palettes.is_empty() {
            ui.menu_button("Imported", |ui| self.draw_imported_menu(ui));
        }
        ui.separator();
        if ui.button("Surprise me").clicked() {
            self.surprise_me(ui.ctx());
            ui.close();
        }
        if ui.button("Customise...").clicked() {
            self.customise_open = true;
            ui.close();
        }
    }

    /// The Imported submenu: one row per imported palette, then a Remove
    /// submenu listing them again.
    fn draw_imported_menu(&mut self, ui: &mut egui::Ui) {
        // Cloned so the rows can borrow `self` mutably; the list is short.
        let custom = self.custom_palettes.clone();
        for palette in &custom {
            self.theme_row(ui, &palette.id, &palette.label, Some(palette));
        }
        ui.separator();
        ui.menu_button("Remove", |ui| {
            for palette in &custom {
                if ui.button(palette.label.as_ref()).clicked() {
                    self.remove_custom_theme(ui.ctx(), &palette.id);
                    ui.close();
                }
            }
        });
    }

    /// One Themes menu row: a swatch for palettes, then the radio label. An
    /// unknown stored id simply leaves no row checked. Clicking applies the
    /// theme but leaves the menu open.
    fn theme_row(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        label: &str,
        swatch: Option<&theme::Palette>,
    ) {
        let selected = self.settings.theme == id;
        let clicked = match swatch {
            Some(palette) => {
                ui.horizontal(|ui| {
                    draw_swatch(ui, palette);
                    ui.radio(selected, label).clicked()
                })
                .inner
            }
            None => ui.radio(selected, label).clicked(),
        };
        // The Themes menu is set to stay open (see `draw_menu_bar`), so
        // palettes can be cycled through in one go.
        if clicked && !selected {
            self.settings.theme = id.to_string();
            self.restyle(ui.ctx());
            self.save_settings();
        }
    }

    /// Switch to a random bundled palette other than the current theme.
    fn surprise_me(&mut self, ctx: &egui::Context) {
        let others: Vec<&theme::Palette> = theme::builtin()
            .filter(|p| p.id != self.settings.theme)
            .collect();
        if others.is_empty() {
            return;
        }
        let palette = others[random_index(others.len(), time_seed())];
        self.settings.theme = palette.id.to_string();
        self.restyle(ctx);
        self.save_settings();
    }

    /// File > Import theme...: pick a colour theme JSON file, add it to the
    /// imported palettes and switch to it. A failure opens the error window.
    fn import_theme(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Colour theme", &["json"])
            .pick_file()
        else {
            return;
        };
        match theme::import::import(&path) {
            Ok(palette) => self.add_custom_theme(ctx, &palette),
            Err(e) => {
                tracing::warn!("failed to import {}: {e:#}", path.display());
                self.import_error = Some(format!("{e:#}"));
            }
        }
    }

    /// Store `palette` as an imported theme (replacing one with the same id)
    /// and make it the active theme.
    fn add_custom_theme(&mut self, ctx: &egui::Context, palette: &theme::Palette) {
        let mut stored = theme::CustomTheme::from(palette);
        stored.id = custom_theme_id(&stored.id);
        // Round trip through the stored form, so this session shows exactly
        // what the next start will load.
        let palette = stored.to_palette();
        self.settings.custom_themes.retain(|t| t.id != stored.id);
        self.custom_palettes.retain(|p| p.id != palette.id);
        self.settings.theme = stored.id.clone();
        self.settings.custom_themes.push(stored);
        self.custom_palettes.push(palette);
        self.import_error = None;
        self.restyle(ctx);
        self.save_settings();
    }

    /// Forget the imported theme `id`, falling back to the default theme if
    /// it was the active one.
    fn remove_custom_theme(&mut self, ctx: &egui::Context, id: &str) {
        self.settings.custom_themes.retain(|t| t.id != id);
        self.custom_palettes.retain(|p| p.id != id);
        if self.settings.theme == id {
            self.settings.theme = theme::DEFAULT_ID.to_string();
            self.restyle(ctx);
        }
        self.save_settings();
    }

    fn draw_view_menu(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let view = &mut self.settings.view;
        ui.menu_button("Columns", |ui| {
            for (show, label) in [
                (&mut view.show_flows, "Flows"),
                (&mut view.show_down, "Down"),
                (&mut view.show_up, "Up"),
                (&mut view.show_down_total, "Total down"),
                (&mut view.show_up_total, "Total up"),
                (&mut view.show_rule, "Rule"),
            ] {
                changed |= ui.checkbox(show, label).changed();
            }
        });
        ui.menu_button("Plot", |ui| {
            changed |= ui.checkbox(&mut view.plot_fill, "Filled area").changed();
            ui.menu_button("Line width", |ui| {
                for (width, label) in LINE_WIDTH_CHOICES {
                    changed |= ui
                        .radio_value(&mut view.plot_line_width, width, label)
                        .changed();
                }
            });
            ui.menu_button("Header plot history", |ui| {
                for (secs, label) in HISTORY_CHOICES {
                    changed |= ui
                        .radio_value(&mut view.history_secs, secs, label)
                        .changed();
                }
            });
        });
        ui.separator();
        let on_top_changed = ui
            .checkbox(&mut view.always_on_top, "Always on top")
            .changed();
        changed |= on_top_changed;
        changed |= ui
            .checkbox(&mut view.compact, "Compact (hide header plot)")
            .changed();
        changed |= ui.checkbox(&mut view.show_filter, "Filter box").changed();
        if on_top_changed {
            self.send_window_level(ui.ctx());
        }
        if changed {
            self.save_settings();
        }
    }

    /// The Customise window: appearance options on top of the theme.
    fn draw_customise_window(&mut self, ctx: &egui::Context) {
        if !self.customise_open {
            return;
        }
        let mut open = true;
        let changed = egui::Window::new("Customise")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                appearance::customise_ui(ui, &mut self.settings.appearance)
            })
            .and_then(|r| r.inner)
            .unwrap_or(false);
        self.customise_open = open;
        if changed {
            self.restyle(ctx);
            self.save_settings();
        }
    }

    /// The "Import failed" window, shown while `import_error` is set.
    fn draw_import_error(&mut self, ctx: &egui::Context) {
        let Some(message) = &self.import_error else {
            return;
        };
        let mut open = true;
        let mut close = false;
        egui::Window::new("Import failed")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("The colour theme could not be imported.");
                ui.label(egui::RichText::new(message).color(self.accents.block));
                ui.add_space(4.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        if close || !open {
            self.import_error = None;
        }
    }

    fn draw_update_banner(&mut self, ui: &mut egui::Ui) {
        if self.update_dismissed {
            return;
        }
        let Some(update) = self.update.clone() else {
            return;
        };

        // Poll a running install; on success relaunch the new build and quit.
        if let UpdateInstall::Running(rx) = &self.update_install
            && let Ok(result) = rx.try_recv()
        {
            let relaunched = result.and_then(|exe| {
                std::process::Command::new(&exe)
                    .args(["--wait-pid", &std::process::id().to_string()])
                    .spawn()
                    .map_err(|e| format!("could not start the new version: {e}"))
            });
            self.update_install = match relaunched {
                Ok(_) => {
                    self.send_shutdown();
                    self.exit(ui.ctx());
                    UpdateInstall::Idle
                }
                Err(e) => {
                    tracing::error!("update failed: {e}");
                    UpdateInstall::Failed(e)
                }
            };
        }

        let release_page = if update.url.is_empty() {
            format!("{REPO_URL}/releases/latest")
        } else {
            update.url.clone()
        };

        egui::Panel::top("update_banner").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "Throttle v{} is available (you have v{APP_VERSION}).",
                        update.version
                    ))
                    .strong(),
                );
                if let UpdateInstall::Running(_) = self.update_install {
                    ui.spinner();
                    ui.label("Downloading update\u{2026}");
                    return;
                }
                if let UpdateInstall::Failed(e) = &self.update_install {
                    ui.label(
                        egui::RichText::new(format!("Update failed: {e}"))
                            .color(self.accents.block),
                    );
                }
                match &update.asset_url {
                    Some(asset_url) => {
                        if ui.button("Update and restart").clicked() {
                            let (tx, rx) = crossbeam_channel::bounded(1);
                            let asset_url = asset_url.clone();
                            std::thread::spawn(move || {
                                let result =
                                    update_check::install(&asset_url).map_err(|e| format!("{e:#}"));
                                let _ = tx.send(result);
                            });
                            self.update_install = UpdateInstall::Running(rx);
                        }
                        if ui.button("Release notes").clicked() {
                            open_url(ui.ctx(), &release_page);
                        }
                    }
                    None => {
                        if ui.button("Download").clicked() {
                            open_url(ui.ctx(), &release_page);
                        }
                    }
                }
                if ui.button("Dismiss").clicked() {
                    self.update_dismissed = true;
                }
            });
        });
    }

    fn draw_about(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let mut open = true;
        egui::Window::new("About Throttle")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    ui.heading("Throttle");
                    ui.label(egui::RichText::new(format!("Version {APP_VERSION}")).strong());
                    ui.add_space(4.0);
                    ui.label(env!("CARGO_PKG_DESCRIPTION"));
                    ui.add_space(8.0);
                });
                ui.separator();
                egui::Grid::new("about_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Packet driver");
                    ui.label(format!("WinDivert {WINDIVERT_VERSION}"));
                    ui.end_row();
                    ui.label("UI toolkit");
                    ui.label("egui / eframe");
                    ui.end_row();
                    ui.label("License");
                    ui.label(env!("CARGO_PKG_LICENSE"));
                    ui.end_row();
                    ui.label("Data folder");
                    ui.label(
                        egui::RichText::new(rules::config_dir().display().to_string())
                            .weak()
                            .small(),
                    );
                    ui.end_row();
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.hyperlink_to("GitHub", REPO_URL);
                    ui.hyperlink_to("Releases", format!("{REPO_URL}/releases"));
                    ui.hyperlink_to("Report issue", format!("{REPO_URL}/issues"));
                    ui.hyperlink_to("Buy me a coffee", SUPPORT_URL);
                });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Copyright \u{a9} 2026 Francesco Gruosso")
                        .weak()
                        .small(),
                );
            });
        self.about_open = open;
    }

    /// Monospace `text` when the Customise window asks for monospace numbers.
    fn numeric(&self, text: egui::RichText) -> egui::RichText {
        if self.settings.appearance.numbers_monospace {
            text.monospace()
        } else {
            text
        }
    }

    /// The header line: app name, total Down / Up rates and process count.
    fn draw_totals_line(&self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.horizontal(|ui| {
            ui.heading("Throttle");
            ui.separator();
            ui.label(
                self.numeric(
                    egui::RichText::new(format!("Down {}", humanize_rate(snap.total_down_rate)))
                        .color(self.accents.down)
                        .strong(),
                ),
            );
            ui.label(
                self.numeric(
                    egui::RichText::new(format!("Up {}", humanize_rate(snap.total_up_rate)))
                        .color(self.accents.up)
                        .strong(),
                ),
            );
            ui.separator();
            ui.label(format!("{} processes", snap.processes.len()));
            if let Some(text) = self.total_plot_hint.text() {
                ui.separator();
                ui.weak(text);
            }
        });
    }

    fn draw_top_panel(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        if self.settings.view.compact {
            // Only the totals line. Its own id keeps the full panel's
            // resized height for when compact mode is turned off again.
            egui::Panel::top("top_panel_compact").show(ui, |ui| {
                ui.add_space(4.0);
                self.draw_totals_line(ui, snap);
                ui.add_space(4.0);
            });
            return;
        }
        egui::Panel::top("top_panel")
            .resizable(true)
            .default_size(self.settings.panels.plot_height.unwrap_or(120.0))
            .min_size(70.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                self.draw_totals_line(ui, snap);

                // Let the plot fill the panel's remaining height rather than
                // requesting an explicit one. An explicit height derived from
                // available_height() makes the content's reported size disagree
                // with the drag target each frame, which egui persists as the new
                // panel size and turns into a resize feedback loop. Filling avoids
                // that.
                let plot = plot::rate_plot("total_plot").show(ui, |pui| {
                    let view = &self.settings.view;
                    pui.line(plot::plot_line(
                        view,
                        "Down",
                        &self.total_down_hist,
                        self.accents.down,
                    ));
                    pui.line(plot::plot_line(
                        view,
                        "Up",
                        &self.total_up_hist,
                        self.accents.up,
                    ));
                    plot::plot_navigation(pui);
                    pui.auto_bounds()
                });
                self.total_plot_hint = PlotHint::from(&plot);
            });
    }

    fn draw_detail_panel(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        // Always visible; shows a hint until a process is selected.
        let stats = self
            .selected
            .as_ref()
            .and_then(|key| snap.processes.get(key).cloned());

        egui::Panel::right("detail_panel")
            .resizable(true)
            .default_size(self.settings.panels.detail_width.unwrap_or(300.0))
            .min_size(220.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                let Some(stats) = stats else {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Select a process to see details")
                                .weak()
                                .italics(),
                        );
                    });
                    return;
                };
                let key = stats.exe_path.clone();
                self.draw_detail_header(ui, &stats);
                ui.separator();

                egui::Grid::new("detail_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("PID");
                        ui.label(stats.pid.to_string());
                        ui.end_row();
                        ui.label("Flows");
                        ui.label(stats.flow_count.to_string());
                        ui.end_row();
                        ui.label("Download");
                        ui.label(
                            egui::RichText::new(humanize_rate(stats.down_rate))
                                .color(self.accents.down),
                        );
                        ui.end_row();
                        ui.label("Upload");
                        ui.label(
                            egui::RichText::new(humanize_rate(stats.up_rate))
                                .color(self.accents.up),
                        );
                        ui.end_row();
                        ui.label("Total down");
                        ui.label(humanize_bytes(stats.down_total));
                        ui.end_row();
                        ui.label("Total up");
                        ui.label(humanize_bytes(stats.up_total));
                        ui.end_row();
                        ui.label("Rule");
                        ui.label(rule_summary(snap.rules.get(&key)));
                        ui.end_row();
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("History (per second)");
                    // Truncated: a side panel grows to fit its content, and
                    // the hint must not widen it whenever the plot is hovered.
                    if let Some(text) = self.detail_plot_hint.text() {
                        ui.add(egui::Label::new(egui::RichText::new(text).weak()).truncate());
                    }
                });

                let down: Vec<f64> = stats.down_history.iter().map(|&v| v as f64).collect();
                let up: Vec<f64> = stats.up_history.iter().map(|&v| v as f64).collect();
                let plot = plot::rate_plot("detail_plot")
                    .height(160.0)
                    .show(ui, |pui| {
                        let view = &self.settings.view;
                        pui.line(plot::plot_line(view, "Down", &down, self.accents.down));
                        pui.line(plot::plot_line(view, "Up", &up, self.accents.up));
                        plot::plot_navigation(pui);
                        pui.auto_bounds()
                    });
                self.detail_plot_hint = PlotHint::from(&plot);
            });
    }

    /// Process name and path, with a folder button on the right that opens
    /// the executable's folder. The button is as tall as the two text lines.
    fn draw_detail_header(&self, ui: &mut egui::Ui, stats: &ProcessStats) {
        let folder = std::path::Path::new(&stats.exe_path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty() && p.is_dir());
        ui.horizontal(|ui| {
            let button_width = if folder.is_some() {
                FOLDER_BUTTON_WIDTH
            } else {
                0.0
            };
            let text_width = (ui.available_width() - button_width).max(0.0);
            let text = ui
                .allocate_ui_with_layout(
                    egui::vec2(text_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(text_width);
                        ui.heading(&stats.name);
                        ui.label(egui::RichText::new(&stats.exe_path).weak().small());
                    },
                )
                .response
                .rect;
            if let Some(folder) = folder {
                let side = text.height().clamp(24.0, FOLDER_BUTTON_WIDTH);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response =
                        folder_button(ui, side).on_hover_text(format!("Open {}", folder.display()));
                    if response.clicked() {
                        open_path(folder);
                    }
                });
            }
        });
    }

    fn draw_table(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        // Owned, sorted list of (lowercase key, stats). The key is used for all
        // rule commands per the shared contract.
        let total = snap.processes.len();
        let mut rows: Vec<(String, ProcessStats)> = snap
            .processes
            .iter()
            .filter(|(_, v)| matches_filter(v, &self.filter))
            .filter(|(_, v)| !self.settings.table.active_only || is_active(v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let columns = visible_columns(&self.settings.view);
        // A hidden column cannot be clicked to change the order, so fall back
        // to sorting by name.
        if !columns
            .iter()
            .any(|c| c.sort() == Some(self.settings.table.sort))
        {
            self.settings.table.sort = SortColumn::Name;
            self.settings.table.descending = false;
            self.save_settings();
        }

        let sort_column = self.settings.table.sort;
        let sort_desc = self.settings.table.descending;
        rows.sort_by(|a, b| {
            let (a, b) = (&a.1, &b.1);
            let ord = match sort_column {
                SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortColumn::Flows => a.flow_count.cmp(&b.flow_count),
                SortColumn::Down => a.down_rate.cmp(&b.down_rate),
                SortColumn::Up => a.up_rate.cmp(&b.up_rate),
                SortColumn::DownTotal => a.down_total.cmp(&b.down_total),
                SortColumn::UpTotal => a.up_total.cmp(&b.up_total),
            };
            if sort_desc { ord.reverse() } else { ord }
        });

        let row_height = self.settings.appearance.density.row_height();

        egui::CentralPanel::default().show(ui, |ui| {
            if self.settings.view.show_filter {
                self.draw_filter_row(ui, rows.len(), total);
            }

            // Saved widths are only usable as a full set: with any column
            // left to auto size, the table's sizing pass resizes every column
            // to its content and the saved widths would be lost anyway.
            let saved = &self.settings.table.widths;
            let restore = columns.iter().all(|c| saved.contains_key(c.id()));
            let last = columns.len() - 1;
            let mut table = egui_extras::TableBuilder::new(ui)
                .resizable(true)
                .auto_shrink(false)
                .striped(self.settings.appearance.stripes)
                .sense(egui::Sense::CLICK | egui::Sense::HOVER)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
            for (i, column) in columns.iter().enumerate() {
                table = table.column(match column {
                    // The last column fills the table, so it stays remainder.
                    _ if restore && i == last => egui_extras::Column::remainder(),
                    TableColumn::Name if restore => {
                        egui_extras::Column::initial(saved[column.id()]).at_least(200.)
                    }
                    _ if restore => egui_extras::Column::initial(saved[column.id()]),
                    TableColumn::Name => egui_extras::Column::remainder().at_least(200.),
                    TableColumn::Rule => egui_extras::Column::remainder(),
                    _ => egui_extras::Column::auto(), // Numeric
                });
            }
            // Column widths as laid out this frame, from the header cells. The
            // table sizes auto columns in a sizing pass of its own child ui,
            // where the widths are provisional, so the cells report that too.
            let mut widths: Vec<f32> = Vec::with_capacity(columns.len());
            let mut sizing_pass = false;
            table
                .header(row_height, |mut row| {
                    for &column in &columns {
                        match column.sort() {
                            Some(sort_column) => {
                                row.col(|ui| {
                                    sizing_pass |= ui.is_sizing_pass();
                                    widths.push(ui.max_rect().width());
                                    self.sort_header(ui, column.label(), sort_column);
                                });
                            }
                            None => {
                                row.col(|ui| {
                                    sizing_pass |= ui.is_sizing_pass();
                                    widths.push(ui.max_rect().width());
                                    ui.label(egui::RichText::new(column.label()).strong());
                                });
                            }
                        }
                    }
                })
                .body(|body| {
                    body.rows(row_height, rows.len(), |row| {
                        let (key, stats) = rows.get(row.index()).expect("row index in bounds");

                        let resp = self.draw_table_row(row, key, stats, snap, &columns);
                        if resp.clicked() || resp.secondary_clicked() {
                            self.selected = Some(key.clone());
                        }
                        self.row_context_menu(&resp, key, &stats.name);
                    });
                });
            // Not during a sizing pass nor while a drag may be under way.
            if !sizing_pass && !ui.input(|i| i.pointer.any_down()) {
                self.remember_widths(&columns, &widths);
            }
        });
    }

    /// The filter box above the table, with the match count and a clear
    /// button once something is typed. Ctrl+F focuses it, Escape clears it.
    fn draw_filter_row(&mut self, ui: &mut egui::Ui, shown: usize, total: usize) {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            // The text edit drops focus and swallows Escape itself, so the
            // key is read before it runs.
            let id = ui.make_persistent_id("process_filter");
            let escape =
                ui.memory(|m| m.has_focus(id)) && ui.input(|i| i.key_pressed(egui::Key::Escape));
            if escape {
                self.filter.clear();
            }
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .id(id)
                    .hint_text("Filter processes (Ctrl+F)")
                    .margin(egui::Margin::symmetric(8, 5))
                    .desired_width(220.0),
            );
            if self.focus_filter {
                self.focus_filter = false;
                response.request_focus();
            }
            if !self.filter.is_empty() && ui.small_button("Clear").clicked() {
                self.filter.clear();
            }
            // Framed in every state: an unframed idle chip grows by its
            // stroke on hover and shifts the row by a pixel.
            let active = self.settings.table.active_only;
            if ui
                .add(egui::Button::selectable(active, "Active").frame_when_inactive(true))
                .on_hover_text("Only processes moving data right now")
                .clicked()
            {
                self.settings.table.active_only = !active;
                self.save_settings();
            }
            if shown != total {
                ui.weak(format!("{shown} of {total}"));
            }
        });
        ui.add_space(2.0);
    }

    /// Keep the sizes of the two resizable panels in the settings, once a
    /// drag has ended and only when one of them moved by more than a hair.
    fn remember_panels(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let size = |id: &str| {
            egui::containers::panel::PanelState::load(ctx, egui::Id::new(id)).map(|s| s.size())
        };
        let plot_height = size("top_panel").map(|s| s.y);
        let detail_width = size("detail_panel").map(|s| s.x);
        let mut changed = false;
        for (saved, seen) in [
            (&mut self.settings.panels.plot_height, plot_height),
            (&mut self.settings.panels.detail_width, detail_width),
        ] {
            if let Some(v) = seen.and_then(|seen| settle(*saved, seen)) {
                *saved = Some(v);
                changed = true;
            }
        }
        if changed {
            self.save_settings();
        }
    }

    /// Keep the table's column widths in the settings. Only widths that
    /// moved by more than a hair are written, so a drag ends in one save.
    /// The last column takes whatever is left, so it follows every window
    /// resize; it is recorded but never a reason to write the file.
    fn remember_widths(&mut self, columns: &[TableColumn], widths: &[f32]) {
        if widths.len() != columns.len() {
            return;
        }
        let saved = &mut self.settings.table.widths;
        let mut changed = false;
        let last = columns.len() - 1;
        for (i, (column, &width)) in columns.iter().zip(widths).enumerate() {
            if let Some(width) = settle(saved.get(column.id()).copied(), width) {
                let first_time = saved.insert(column.id().to_string(), width).is_none();
                changed |= i != last || first_time;
            }
        }
        if changed {
            self.save_settings();
        }
    }

    fn draw_table_row(
        &mut self,
        mut row: egui_extras::TableRow,
        key: &String,
        stats: &ProcessStats,
        snap: &Snapshot,
        columns: &[TableColumn],
    ) -> egui::Response {
        let selected = self.selected.as_deref() == Some(key.as_str());

        row.set_selected(selected);
        for &column in columns {
            let content = match column {
                TableColumn::Name => egui::RichText::new(&stats.name),
                TableColumn::Flows => egui::RichText::new(stats.flow_count.to_string()),
                TableColumn::Down => self.numeric(
                    egui::RichText::new(humanize_rate(stats.down_rate)).color(self.accents.down),
                ),
                TableColumn::Up => self.numeric(
                    egui::RichText::new(humanize_rate(stats.up_rate)).color(self.accents.up),
                ),
                TableColumn::DownTotal => {
                    self.numeric(egui::RichText::new(humanize_bytes(stats.down_total)))
                }
                TableColumn::UpTotal => {
                    self.numeric(egui::RichText::new(humanize_bytes(stats.up_total)))
                }
                TableColumn::Rule => {
                    let rule = snap.rules.get(key);
                    let summary = rule_summary(rule);
                    if rule.is_some_and(|r| r.disabled) {
                        egui::RichText::new(summary).weak()
                    } else if rule.is_some_and(|r| r.blocked) {
                        egui::RichText::new(summary)
                            .color(self.accents.block)
                            .strong()
                    } else {
                        egui::RichText::new(summary)
                    }
                }
            };
            row.col(|ui| {
                egui::Label::new(content)
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .selectable(false)
                    .ui(ui);
            });
        }

        row.response()
    }

    fn row_context_menu(&mut self, resp: &egui::Response, key: &str, name: &str) {
        resp.context_menu(|ui| {
            // The "Unknown" row is not a process but the traffic we failed to
            // attribute, so it gets no rule actions.
            if !is_shapable(key) {
                ui.label(
                    egui::RichText::new("Unattributed traffic cannot be limited")
                        .weak()
                        .italics(),
                );
                return;
            }
            // System is shapable, but covers more than people expect.
            if is_system(key) {
                ui.label(
                    egui::RichText::new(
                        "Kernel traffic: includes VPN tunnels, network shares and Windows Update",
                    )
                    .weak()
                    .small(),
                );
                ui.separator();
            }
            if ui.button("Set download limit\u{2026}").clicked() {
                self.open_limit_modal(key, name, Direction::Down);
                ui.close();
            }
            if ui.button("Set upload limit\u{2026}").clicked() {
                self.open_limit_modal(key, name, Direction::Up);
                ui.close();
            }
            ui.separator();
            let blocked = self
                .last_snapshot
                .rules
                .get(key)
                .map(|r| r.blocked)
                .unwrap_or(false);
            if blocked {
                if ui.button("Unblock").clicked() {
                    self.set_blocked(key, false);
                    ui.close();
                }
            } else if ui.button("Block").clicked() {
                self.set_blocked(key, true);
                ui.close();
            }
            ui.separator();
            if let Some(rule) = self.last_snapshot.rules.get(key) {
                let (label, disable) = if rule.disabled {
                    ("Enable rule", false)
                } else {
                    ("Disable rule", true)
                };
                if ui.button(label).clicked() {
                    self.set_rule_disabled(key, disable);
                    ui.close();
                }
            }
            if ui.button("Remove rule").clicked() {
                let _ = self.cmd_tx.send(Command::RemoveRule(key.to_string()));
                ui.close();
            }
        });
    }

    fn draw_limit_modal(&mut self, ctx: &egui::Context) {
        let Some(mut modal) = self.limit_modal.take() else {
            return;
        };

        let dir_label = match modal.direction {
            Direction::Down => "download",
            Direction::Up => "upload",
        };

        let mut open = true;
        let mut apply = false;
        let mut cancel = false;

        egui::Window::new(format!("Set {dir_label} limit"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("Process: {}", modal.name));
                ui.label(
                    egui::RichText::new("Leave empty for unlimited.")
                        .weak()
                        .small(),
                );
                ui.horizontal(|ui| {
                    // As tall as the unit box beside it, which is a button:
                    // the interact height, or its text plus padding if taller.
                    let spacing = ui.spacing();
                    let button_height = spacing.interact_size.y.max(
                        ui.text_style_height(&egui::TextStyle::Button)
                            + 2.0 * spacing.button_padding.y,
                    );
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut modal.value)
                            .desired_width(80.0)
                            .min_size(egui::vec2(0.0, button_height))
                            .vertical_align(egui::Align::Center)
                            .hint_text("e.g. 2"),
                    );
                    if resp.changed() {
                        modal.value = numeric_input(&modal.value);
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        apply = true;
                    }
                    egui::ComboBox::from_id_salt("rate-unit")
                        .selected_text(modal.unit.label())
                        .show_ui(ui, |ui| {
                            for u in RateUnit::ALL {
                                ui.selectable_value(&mut modal.unit, u, u.label());
                            }
                        });
                });
                if let Some(err) = &modal.error {
                    ui.colored_label(self.accents.block, err);
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if cancel || !open {
            return; // modal dropped
        }

        if apply {
            let trimmed = modal.value.trim().replace(',', ".");
            let limit: Option<u64> = if trimmed.is_empty() {
                None
            } else {
                match trimmed.parse::<f64>() {
                    Ok(v) if v.is_finite() && v >= 0.0 => {
                        Some((v * modal.unit.multiplier()) as u64)
                    }
                    _ => {
                        modal.error = Some("Enter a plain number, e.g. 2 or 0.5".to_string());
                        self.limit_modal = Some(modal);
                        return;
                    }
                }
            };
            let mut rule = self.base_rule(&modal.exe_path);
            rule.exe_path = modal.exe_path.clone();
            match modal.direction {
                Direction::Down => rule.down_limit = limit,
                Direction::Up => rule.up_limit = limit,
            }
            // A new limit is meant to apply, so a disabled rule comes back on.
            rule.disabled = false;
            self.send_rule(rule);
            return; // done, modal closes
        }

        // Still open, no decision yet: keep it around.
        self.limit_modal = Some(modal);
    }
}

impl eframe::App for ThrottleApp {
    /// Runs every frame, and also while the window is hidden to the tray, so
    /// everything that must keep ticking in the background lives here.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drain pending snapshots even while hidden: the channel is unbounded.
        let incoming: Vec<Snapshot> = self.snapshot_rx.try_iter().collect();
        let keep = self.settings.view.history_secs.max(1);
        let mut fresh = false;
        for snap in incoming {
            self.total_down_hist.push(snap.total_down_rate as f64);
            self.total_up_hist.push(snap.total_up_rate as f64);
            trim_front(&mut self.total_down_hist, keep);
            trim_front(&mut self.total_up_hist, keep);
            self.last_snapshot = snap;
            fresh = true;
        }
        if fresh {
            self.refresh_tray(ctx);
        }

        // The window exists by the first frame; only a non default level is
        // sent, so the stock window is left alone.
        if self.first_frame {
            self.first_frame = false;
            if self.settings.view.always_on_top {
                self.send_window_level(ctx);
            }
        }

        // At logon the startup task can run before Explorer has created the
        // taskbar. The tray icon is then only added once it appears, so wait for
        // it rather than hide with no icon to click; give up and stay visible if
        // it takes too long.
        if let Some(deadline) = self.start_hidden {
            if tray::taskbar_ready() {
                self.start_hidden = None;
                self.hide_to_tray(ctx);
            } else if Instant::now() >= deadline {
                tracing::warn!("taskbar not found; starting visible instead of in the tray");
                self.start_hidden = None;
            }
        }

        let actions: Vec<TrayAction> = self.tray_rx.try_iter().collect();
        for action in actions {
            match action {
                TrayAction::Show => {
                    self.start_hidden = None;
                    self.show_window(ctx);
                }
                TrayAction::Exit => self.exit(ctx),
                TrayAction::ToggleRule(key) => {
                    let disabled = self.last_snapshot.rules.get(&key).map(|r| r.disabled);
                    if let Some(disabled) = disabled {
                        self.set_rule_disabled(&key, !disabled);
                    }
                }
            }
        }

        // Hide when the window becomes minimised, not while it stays so: just
        // after a restore from the tray it still reads as minimised for a frame,
        // and hiding then would take a second click to bring it back.
        let minimized = ctx.input(|i| i.viewport().minimized == Some(true));
        if minimized && !self.was_minimized && self.settings.minimize_to_tray && !self.hidden {
            self.hide_to_tray(ctx);
        }
        self.was_minimized = minimized;

        // Window close: hide to the tray if asked to, else shut the backend down.
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.settings.close_to_tray && self.tray.is_some() && !self.exiting {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.hide_to_tray(ctx);
            } else {
                self.send_shutdown();
            }
        }

        // Keep ticking (and draining snapshots) while hidden to the tray.
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    /// Only the window geometry is persisted by eframe; the rest of the
    /// layout is in settings.json, so a stale egui state cannot shadow it.
    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Ctrl plus / minus zoomed the UI: keep the new factor as the scale.
        // A scale change from the Customise window lands here already equal.
        let zoom = ctx.zoom_factor();
        if (zoom - self.settings.appearance.scale).abs() > 0.001 {
            self.settings.appearance.scale = zoom;
            self.save_settings();
        }

        if let Ok(enabled) = self.autostart_rx.try_recv() {
            self.autostart = Some(enabled);
        }

        let snap = self.last_snapshot.clone();

        // Clear a stale selection so the side panel doesn't linger.
        if let Some(sel) = &self.selected
            && !snap.processes.contains_key(sel)
        {
            self.selected = None;
        }

        if let Ok(update) = self.update_rx.try_recv() {
            self.update = Some(update);
        }

        if self.settings.view.show_filter
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F))
        {
            self.focus_filter = true;
        }

        self.draw_menu_bar(ui);
        self.draw_update_banner(ui);
        self.draw_top_panel(ui, &snap);
        self.draw_detail_panel(ui, &snap);
        self.draw_table(ui, &snap);
        self.draw_limit_modal(&ctx);
        self.draw_about(&ctx);
        self.draw_customise_window(&ctx);
        self.draw_import_error(&ctx);
        self.remember_panels(&ctx);
    }
}

impl Drop for ThrottleApp {
    fn drop(&mut self) {
        self.send_shutdown();
    }
}

// ---------------------------------------------------------------------------
// Startup helpers
// ---------------------------------------------------------------------------

/// Check the startup task off the UI thread (`schtasks` takes a moment). The
/// task is re-pointed at this exe only if the exe it starts no longer exists
/// (Throttle was moved): running another copy once must not take it over.
fn spawn_autostart_check() -> Receiver<bool> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let enabled = autostart::is_enabled();
        if enabled
            && autostart::task_command().is_some_and(|cmd| !cmd.exists())
            && let Ok(exe) = std::env::current_exe()
            && let Err(e) = autostart::enable(&exe)
        {
            tracing::warn!("failed to refresh the startup task: {e:#}");
        }
        let _ = tx.send(enabled);
    });
    rx
}

// ---------------------------------------------------------------------------
// Theme and plot helpers
// ---------------------------------------------------------------------------

/// Paint a palette preview before its menu label: the background with a
/// border and one dot per accent (down, up, block).
fn draw_swatch(ui: &mut egui::Ui, palette: &theme::Palette) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 10.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, palette.bg);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, palette.border),
        egui::StrokeKind::Inside,
    );
    let accents = [
        palette.accents.down,
        palette.accents.up,
        palette.accents.block,
    ];
    for (i, colour) in accents.into_iter().enumerate() {
        let centre = egui::pos2(rect.left() + 3.5 * (i + 1) as f32, rect.center().y);
        painter.circle_filled(centre, 1.5, colour);
    }
}

/// Drop the oldest samples so at most `keep` remain.
fn trim_front(hist: &mut Vec<f64>, keep: usize) {
    if hist.len() > keep {
        let excess = hist.len() - keep;
        hist.drain(0..excess);
    }
}

/// The id to store an imported palette under: its own, unless it is empty
/// or would be shadowed by a bundled theme, which `theme::find` prefers.
fn custom_theme_id(id: &str) -> String {
    let id = id.trim();
    if id.is_empty() {
        return "imported".to_string();
    }
    let taken =
        theme::BUILTIN.iter().any(|(b, _)| *b == id) || theme::builtin().any(|p| p.id == id);
    if taken {
        format!("{id}-imported")
    } else {
        id.to_string()
    }
}

/// A seed for "Surprise me" from the clock; no need for a random crate.
fn time_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() ^ u64::from(d.subsec_nanos()))
        .unwrap_or(0)
}

/// Map `seed` to an index below `len` (which must be non zero). The seed is
/// mixed first: Windows clocks tick in 100 ns steps, so the raw nanoseconds
/// share factors with many list lengths.
fn random_index(len: usize, seed: u64) -> usize {
    let mut x = seed;
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    (x % len as u64) as usize
}

// ---------------------------------------------------------------------------
// Shell helpers
// ---------------------------------------------------------------------------

/// Open a URL in the default browser.
fn open_url(ctx: &egui::Context, url: &str) {
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

/// Open a file with its default application, or a folder in Explorer.
/// Best-effort: failures are logged, never surfaced as errors.
/// A square folder icon button of `side` points. It uses the theme's text
/// colours, which are chosen to read against the panel in every palette:
/// the weak text colour at rest, the full text colour under the pointer,
/// over a faint translucent background.
fn folder_button(ui: &mut egui::Ui, side: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    let visuals = ui.visuals();
    let colour = if response.hovered() {
        visuals.text_color()
    } else {
        visuals.weak_text_color()
    };
    // The front flap is a step towards the panel colour, for a little depth.
    let flap_colour = colour.lerp_to_gamma(visuals.panel_fill, 0.3);
    let painter = ui.painter();
    if response.hovered() {
        let bg = visuals.text_color().gamma_multiply(0.1);
        painter.rect_filled(rect, egui::CornerRadius::same(4), bg);
    }
    // A tab on the top left, the body under it and the front flap.
    let r = rect.shrink(side * 0.22);
    let tab = egui::Rect::from_min_size(r.min, egui::vec2(r.width() * 0.42, r.height() * 0.3));
    let body = egui::Rect::from_min_max(egui::pos2(r.min.x, r.min.y + r.height() * 0.16), r.max);
    let flap = egui::Rect::from_min_max(egui::pos2(r.min.x, r.min.y + r.height() * 0.45), r.max);
    painter.rect_filled(tab, egui::CornerRadius::same(2), colour);
    painter.rect_filled(body, egui::CornerRadius::same(2), colour);
    painter.rect_filled(flap, egui::CornerRadius::same(2), flap_colour);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn open_path(path: &std::path::Path) {
    if let Err(e) = std::process::Command::new("explorer").arg(path).spawn() {
        tracing::warn!("failed to open {}: {e}", path.display());
    }
}

// ---------------------------------------------------------------------------
// Fallback error window (shown when the backend can't start, e.g. not elevated
// or WinDivert is missing).
// ---------------------------------------------------------------------------

pub struct ErrorApp {
    message: String,
}

impl ErrorApp {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

impl eframe::App for ErrorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.heading("Throttle could not start");
            });
            ui.add_space(12.0);
            ui.label(
                "The backend failed to initialize. This usually means the app is not \
                 running as Administrator, or the WinDivert driver files \
                 (WinDivert.dll / WinDivert64.sys) are missing next to the executable.",
            );
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Details:").strong());
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(
                    egui::RichText::new(&self.message)
                        .monospace()
                        .color(theme::Accents::DEFAULT.block),
                );
            });
            ui.add_space(12.0);
            ui.separator();
            if ui.button("Close").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_history_matches_view_default() {
        assert_eq!(View::default().history_secs, TOTAL_HISTORY_LEN);
        assert!(HISTORY_CHOICES.iter().any(|(s, _)| *s == TOTAL_HISTORY_LEN));
        let width = View::default().plot_line_width;
        assert!(LINE_WIDTH_CHOICES.iter().any(|(w, _)| *w == width));
    }

    #[test]
    fn default_view_shows_every_column() {
        assert_eq!(visible_columns(&View::default()), TableColumn::ALL.to_vec());
    }

    #[test]
    fn hidden_columns_keep_the_name() {
        let view = View {
            show_flows: false,
            show_down: false,
            show_up: false,
            show_down_total: false,
            show_up_total: false,
            show_rule: false,
            ..View::default()
        };
        assert_eq!(visible_columns(&view), vec![TableColumn::Name]);
        let view = View {
            show_up: false,
            ..View::default()
        };
        let columns = visible_columns(&view);
        assert!(!columns.contains(&TableColumn::Up));
        assert_eq!(columns.len(), 6);
    }

    #[test]
    fn limit_field_keeps_only_a_number() {
        assert_eq!(numeric_input("dsadsa"), "");
        assert_eq!(numeric_input("2"), "2");
        assert_eq!(numeric_input("0.5"), "0.5");
        assert_eq!(numeric_input("1,5"), "1,5");
        assert_eq!(numeric_input("1.2.3"), "1.23");
        assert_eq!(numeric_input("12 MB/s"), "12");
        assert_eq!(numeric_input("-3e5"), "35");
    }

    #[test]
    fn rule_summary_marks_disabled_rules() {
        let rule = Rule {
            exe_path: "c:\\x\\a.exe".to_string(),
            down_limit: Some(2 * 1024 * 1024),
            up_limit: None,
            blocked: false,
            disabled: true,
        };
        assert_eq!(rule_summary(Some(&rule)), "Off: Dn 2.00 MB/s");
        let active = Rule {
            disabled: false,
            ..rule.clone()
        };
        assert_eq!(rule_summary(Some(&active)), "Dn 2.00 MB/s");
        let blocked = Rule {
            blocked: true,
            ..rule
        };
        assert_eq!(rule_summary(Some(&blocked)), "Off: BLOCKED");
        assert_eq!(rule_summary(None), "-");
    }

    #[test]
    fn active_means_moving_data_in_either_direction() {
        let idle = ProcessStats::default();
        assert!(!is_active(&idle));
        assert!(is_active(&ProcessStats {
            up_rate: 1,
            ..ProcessStats::default()
        }));
        assert!(is_active(&ProcessStats {
            down_rate: 1,
            ..ProcessStats::default()
        }));
    }

    #[test]
    fn filter_matches_name_or_path_ignoring_case() {
        let stats = ProcessStats {
            name: "Chrome.exe".to_string(),
            exe_path: "c:\\program files\\google\\chrome.exe".to_string(),
            ..ProcessStats::default()
        };
        assert!(matches_filter(&stats, ""));
        assert!(matches_filter(&stats, "  "));
        assert!(matches_filter(&stats, "CHROME"));
        assert!(matches_filter(&stats, "google"));
        assert!(!matches_filter(&stats, "firefox"));
    }

    #[test]
    fn column_ids_are_unique() {
        let mut ids: Vec<&str> = TableColumn::ALL.iter().map(|c| c.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TableColumn::ALL.len());
    }

    #[test]
    fn trim_front_keeps_the_newest() {
        let mut hist: Vec<f64> = (0..10).map(f64::from).collect();
        trim_front(&mut hist, 3);
        assert_eq!(hist, vec![7.0, 8.0, 9.0]);
        trim_front(&mut hist, 5);
        assert_eq!(hist.len(), 3);
    }

    #[test]
    fn random_index_stays_in_range_and_varies() {
        let mut seen = std::collections::HashSet::new();
        // Seeds 100 ns apart, as a Windows clock gives them.
        for i in 0..200u64 {
            let idx = random_index(20, 1_700_000_000 ^ (i * 100));
            assert!(idx < 20);
            seen.insert(idx);
        }
        assert!(seen.len() > 10, "only {} distinct picks", seen.len());
        assert_eq!(random_index(1, 12345), 0);
    }

    #[test]
    fn custom_ids_never_shadow_bundled_themes() {
        assert_eq!(custom_theme_id("my-theme"), "my-theme");
        assert_eq!(custom_theme_id("  "), "imported");
        assert_eq!(custom_theme_id(theme::DEFAULT_ID), "system-imported");
        if let Some(p) = theme::builtin().next() {
            let id = custom_theme_id(&p.id);
            assert_ne!(id, p.id);
            assert!(theme::find(&id, &[]).is_none());
        }
    }
}
