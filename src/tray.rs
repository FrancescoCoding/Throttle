//! Tray icon: menu with rule toggles, tooltip and icon colours.

use crossbeam_channel::Sender;
use eframe::egui;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::gui::rule_summary;
use crate::types::Snapshot;
use crate::units::humanize_rate;
use crate::{icon, theme};

/// Ids of the fixed tray menu entries; rule entries use `TRAY_RULE_PREFIX`.
const TRAY_SHOW_ID: &str = "show";
const TRAY_EXIT_ID: &str = "exit";
/// Tray menu id prefix for a rule toggle; the rest is the rule's exe key.
const TRAY_RULE_PREFIX: &str = "rule:";
/// Most rule toggles the tray menu lists before it points to the window.
const TRAY_RULE_LIMIT: usize = 15;

/// What the tray icon (or a second launch) asks the app to do.
pub enum TrayAction {
    Show,
    Exit,
    /// Switch the rule with this exe key off, or back on.
    ToggleRule(String),
}

/// One rule toggle in the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayRule {
    /// Lowercase exe path, the rule's key.
    pub key: String,
    /// Process name and what the rule does.
    pub label: String,
    /// The rule is active (the entry shows a check mark).
    pub active: bool,
}

/// Create the tray icon: left click shows the window, right click opens a
/// Show / Exit menu. Returns `None` (logged) if it cannot be created. If the
/// taskbar does not exist yet, tray-icon still succeeds and adds the icon when
/// Explorer announces the taskbar (see `taskbar_ready`).
pub fn create_tray(ctx: &egui::Context, tx: Sender<TrayAction>) -> Option<TrayIcon> {
    let menu = menu(&[]);

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
    // Entries are known by their string ids, so the menu can be rebuilt
    // without touching this handler.
    let ctx = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let id = event.id.0.as_str();
        let action = if id == TRAY_SHOW_ID {
            TrayAction::Show
        } else if id == TRAY_EXIT_ID {
            TrayAction::Exit
        } else if let Some(key) = id.strip_prefix(TRAY_RULE_PREFIX) {
            TrayAction::ToggleRule(key.to_string())
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

/// The tray menu: Show, one check entry per rule (checked while the rule is
/// active; clicking switches it off or on), Exit. With no rules the menu is
/// the two entries it always had.
pub fn menu(rules: &[TrayRule]) -> Menu {
    let menu = Menu::new();
    let show = MenuItem::with_id(TRAY_SHOW_ID, "Show Throttle", true, None);
    let exit = MenuItem::with_id(TRAY_EXIT_ID, "Exit", true, None);
    let mut result = menu.append_items(&[&show, &PredefinedMenuItem::separator()]);
    if !rules.is_empty() {
        for rule in rules.iter().take(TRAY_RULE_LIMIT) {
            let id = format!("{TRAY_RULE_PREFIX}{}", rule.key);
            let item = CheckMenuItem::with_id(id, &rule.label, true, rule.active, None);
            result = result.and(menu.append(&item));
        }
        if rules.len() > TRAY_RULE_LIMIT {
            let more = rules.len() - TRAY_RULE_LIMIT;
            let note = MenuItem::new(format!("{more} more in the window"), false, None);
            result = result.and(menu.append(&note));
        }
        result = result.and(menu.append(&PredefinedMenuItem::separator()));
    }
    result = result.and(menu.append(&exit));
    if let Err(e) = result {
        tracing::warn!("tray menu incomplete: {e}");
    }
    menu
}

/// The rule toggles for the tray menu, by process name. A rule whose
/// process is not running is named after its executable; one that neither
/// limits nor blocks has nothing to switch and is left out.
pub fn rules(snap: &Snapshot) -> Vec<TrayRule> {
    let mut rules: Vec<TrayRule> = snap
        .rules
        .values()
        .filter(|r| r.blocked || r.down_limit.is_some() || r.up_limit.is_some())
        .map(|rule| {
            let name = snap
                .processes
                .get(&rule.exe_path)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| exe_name(&rule.exe_path));
            let mut active = rule.clone();
            active.disabled = false;
            TrayRule {
                key: rule.exe_path.clone(),
                label: format!("{name}: {}", rule_summary(Some(&active))),
                active: !rule.disabled,
            }
        })
        .collect();
    rules.sort_by_key(|r| r.label.to_lowercase());
    rules
}

/// The tray tooltip: the live download and upload totals.
pub fn tooltip(snap: &Snapshot) -> String {
    format!(
        "Throttle\nDown {}\nUp {}",
        humanize_rate(snap.total_down_rate),
        humanize_rate(snap.total_up_rate)
    )
}

/// The file name of an exe path, or the path itself if it has none.
fn exe_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| path.to_string())
}

/// Tray icon pixels for `accents`. The stock accents keep the stock icon,
/// whose arrow colours predate the theme accents, so the default look is
/// unchanged.
pub fn icon_rgba(accents: theme::Accents) -> Vec<u8> {
    let stock = theme::Accents::DEFAULT;
    if (accents.down, accents.up) == (stock.down, stock.up) {
        icon::rgba()
    } else {
        let rgb = |c: egui::Color32| [c.r(), c.g(), c.b()];
        icon::rgba_with(rgb(accents.down), rgb(accents.up))
    }
}

/// Whether Explorer's taskbar (and so the notification area) exists.
pub fn taskbar_ready() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    use windows::core::w;
    // SAFETY: FindWindowW with a valid static class name; a miss is an error.
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.is_ok_and(|h| !h.is_invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ProcessStats, Rule};

    #[test]
    fn rules_name_running_processes_and_sort_by_label() {
        let mut snap = Snapshot::default();
        let steam = "c:\\steam\\steam.exe".to_string();
        let curl = "c:\\tools\\curl.exe".to_string();
        snap.processes.insert(
            steam.clone(),
            ProcessStats {
                name: "Steam.exe".to_string(),
                exe_path: steam.clone(),
                ..ProcessStats::default()
            },
        );
        snap.rules.insert(
            steam.clone(),
            Rule {
                exe_path: steam.clone(),
                down_limit: Some(1024 * 1024),
                disabled: true,
                ..Rule::default()
            },
        );
        snap.rules.insert(
            curl.clone(),
            Rule {
                exe_path: curl.clone(),
                blocked: true,
                ..Rule::default()
            },
        );
        // A rule with nothing to apply is not worth a toggle.
        snap.rules.insert(
            r"c:\idle.exe".to_string(),
            Rule {
                exe_path: r"c:\idle.exe".to_string(),
                ..Rule::default()
            },
        );
        let rules = rules(&snap);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].label, "curl.exe: BLOCKED");
        assert!(rules[0].active);
        // Disabled rules still show what they would do, unchecked.
        assert_eq!(rules[1].label, "Steam.exe: Dn 1.00 MB/s");
        assert!(!rules[1].active);
        assert_eq!(rules[1].key, steam);
    }

    #[test]
    fn stock_accents_keep_the_stock_tray_icon() {
        assert_eq!(icon_rgba(theme::Accents::DEFAULT), icon::rgba());
    }
}
