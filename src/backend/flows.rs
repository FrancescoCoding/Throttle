//! Flow table, per-executable byte counters, and PID-to-path resolution.
//!
//! The flow table maps a network 5-tuple `(protocol, local, remote)` to the
//! owning process (`pid`, lowercase `exe_path`). It is populated from two
//! sources:
//!   * WinDivert flow-layer events (which carry the PID directly), and
//!   * a one-time bootstrap of pre-existing TCP connections via
//!     `GetExtendedTcpTable`.
//!
//! Byte counters are kept per lowercase `exe_path`. The engine/drainer threads
//! call [`FlowTable::add_bytes`] for traffic that actually passes; once per
//! second the aggregator calls [`FlowTable::snapshot`] which converts the
//! accumulated bytes into a per-second rate, folds them into totals + rolling
//! history, and produces the `ProcessStats` the GUI consumes.

use std::collections::HashMap;
use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::core::PWSTR;

use crate::types::{ProcessStats, HISTORY_LEN};

/// IANA protocol number for TCP.
const IPPROTO_TCP: u8 = 6;
/// `AF_INET` (IPv4) address family for the socket-table query.
const AF_INET: u32 = 2;
/// `NO_ERROR` return value from Win32 table APIs.
const NO_ERROR: u32 = 0;

/// Key identifying a single network flow.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FlowKey {
    pub protocol: u8,
    pub local: SocketAddr,
    pub remote: SocketAddr,
}

impl FlowKey {
    fn new(protocol: u8, local: SocketAddr, remote: SocketAddr) -> Self {
        Self {
            protocol,
            local,
            remote,
        }
    }
}

/// The process that owns a flow.
struct FlowEntry {
    #[allow(dead_code)]
    pid: u32,
    /// Lowercase full executable path, or empty if resolution failed.
    exe_path: String,
}

/// Rolling byte counters for one executable path.
#[derive(Default)]
struct ExeCounter {
    /// Representative PID (most recently seen) for display.
    pid: u32,
    down_total: u64,
    up_total: u64,
    /// Bytes accumulated since the last snapshot tick (this becomes the rate).
    down_accum: u64,
    up_accum: u64,
    down_history: Vec<u64>,
    up_history: Vec<u64>,
}

/// Shared flow table. Guarded by a `Mutex` in `Shared`; keep critical sections
/// short (the engine locks this on the packet hot path).
pub struct FlowTable {
    flows: HashMap<FlowKey, FlowEntry>,
    counters: HashMap<String, ExeCounter>,
    /// PID to resolved lowercase path. `None` means resolution was attempted and
    /// failed (avoids re-querying dead/protected PIDs every packet).
    pid_cache: HashMap<u32, Option<String>>,
}

impl Default for FlowTable {
    fn default() -> Self {
        Self::new()
    }
}

impl FlowTable {
    pub fn new() -> Self {
        Self {
            flows: HashMap::new(),
            counters: HashMap::new(),
            pid_cache: HashMap::new(),
        }
    }

    /// Resolve a PID to a lowercase executable path, caching the result.
    fn resolve_path(&mut self, pid: u32) -> Option<String> {
        if let Some(cached) = self.pid_cache.get(&pid) {
            return cached.clone();
        }
        let resolved = query_image_path(pid);
        self.pid_cache.insert(pid, resolved.clone());
        resolved
    }

    /// Insert (or refresh) a flow discovered via a flow-layer event.
    pub fn insert_flow(&mut self, protocol: u8, local: SocketAddr, remote: SocketAddr, pid: u32) {
        let exe_path = self.resolve_path(pid).unwrap_or_default();
        if !exe_path.is_empty() {
            let c = self.counters.entry(exe_path.clone()).or_default();
            c.pid = pid;
        }
        self.flows
            .insert(FlowKey::new(protocol, local, remote), FlowEntry { pid, exe_path });
    }

    /// Remove a flow that has been torn down.
    pub fn remove_flow(&mut self, protocol: u8, local: SocketAddr, remote: SocketAddr) {
        self.flows.remove(&FlowKey::new(protocol, local, remote));
    }

    /// Look up the owning executable path for a packet's `(protocol, local,
    /// remote)`. Tries the swapped orientation as a fallback so a caller that
    /// guessed direction wrong still resolves. Returns `None` for unknown flows
    /// or flows whose path could not be resolved.
    pub fn exe_for(&self, protocol: u8, local: SocketAddr, remote: SocketAddr) -> Option<String> {
        let direct = self.flows.get(&FlowKey::new(protocol, local, remote));
        let entry = direct.or_else(|| self.flows.get(&FlowKey::new(protocol, remote, local)))?;
        if entry.exe_path.is_empty() {
            None
        } else {
            Some(entry.exe_path.clone())
        }
    }

    /// Add passed-through bytes for an executable (called by engine/drainer for
    /// traffic that was actually injected, i.e. real shaped throughput).
    pub fn add_bytes(&mut self, exe_path: &str, down: u64, up: u64) {
        if exe_path.is_empty() {
            return;
        }
        let c = self.counters.entry(exe_path.to_string()).or_default();
        c.down_accum = c.down_accum.saturating_add(down);
        c.up_accum = c.up_accum.saturating_add(up);
    }

    /// Bootstrap the flow table with existing IPv4 TCP connections so processes
    /// that were already communicating before we started are attributed. UDP
    /// and IPv6 bootstrap are skipped (new flows of any kind are still picked
    /// up live via flow events).
    pub fn bootstrap_tcp(&mut self) {
        let rows = match query_tcp_table() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("TCP table bootstrap failed: {e}");
                return;
            }
        };
        let mut inserted = 0usize;
        for row in rows {
            let local = SocketAddr::new(
                IpAddr::V4(Ipv4Addr::from(row.local_addr.to_ne_bytes())),
                port_from_dword(row.local_port),
            );
            let remote = SocketAddr::new(
                IpAddr::V4(Ipv4Addr::from(row.remote_addr.to_ne_bytes())),
                port_from_dword(row.remote_port),
            );
            self.insert_flow(IPPROTO_TCP, local, remote, row.owning_pid);
            inserted += 1;
        }
        tracing::info!("bootstrapped {inserted} existing TCP flow(s)");
    }

    /// Produce a one-second snapshot: convert accumulated bytes into per-second
    /// rates, fold into totals + history, reset accumulators, and build the
    /// `ProcessStats` map. Returns `(processes, total_down_rate, total_up_rate)`.
    pub fn snapshot(&mut self) -> (HashMap<String, ProcessStats>, u64, u64) {
        // Count active flows per executable for the GUI's flow_count column.
        let mut flow_counts: HashMap<String, usize> = HashMap::new();
        for entry in self.flows.values() {
            if !entry.exe_path.is_empty() {
                *flow_counts.entry(entry.exe_path.clone()).or_default() += 1;
            }
        }

        let mut out = HashMap::with_capacity(self.counters.len());
        let mut total_down = 0u64;
        let mut total_up = 0u64;

        for (exe, c) in self.counters.iter_mut() {
            let down_rate = c.down_accum;
            let up_rate = c.up_accum;
            c.down_total = c.down_total.saturating_add(down_rate);
            c.up_total = c.up_total.saturating_add(up_rate);
            c.down_accum = 0;
            c.up_accum = 0;

            push_history(&mut c.down_history, down_rate);
            push_history(&mut c.up_history, up_rate);

            total_down = total_down.saturating_add(down_rate);
            total_up = total_up.saturating_add(up_rate);

            out.insert(
                exe.clone(),
                ProcessStats {
                    pid: c.pid,
                    exe_path: exe.clone(),
                    name: file_name(exe),
                    down_rate,
                    up_rate,
                    down_total: c.down_total,
                    up_total: c.up_total,
                    down_history: c.down_history.clone(),
                    up_history: c.up_history.clone(),
                    flow_count: flow_counts.get(exe).copied().unwrap_or(0),
                },
            );
        }

        (out, total_down, total_up)
    }
}

/// Append `value`, keeping at most `HISTORY_LEN` samples (newest last).
fn push_history(history: &mut Vec<u64>, value: u64) {
    history.push(value);
    if history.len() > HISTORY_LEN {
        let overflow = history.len() - HISTORY_LEN;
        history.drain(0..overflow);
    }
}

/// Extract the filename portion of a Windows or Unix-style path.
fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_string()
}

/// Convert a `MIB_TCP*` port dword (network byte order in the low 16 bits) to a
/// host-order `u16`.
fn port_from_dword(dw: u32) -> u16 {
    let b0 = (dw & 0xff) as u8; // first network byte  = high byte of the port
    let b1 = ((dw >> 8) & 0xff) as u8; // second network byte = low byte of the port
    u16::from_be_bytes([b0, b1])
}

/// Resolve a PID to its lowercase full image path via the Win32 API.
fn query_image_path(pid: u32) -> Option<String> {
    // PID 0 (System Idle) / 4 (System) can't be opened for image name.
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle: HANDLE =
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut buf = vec![0u16; 512];
        let mut size = buf.len() as u32;
        let res = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        res.ok()?;

        if size == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..size as usize]);
        Some(s.to_lowercase())
    }
}

/// Row extracted from `GetExtendedTcpTable` (values kept in their raw
/// network-byte-order form; the caller converts).
struct TcpRow {
    local_addr: u32,
    local_port: u32,
    remote_addr: u32,
    remote_port: u32,
    owning_pid: u32,
}

/// Query the IPv4 TCP connection table (owner-PID variant).
fn query_tcp_table() -> anyhow::Result<Vec<TcpRow>> {
    unsafe {
        // First call: discover required buffer size.
        let mut size: u32 = 0;
        GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
        if size == 0 {
            return Ok(Vec::new());
        }

        let mut buf = vec![0u8; size as usize];
        let ret = GetExtendedTcpTable(
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut size,
            false,
            AF_INET,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
        if ret != NO_ERROR {
            anyhow::bail!("GetExtendedTcpTable returned error code {ret}");
        }

        let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
        let count = table.dwNumEntries as usize;
        // `table.table` is a flexible array member declared with length 1.
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), count);

        let mut out = Vec::with_capacity(count);
        for r in rows {
            out.push(TcpRow {
                local_addr: r.dwLocalAddr,
                local_port: r.dwLocalPort,
                remote_addr: r.dwRemoteAddr,
                remote_port: r.dwRemotePort,
                owning_pid: r.dwOwningPid,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn sa(a: [u8; 4], p: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(a[0], a[1], a[2], a[3])), p)
    }

    #[test]
    fn exe_for_matches_both_orientations() {
        let mut t = FlowTable::new();
        let local = sa([192, 168, 1, 10], 50000);
        let remote = sa([93, 184, 216, 34], 443);
        // Insert directly with a known exe path (bypassing PID resolution).
        t.flows.insert(
            FlowKey::new(IPPROTO_TCP, local, remote),
            FlowEntry {
                pid: 1234,
                exe_path: "c:\\app\\thing.exe".into(),
            },
        );

        assert_eq!(
            t.exe_for(IPPROTO_TCP, local, remote).as_deref(),
            Some("c:\\app\\thing.exe")
        );
        // Swapped orientation (e.g. inbound packet) still resolves.
        assert_eq!(
            t.exe_for(IPPROTO_TCP, remote, local).as_deref(),
            Some("c:\\app\\thing.exe")
        );
        // Unknown protocol does not.
        assert_eq!(t.exe_for(17, local, remote), None);
    }

    #[test]
    fn snapshot_computes_rates_totals_and_history() {
        let mut t = FlowTable::new();
        t.add_bytes("a.exe", 1000, 200);
        let (procs, td, tu) = t.snapshot();
        let s = procs.get("a.exe").unwrap();
        assert_eq!(s.down_rate, 1000);
        assert_eq!(s.up_rate, 200);
        assert_eq!(s.down_total, 1000);
        assert_eq!(s.up_total, 200);
        assert_eq!(s.down_history, vec![1000]);
        assert_eq!(td, 1000);
        assert_eq!(tu, 200);
        assert_eq!(s.name, "a.exe");

        // Second interval: accumulators reset, totals accumulate, history grows.
        t.add_bytes("a.exe", 500, 0);
        let (procs, _, _) = t.snapshot();
        let s = procs.get("a.exe").unwrap();
        assert_eq!(s.down_rate, 500);
        assert_eq!(s.down_total, 1500);
        assert_eq!(s.down_history, vec![1000, 500]);
    }

    #[test]
    fn history_is_capped_at_history_len() {
        let mut t = FlowTable::new();
        for i in 0..(HISTORY_LEN + 10) {
            t.add_bytes("x.exe", i as u64, 0);
            t.snapshot();
        }
        let (procs, _, _) = t.snapshot();
        let s = procs.get("x.exe").unwrap();
        assert_eq!(s.down_history.len(), HISTORY_LEN);
        // Newest sample (last snapshot added 0) is last.
        assert_eq!(*s.down_history.last().unwrap(), 0);
    }

    #[test]
    fn port_dword_conversion_is_network_order() {
        // Port 443 in network byte order is bytes [0x01, 0xBB]; stored in the
        // low word of a dword on a little-endian machine that is 0x0000BB01.
        let dw = 0x0000_BB01u32;
        assert_eq!(port_from_dword(dw), 443);
    }

    #[test]
    fn file_name_handles_windows_paths() {
        assert_eq!(file_name("c:\\a\\b\\thing.exe"), "thing.exe");
        assert_eq!(file_name("/usr/bin/curl"), "curl");
        assert_eq!(file_name("bare.exe"), "bare.exe");
    }
}
