//! egui/eframe front-end for Throttle.
//!
//! Consumes `Snapshot`s from the backend (~1/sec) and emits `Command`s back.
//! This file is built against the shared contract in `crate::types` and must
//! not depend on any backend internals beyond the `snapshot_rx` receiver.

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use eframe::egui::Widget;
use egui_plot::{Legend, Line, Plot, PlotPoints};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::backend::rules;
use crate::backend::synthetic::{is_shapable, is_system};
use crate::settings::{self, Settings};
use crate::types::{Command, ProcessStats, Rule, Snapshot};
use crate::update_check::{self, UpdateInfo};
use crate::{autostart, icon, single_instance};

/// How many total-rate samples we keep for the header graph.
const TOTAL_HISTORY_LEN: usize = 120;

/// Project links used by the Help menu and the About dialog.
const REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Version of the bundled WinDivert driver (see `vendor/`).
const WINDIVERT_VERSION: &str = "2.2.2";

const COLOR_DOWN: egui::Color32 = egui::Color32::from_rgb(0x4c, 0xaf, 0x50);
const COLOR_UP: egui::Color32 = egui::Color32::from_rgb(0x21, 0x96, 0xf3);
const COLOR_BLOCK: egui::Color32 = egui::Color32::from_rgb(0xe5, 0x39, 0x35);

// ---------------------------------------------------------------------------
// Formatting / parsing helpers
// ---------------------------------------------------------------------------

const KB: f64 = 1024.0;
const MB: f64 = 1024.0 * 1024.0;
const GB: f64 = 1024.0 * 1024.0 * 1024.0;
const TB: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0;

/// Humanize a byte-per-second rate using base-1024 units.
fn humanize_rate(bps: u64) -> String {
    let b = bps as f64;
    if b >= GB {
        format!("{:.2} GB/s", b / GB)
    } else if b >= MB {
        format!("{:.2} MB/s", b / MB)
    } else if b >= KB {
        format!("{:.1} KB/s", b / KB)
    } else {
        format!("{bps} B/s")
    }
}

/// Humanize a plain byte total using base-1024 units.
fn humanize_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= TB {
        format!("{:.2} TB", b / TB)
    } else if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

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

/// One-line summary of a rule for the table column.
fn rule_summary(rule: Option<&Rule>) -> String {
    match rule {
        None => "-".to_string(),
        Some(r) if r.blocked => "BLOCKED".to_string(),
        Some(r) => {
            let mut parts = Vec::new();
            if let Some(d) = r.down_limit {
                parts.push(format!("Dn {}", humanize_rate(d)));
            }
            if let Some(u) = r.up_limit {
                parts.push(format!("Up {}", humanize_rate(u)));
            }
            if parts.is_empty() {
                "-".to_string()
            } else {
                parts.join("  ")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sorting / modal state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortColumn {
    Name,
    Flows,
    Down,
    Up,
    DownTotal,
    UpTotal,
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

    sort_column: SortColumn,
    sort_desc: bool,
    selected: Option<String>,

    limit_modal: Option<LimitModal>,
    about_open: bool,
    shutdown_sent: bool,

    settings: Settings,
    /// `None` until the background check of the startup task reports back.
    autostart: Option<bool>,
    autostart_rx: Receiver<bool>,
    autostart_error: Option<String>,

    /// `None` if the tray icon could not be created; the window then never hides.
    tray: Option<TrayIcon>,
    tray_rx: Receiver<TrayAction>,
    /// The window is hidden to the tray (no taskbar button).
    hidden: bool,
    /// Whether the window was minimised last frame, to catch the moment it is.
    was_minimized: bool,
    /// Hide to the tray on the first frame (`--tray` or "Start minimised").
    start_hidden: bool,
    /// A real exit is under way, so a close request must not go to the tray.
    exiting: bool,

    update_rx: Receiver<UpdateInfo>,
    update: Option<UpdateInfo>,
    update_dismissed: bool,
    update_install: UpdateInstall,
}

/// What the tray icon (or a second launch) asks the app to do.
enum TrayAction {
    Show,
    Exit,
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
        let tray = create_tray(&cc.egui_ctx, tray_tx.clone());
        let ctx = cc.egui_ctx.clone();
        single_instance::on_show_request(move || {
            let _ = tray_tx.send(TrayAction::Show);
            ctx.request_repaint();
        });
        Self {
            snapshot_rx,
            cmd_tx,
            last_snapshot: Snapshot::default(),
            total_down_hist: Vec::with_capacity(TOTAL_HISTORY_LEN),
            total_up_hist: Vec::with_capacity(TOTAL_HISTORY_LEN),
            sort_column: SortColumn::Down,
            sort_desc: true,
            selected: None,
            limit_modal: None,
            about_open: false,
            shutdown_sent: false,
            settings,
            autostart: None,
            autostart_rx: spawn_autostart_check(),
            autostart_error: None,
            tray,
            tray_rx,
            hidden: false,
            was_minimized: false,
            start_hidden,
            exiting: false,
            update_rx: update_check::spawn(APP_VERSION),
            update: None,
            update_dismissed: false,
            update_install: UpdateInstall::Idle,
        }
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

    fn set_blocked(&mut self, key: &str, blocked: bool) {
        let mut rule = self.base_rule(key);
        rule.exe_path = key.to_string();
        rule.blocked = blocked;
        self.send_rule(rule);
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
        let col_selected = self.sort_column == col;

        let direction_text = if !col_selected {
            ""
        } else if self.sort_desc {
            "▼"
        } else {
            "▲"
        };
        // The default font is missing the arrows, use the bundled monospace font (Hack) instead
        let direction = egui::RichText::new(direction_text).monospace();

        self.sort_header_button_style(ui, col_selected);
        let button = egui::Button::new((text, grow))
            .right_text(direction)
            .min_size(ui.available_size())
            .corner_radius(0);

        if ui.add(button).clicked() {
            if col_selected {
                self.sort_desc = !self.sort_desc;
            } else {
                self.sort_column = col;
                self.sort_desc = true;
            }
        }
    }

    fn draw_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
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
                        Some(u) => format!("Update available: v{}…", u.version),
                        None => "Check for updates…".to_string(),
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
                    ui.separator();
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
            ui.label(egui::RichText::new(e).small().color(COLOR_BLOCK));
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
                    ui.label("Downloading update…");
                    return;
                }
                if let UpdateInstall::Failed(e) = &self.update_install {
                    ui.label(egui::RichText::new(format!("Update failed: {e}")).color(COLOR_BLOCK));
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
                });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Copyright © 2026 Francesco Gruosso")
                        .weak()
                        .small(),
                );
            });
        self.about_open = open;
    }

    fn draw_top_panel(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        egui::Panel::top("top_panel")
            .resizable(true)
            .default_size(120.0)
            .min_size(70.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.heading("Throttle");
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "Down {}",
                            humanize_rate(snap.total_down_rate)
                        ))
                        .color(COLOR_DOWN)
                        .strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!("Up {}", humanize_rate(snap.total_up_rate)))
                            .color(COLOR_UP)
                            .strong(),
                    );
                    ui.separator();
                    ui.label(format!("{} processes", snap.processes.len()));
                });

                // Let the plot fill the panel's remaining height rather than
                // requesting an explicit one. An explicit height derived from
                // available_height() makes the content's reported size disagree
                // with the drag target each frame, which egui persists as the new
                // panel size and turns into a resize feedback loop. Filling avoids
                // that.
                Plot::new("total_plot")
                    .show_x(false)
                    .allow_zoom(false)
                    .allow_drag(false)
                    .allow_scroll(false)
                    .allow_boxed_zoom(false)
                    .legend(Legend::default())
                    .include_y(0.0)
                    .show(ui, |pui| {
                        pui.line(
                            Line::new("Down", PlotPoints::from_ys_f64(&self.total_down_hist))
                                .color(COLOR_DOWN),
                        );
                        pui.line(
                            Line::new("Up", PlotPoints::from_ys_f64(&self.total_up_hist))
                                .color(COLOR_UP),
                        );
                    });
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
            .default_size(300.0)
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
                ui.heading(&stats.name);
                ui.label(egui::RichText::new(&stats.exe_path).weak().small());
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
                            egui::RichText::new(humanize_rate(stats.down_rate)).color(COLOR_DOWN),
                        );
                        ui.end_row();
                        ui.label("Upload");
                        ui.label(egui::RichText::new(humanize_rate(stats.up_rate)).color(COLOR_UP));
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
                ui.label("History (per second)");

                let down: Vec<f64> = stats.down_history.iter().map(|&v| v as f64).collect();
                let up: Vec<f64> = stats.up_history.iter().map(|&v| v as f64).collect();
                Plot::new("detail_plot")
                    .height(160.0)
                    .show_x(false)
                    .allow_zoom(false)
                    .allow_drag(false)
                    .allow_scroll(false)
                    .allow_boxed_zoom(false)
                    .legend(Legend::default())
                    .include_y(0.0)
                    .show(ui, |pui| {
                        pui.line(
                            Line::new("Down", PlotPoints::from_ys_f64(&down)).color(COLOR_DOWN),
                        );
                        pui.line(Line::new("Up", PlotPoints::from_ys_f64(&up)).color(COLOR_UP));
                    });
            });
    }

    fn draw_table(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        // Owned, sorted list of (lowercase key, stats). The key is used for all
        // rule commands per the shared contract.
        let mut rows: Vec<(String, ProcessStats)> = snap
            .processes
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let sort_column = self.sort_column;
        let sort_desc = self.sort_desc;
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

        const SORT_COLUMN_LABELS: [(SortColumn, &str); 6] = [
            (SortColumn::Name, "Process"),
            (SortColumn::Flows, "Flows"),
            (SortColumn::Down, "Down"),
            (SortColumn::Up, "Up"),
            (SortColumn::DownTotal, "Total down"),
            (SortColumn::UpTotal, "Total up"),
        ];

        egui::CentralPanel::default().show(ui, |ui| {
            egui_extras::TableBuilder::new(ui)
                .resizable(true)
                .auto_shrink(false)
                .striped(true)
                .sense(egui::Sense::CLICK | egui::Sense::HOVER)
                .column(egui_extras::Column::remainder().at_least(200.)) // Name
                .columns(egui_extras::Column::auto(), 5) // Numeric
                .column(egui_extras::Column::remainder()) // Rule
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .header(20., |mut row| {
                    for (sort_column, label) in SORT_COLUMN_LABELS {
                        row.col(|ui| self.sort_header(ui, label, sort_column));
                    }
                    row.col(|ui| {
                        ui.label(egui::RichText::new("Rule").strong());
                    });
                })
                .body(|body| {
                    body.rows(20., rows.len(), |row| {
                        let (key, stats) = rows.get(row.index()).expect("row index in bounds");

                        let resp = self.draw_table_row(row, key, stats, snap);
                        if resp.clicked() || resp.secondary_clicked() {
                            self.selected = Some(key.clone());
                        }
                        self.row_context_menu(&resp, key, &stats.name);
                    });
                });
        });
    }

    fn draw_table_row(
        &mut self,
        mut row: egui_extras::TableRow,
        key: &String,
        stats: &ProcessStats,
        snap: &Snapshot,
    ) -> egui::Response {
        let selected = self.selected.as_deref() == Some(key.as_str());

        let rule = snap.rules.get(key);
        let summary = rule_summary(rule);
        let rule_text = if rule.map(|r| r.blocked).unwrap_or(false) {
            egui::RichText::new(summary).color(COLOR_BLOCK).strong()
        } else {
            egui::RichText::new(summary)
        };

        let cells: [egui::RichText; 7] = [
            egui::RichText::new(&stats.name),
            egui::RichText::new(stats.flow_count.to_string()),
            egui::RichText::new(humanize_rate(stats.down_rate)).color(COLOR_DOWN),
            egui::RichText::new(humanize_rate(stats.up_rate)).color(COLOR_UP),
            egui::RichText::new(humanize_bytes(stats.down_total)),
            egui::RichText::new(humanize_bytes(stats.up_total)),
            rule_text,
        ];
        row.set_selected(selected);
        for content in cells {
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
            if ui.button("Set download limit…").clicked() {
                self.open_limit_modal(key, name, Direction::Down);
                ui.close();
            }
            if ui.button("Set upload limit…").clicked() {
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
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut modal.value)
                            .desired_width(80.0)
                            .hint_text("e.g. 2"),
                    );
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
                    ui.colored_label(COLOR_BLOCK, err);
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
        for snap in incoming {
            self.total_down_hist.push(snap.total_down_rate as f64);
            self.total_up_hist.push(snap.total_up_rate as f64);
            if self.total_down_hist.len() > TOTAL_HISTORY_LEN {
                let excess = self.total_down_hist.len() - TOTAL_HISTORY_LEN;
                self.total_down_hist.drain(0..excess);
            }
            if self.total_up_hist.len() > TOTAL_HISTORY_LEN {
                let excess = self.total_up_hist.len() - TOTAL_HISTORY_LEN;
                self.total_up_hist.drain(0..excess);
            }
            self.last_snapshot = snap;
        }

        if std::mem::take(&mut self.start_hidden) {
            self.hide_to_tray(ctx);
        }

        let actions: Vec<TrayAction> = self.tray_rx.try_iter().collect();
        for action in actions {
            match action {
                TrayAction::Show => self.show_window(ctx),
                TrayAction::Exit => self.exit(ctx),
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

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

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

        self.draw_menu_bar(ui);
        self.draw_update_banner(ui);
        self.draw_top_panel(ui, &snap);
        self.draw_detail_panel(ui, &snap);
        self.draw_table(ui, &snap);
        self.draw_limit_modal(&ctx);
        self.draw_about(&ctx);
    }
}

impl Drop for ThrottleApp {
    fn drop(&mut self) {
        self.send_shutdown();
    }
}

// ---------------------------------------------------------------------------
// Tray and startup helpers
// ---------------------------------------------------------------------------

/// Create the tray icon: left click shows the window, right click opens a
/// Show / Exit menu. Returns `None` (logged) if the shell refuses the icon,
/// e.g. Explorer is not running yet.
fn create_tray(ctx: &egui::Context, tx: Sender<TrayAction>) -> Option<TrayIcon> {
    let show = MenuItem::new("Show Throttle", true, None);
    let exit = MenuItem::new("Exit", true, None);
    let menu = Menu::new();
    if let Err(e) = menu.append_items(&[&show, &PredefinedMenuItem::separator(), &exit]) {
        tracing::warn!("tray menu unavailable: {e}");
        return None;
    }

    // Hovering fires Move/Enter/Leave constantly: only clicks wake the app.
    let (click_tx, click_ctx) = (tx.clone(), ctx.clone());
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
        | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } = event
        {
            let _ = click_tx.send(TrayAction::Show);
            click_ctx.request_repaint();
        }
    }));
    let (show_id, exit_id, ctx) = (show.id().clone(), exit.id().clone(), ctx.clone());
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let action = if event.id == show_id {
            TrayAction::Show
        } else if event.id == exit_id {
            TrayAction::Exit
        } else {
            return;
        };
        let _ = tx.send(action);
        ctx.request_repaint();
    }));

    let built = Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE)
        .map_err(|e| e.to_string())
        .and_then(|icon| {
            TrayIconBuilder::new()
                .with_icon(icon)
                .with_tooltip("Throttle")
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .build()
                .map_err(|e| e.to_string())
        });
    match built {
        Ok(tray) => Some(tray),
        Err(e) => {
            tracing::warn!("tray icon unavailable: {e}");
            None
        }
    }
}

/// Check the startup task off the UI thread (`schtasks` takes a moment). An
/// existing task is re-pointed at this exe, in case Throttle was moved (not
/// needed when the task itself launched us).
fn spawn_autostart_check() -> Receiver<bool> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let launched_by_task = std::env::args().any(|a| a == autostart::TRAY_FLAG);
    std::thread::spawn(move || {
        let enabled = autostart::is_enabled();
        if enabled
            && !launched_by_task
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
// Shell helpers
// ---------------------------------------------------------------------------

/// Open a URL in the default browser.
fn open_url(ctx: &egui::Context, url: &str) {
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

/// Open a file with its default application, or a folder in Explorer.
/// Best-effort: failures are logged, never surfaced as errors.
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
                        .color(COLOR_BLOCK),
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
