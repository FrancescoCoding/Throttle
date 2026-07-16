//! Shared types between the capture/limiter backend and the GUI.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Seconds of rate history kept per process for sparklines.
pub const HISTORY_LEN: usize = 60;

/// A per-process traffic rule, matched by lowercase executable path.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    /// Full lowercase path of the executable this rule applies to.
    pub exe_path: String,
    /// Download limit in bytes/sec. `None` = unlimited.
    pub down_limit: Option<u64>,
    /// Upload limit in bytes/sec. `None` = unlimited.
    pub up_limit: Option<u64>,
    /// If true, all traffic for this process is dropped.
    pub blocked: bool,
}

/// Live stats for one process, sent to the GUI once per second.
#[derive(Debug, Clone, Default)]
pub struct ProcessStats {
    pub pid: u32,
    /// Full executable path (may be empty if resolution failed).
    pub exe_path: String,
    /// File name portion, for display.
    pub name: String,
    /// Current download rate, bytes/sec.
    pub down_rate: u64,
    /// Current upload rate, bytes/sec.
    pub up_rate: u64,
    /// Total bytes since start.
    pub down_total: u64,
    pub up_total: u64,
    /// Rolling per-second download rates, newest last, up to HISTORY_LEN.
    pub down_history: Vec<u64>,
    /// Rolling per-second upload rates, newest last, up to HISTORY_LEN.
    pub up_history: Vec<u64>,
    /// Number of active flows attributed to this process.
    pub flow_count: usize,
}

/// Snapshot of everything the GUI needs, produced ~1x/sec by the backend.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Keyed by lowercase exe_path (processes aggregated across PIDs share a key).
    pub processes: HashMap<String, ProcessStats>,
    /// Total current rates across all processes, bytes/sec.
    pub total_down_rate: u64,
    pub total_up_rate: u64,
    /// Currently active rules, keyed by lowercase exe_path.
    pub rules: HashMap<String, Rule>,
}

/// Commands sent from the GUI to the backend.
#[derive(Debug, Clone)]
pub enum Command {
    /// Insert or replace the rule for `rule.exe_path`.
    SetRule(Rule),
    /// Remove any rule for this lowercase exe path.
    RemoveRule(String),
    /// Stop the engine threads and flush state (app exit).
    Shutdown,
}
