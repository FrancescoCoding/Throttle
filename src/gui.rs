//! egui/eframe front-end for Throttle.
//!
//! Consumes `Snapshot`s from the backend (~1/sec) and emits `Command`s back.
//! This file is built against the shared contract in `crate::types` and must
//! not depend on any backend internals beyond the `snapshot_rx` receiver.

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::types::{Command, ProcessStats, Rule, Snapshot};

/// How many total-rate samples we keep for the header graph.
const TOTAL_HISTORY_LEN: usize = 120;

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
    const ALL: [RateUnit; 4] = [RateUnit::Bps, RateUnit::KBps, RateUnit::MBps, RateUnit::GBps];

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
    shutdown_sent: bool,
}

impl ThrottleApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        snapshot_rx: Receiver<Snapshot>,
        cmd_tx: Sender<Command>,
    ) -> Self {
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
            shutdown_sent: false,
        }
    }

    fn send_shutdown(&mut self) {
        if !self.shutdown_sent {
            let _ = self.cmd_tx.send(Command::Shutdown);
            self.shutdown_sent = true;
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

    /// Adjust the sort state when a header is clicked.
    fn sort_header(&mut self, ui: &mut egui::Ui, label: &str, col: SortColumn) {
        let text = egui::RichText::new(label).strong();
        if ui.button(text).clicked() {
            if self.sort_column == col {
                self.sort_desc = !self.sort_desc;
            } else {
                self.sort_column = col;
                self.sort_desc = true;
            }
        }
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
                    egui::RichText::new(format!("Down {}", humanize_rate(snap.total_down_rate)))
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

                egui::Grid::new("detail_grid").num_columns(2).show(ui, |ui| {
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
                        pui.line(Line::new("Down", PlotPoints::from_ys_f64(&down)).color(COLOR_DOWN));
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

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Spread the columns across the full available width so the
                    // table doesn't huddle in the left corner of wide windows.
                    let col_width = ((ui.available_width() - 6.0 * 16.0) / 7.0).max(60.0);
                    egui::Grid::new("proc_grid")
                        .num_columns(7)
                        .striped(true)
                        .spacing([16.0, 4.0])
                        .min_col_width(col_width)
                        .show(ui, |ui| {
                            self.sort_header(ui, "Process", SortColumn::Name);
                            self.sort_header(ui, "Flows", SortColumn::Flows);
                            self.sort_header(ui, "Down", SortColumn::Down);
                            self.sort_header(ui, "Up", SortColumn::Up);
                            self.sort_header(ui, "Total down", SortColumn::DownTotal);
                            self.sort_header(ui, "Total up", SortColumn::UpTotal);
                            ui.label(egui::RichText::new("Rule").strong());
                            ui.end_row();

                            for (key, stats) in &rows {
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
                                    egui::RichText::new(humanize_rate(stats.down_rate))
                                        .color(COLOR_DOWN),
                                    egui::RichText::new(humanize_rate(stats.up_rate))
                                        .color(COLOR_UP),
                                    egui::RichText::new(humanize_bytes(stats.down_total)),
                                    egui::RichText::new(humanize_bytes(stats.up_total)),
                                    rule_text,
                                ];

                                // Reserve a shape slot before the cells so the
                                // row highlight paints behind the text; it is
                                // filled once we know the row's vertical extent.
                                let bg_idx = ui.painter().add(egui::Shape::Noop);
                                let mut y_min = f32::INFINITY;
                                let mut y_max = f32::NEG_INFINITY;

                                // Cells are plain labels; interaction is handled
                                // by one full-width region below so clicking
                                // anywhere on the row (gaps included) selects it.
                                for text in cells {
                                    let resp = ui.add(egui::Label::new(text).selectable(false));
                                    y_min = y_min.min(resp.rect.top());
                                    y_max = y_max.max(resp.rect.bottom());
                                }

                                // Full-width row rect: spans the grid's whole
                                // width, covering column gaps and trailing space.
                                let row_rect = egui::Rect::from_x_y_ranges(
                                    ui.max_rect().x_range(),
                                    egui::Rangef::new(y_min, y_max),
                                );

                                // One interactive region over the entire row.
                                let row_resp = ui.interact(
                                    row_rect,
                                    egui::Id::new(("proc_row", key.as_str())),
                                    egui::Sense::click(),
                                );
                                if row_resp.clicked() {
                                    self.selected = Some(key.clone());
                                }
                                self.row_context_menu(&row_resp, key, &stats.name);

                                // Highlight the selected row, or tint on hover.
                                let fill = if selected {
                                    Some(ui.visuals().selection.bg_fill)
                                } else if row_resp.hovered() {
                                    Some(ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.5))
                                } else {
                                    None
                                };
                                if let Some(fill) = fill {
                                    ui.painter().set(
                                        bg_idx,
                                        egui::Shape::rect_filled(row_rect, 3.0, fill),
                                    );
                                }
                                ui.end_row();
                            }
                        });
                });
        });
    }

    fn row_context_menu(&mut self, resp: &egui::Response, key: &str, name: &str) {
        resp.context_menu(|ui| {
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
                ui.label(egui::RichText::new("Leave empty for unlimited.").weak().small());
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Drain any pending snapshots each frame.
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

        // Window close -> tell the backend to shut down.
        if ctx.input(|i| i.viewport().close_requested()) {
            self.send_shutdown();
        }

        let snap = self.last_snapshot.clone();

        // Clear a stale selection so the side panel doesn't linger.
        if let Some(sel) = &self.selected {
            if !snap.processes.contains_key(sel) {
                self.selected = None;
            }
        }

        self.draw_top_panel(ui, &snap);
        self.draw_detail_panel(ui, &snap);
        self.draw_table(ui, &snap);
        self.draw_limit_modal(&ctx);

        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

impl Drop for ThrottleApp {
    fn drop(&mut self) {
        self.send_shutdown();
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
