//! Flow table and per-executable byte counters.
//!
//! Platform access (socket tables, PID resolution) lives in [`super::sockets`];
//! the synthetic "Unknown"/"System" row policy lives in [`super::synthetic`].
//!
//! The flow table maps a network 5-tuple `(protocol, local, remote)` to the
//! owning process (`pid`, lowercase `exe_path`). It is populated from two
//! sources:
//!   * WinDivert flow-layer events (which carry the PID directly), and
//!   * a bootstrap (and periodic refresh) of the kernel socket tables via
//!     `GetExtendedTcpTable` / `GetExtendedUdpTable`, IPv4 and IPv6.
//!
//! Because a UDP socket is often bound to `0.0.0.0` and talks to many remotes,
//! an exact 5-tuple lookup misses most QUIC/UDP traffic. A secondary
//! `(protocol, family, local port)` map is therefore maintained alongside the
//! flow map and used as a fallback ([`FlowTable::exe_for`]). That map is
//! rebuilt from scratch on every successful table refresh so ports belonging
//! to closed sockets cannot misattribute a later socket that reuses them.
//!
//! Byte counters are kept per lowercase `exe_path`. The engine/drainer threads
//! call [`FlowTable::add_bytes`] for traffic that actually passes; traffic that
//! cannot be attributed at all is counted under [`UNKNOWN_EXE`] via
//! [`FlowTable::add_unattributed`] so totals still reflect real link usage.
//! Once per second the aggregator calls [`FlowTable::snapshot`] which converts
//! the accumulated bytes into a per-second rate, folds them into totals +
//! rolling history, and produces the `ProcessStats` the GUI consumes.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;

use super::sockets::{SocketSnapshot, query_socket_tables, resolve_pid_path};
use super::synthetic::{UNKNOWN_EXE, display_name};
use crate::types::{HISTORY_LEN, ProcessStats};

/// Key of the port-owner fallback map: `(protocol, is_ipv6, local port)`. The
/// address family is part of the key because a v4 and a v6 socket may bind the
/// same port in different processes (mDNS, SSDP, dev servers).
type PortKey = (u8, bool, u16);

/// Build the fallback-map key for a local endpoint.
fn port_key(protocol: u8, local: SocketAddr) -> PortKey {
    (protocol, local.is_ipv6(), local.port())
}

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
    /// Fallback owner map: `(protocol, family, local port)` to lowercase exe
    /// path. Used when the exact 5-tuple is unknown (wildcard-bound or
    /// unconnected UDP sockets, flows established before we saw their event,
    /// ...). Replaced wholesale by every successful table refresh.
    port_owners: HashMap<PortKey, String>,
    /// Entries learned from flow-layer events since the last table refresh.
    /// They are newer than any table read, so they are re-applied on top of a
    /// freshly rebuilt `port_owners` and then cleared.
    recent_port_owners: HashMap<PortKey, String>,
    counters: HashMap<String, ExeCounter>,
    /// PID to resolved lowercase path. `None` means the PID is deliberately
    /// ignored (PID 0, the idle process).
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
            port_owners: HashMap::new(),
            recent_port_owners: HashMap::new(),
            counters: HashMap::new(),
            pid_cache: HashMap::new(),
        }
    }

    /// Resolve a PID to a lowercase executable path, caching the result.
    ///
    /// Unlike a raw image-path query this never gives up: PID 4 becomes
    /// `system` and any PID we cannot open becomes `pid:<n>`, so its traffic is
    /// still attributed to a stable row instead of being discarded. PID 0 (the
    /// idle process) is the one exception and yields `None`.
    fn resolve_path(&mut self, pid: u32) -> Option<String> {
        if let Some(cached) = self.pid_cache.get(&pid) {
            return cached.clone();
        }
        let resolved = resolve_pid_path(pid);
        self.pid_cache.insert(pid, resolved.clone());
        resolved
    }

    /// Insert (or refresh) a flow discovered via a flow-layer event. Also
    /// records the `(protocol, family, local port)` fallback mapping.
    pub fn insert_flow(&mut self, protocol: u8, local: SocketAddr, remote: SocketAddr, pid: u32) {
        self.insert_flow_inner(protocol, local, remote, pid, true);
    }

    /// `recent` marks the fallback entry as learned from a live event, so it
    /// survives the next authoritative rebuild of the port map.
    fn insert_flow_inner(
        &mut self,
        protocol: u8,
        local: SocketAddr,
        remote: SocketAddr,
        pid: u32,
        recent: bool,
    ) {
        let exe_path = self.resolve_path(pid).unwrap_or_default();
        if !exe_path.is_empty() {
            let c = self.counters.entry(exe_path.clone()).or_default();
            c.pid = pid;
            self.note_port_owner(protocol, local, exe_path.clone(), recent);
        }
        self.flows.insert(
            FlowKey::new(protocol, local, remote),
            FlowEntry { pid, exe_path },
        );
    }

    /// Record only the port ownership, without a 5-tuple. Used for listening /
    /// unconnected sockets from the UDP table. `recent` marks the entry as
    /// learned from a live event, so it survives the next authoritative rebuild
    /// of the port map.
    fn insert_port_owner_inner(&mut self, protocol: u8, local: SocketAddr, pid: u32, recent: bool) {
        let exe_path = self.resolve_path(pid).unwrap_or_default();
        if exe_path.is_empty() {
            return;
        }
        let c = self.counters.entry(exe_path.clone()).or_default();
        c.pid = pid;
        self.note_port_owner(protocol, local, exe_path, recent);
    }

    /// Write one fallback entry, mirroring it into the "learned since the last
    /// refresh" overlay when it came from a live event.
    fn note_port_owner(&mut self, protocol: u8, local: SocketAddr, exe: String, recent: bool) {
        if local.port() == 0 {
            return;
        }
        let key = port_key(protocol, local);
        if recent {
            self.recent_port_owners.insert(key, exe.clone());
        }
        self.port_owners.insert(key, exe);
    }

    /// Remove a flow that has been torn down. The port-owner fallback entry is
    /// intentionally kept here: a wildcard UDP socket outlives individual
    /// flows. Entries for sockets that are really gone are dropped by the next
    /// authoritative table refresh, which rebuilds the map.
    pub fn remove_flow(&mut self, protocol: u8, local: SocketAddr, remote: SocketAddr) {
        self.flows.remove(&FlowKey::new(protocol, local, remote));
    }

    /// Look up the owning executable path for a packet's `(protocol, local,
    /// remote)`.
    ///
    /// Three attempts, in order of decreasing precision:
    ///   1. the exact 5-tuple,
    ///   2. the swapped orientation (so a caller that guessed direction wrong
    ///      still resolves),
    ///   3. the `(protocol, family, local port)` fallback, which catches
    ///      wildcard-bound UDP sockets (QUIC) and any flow whose event we never
    ///      saw. The *remote* port is deliberately never used: it would hand
    ///      every DNS or HTTPS packet to whichever process happens to own the
    ///      matching local port.
    pub fn exe_for(&self, protocol: u8, local: SocketAddr, remote: SocketAddr) -> Option<String> {
        let direct = self
            .flows
            .get(&FlowKey::new(protocol, local, remote))
            .or_else(|| self.flows.get(&FlowKey::new(protocol, remote, local)));
        if let Some(entry) = direct
            && !entry.exe_path.is_empty()
        {
            return Some(entry.exe_path.clone());
        }
        self.port_owners.get(&port_key(protocol, local)).cloned()
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

    /// Count bytes that passed but could not be attributed to any process, so
    /// they still appear in the totals and in the "Unknown" row.
    pub fn add_unattributed(&mut self, down: u64, up: u64) {
        self.add_bytes(UNKNOWN_EXE, down, up);
    }

    /// PIDs already resolved in the cache. Handed to [`query_socket_tables`]
    /// so it only pays for `OpenProcess` on genuinely new PIDs - and does that
    /// work without holding this table's mutex.
    pub fn known_pids(&self) -> HashSet<u32> {
        self.pid_cache.keys().copied().collect()
    }

    /// Apply a socket-table snapshot gathered outside the lock. Returns the
    /// number of rows applied.
    ///
    /// A `complete` snapshot (every table query succeeded) is authoritative:
    /// `port_owners` is rebuilt from it, so ports belonging to sockets that
    /// have since closed disappear instead of misattributing whichever process
    /// later reuses the ephemeral port. Entries learned from flow events since
    /// the previous refresh are re-applied on top.
    pub fn apply_socket_snapshot(&mut self, snap: SocketSnapshot) -> usize {
        for (pid, path) in snap.resolved {
            self.pid_cache.entry(pid).or_insert(path);
        }

        if snap.complete {
            let mut fresh: HashMap<PortKey, String> = HashMap::new();
            for row in &snap.rows {
                if row.local.port() == 0 {
                    continue;
                }
                if let Some(exe) = self.resolve_path(row.pid) {
                    fresh.insert(port_key(row.protocol, row.local), exe);
                }
            }
            // Flow-event entries are newer than the table read; keep them.
            let recent: Vec<(PortKey, String)> = self.recent_port_owners.drain().collect();
            for (k, v) in recent {
                fresh.insert(k, v);
            }
            self.port_owners = fresh;
        }

        for row in &snap.rows {
            match row.remote {
                // TCP rows carry a peer, so they are a full 5-tuple.
                Some(remote) => {
                    self.insert_flow_inner(row.protocol, row.local, remote, row.pid, false)
                }
                // UDP table rows carry no remote address: a wildcard-bound
                // socket can talk to anyone, so only the port mapping is
                // meaningful.
                None => self.insert_port_owner_inner(row.protocol, row.local, row.pid, false),
            }
        }
        snap.rows.len()
    }

    /// Bootstrap at startup, logging what was learned.
    pub fn bootstrap_tcp(&mut self) {
        let snap = query_socket_tables(&self.known_pids());
        let n = self.apply_socket_snapshot(snap);
        tracing::info!("bootstrapped {n} existing socket-table entries (TCP+UDP, v4+v6)");
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
                    name: display_name(exe),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::sockets::{IPPROTO_TCP, IPPROTO_UDP};

    use std::net::{IpAddr, Ipv4Addr};

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
    fn port_fallback_resolves_wildcard_udp_sockets() {
        let mut t = FlowTable::new();
        // A QUIC socket bound to 0.0.0.0:51000, learned only as a port owner.
        t.port_owners
            .insert((IPPROTO_UDP, false, 51000), "c:\\app\\brave.exe".into());

        let local = sa([192, 168, 1, 10], 51000);
        let remote = sa([142, 250, 1, 1], 443);
        // No 5-tuple entry exists, but the local port resolves.
        assert_eq!(
            t.exe_for(IPPROTO_UDP, local, remote).as_deref(),
            Some("c:\\app\\brave.exe")
        );
        // The *remote* port is never used as a fallback: otherwise any owner of
        // a popular local port would absorb unrelated traffic.
        assert_eq!(t.exe_for(IPPROTO_UDP, remote, local), None);
        // A different protocol on the same port does not match.
        assert_eq!(t.exe_for(IPPROTO_TCP, local, remote), None);
    }

    #[test]
    fn port_fallback_is_address_family_specific() {
        let mut t = FlowTable::new();
        // Only a v6 socket owns port 5353.
        t.port_owners
            .insert((IPPROTO_UDP, true, 5353), "c:\\app\\v6only.exe".into());

        let v4_local = sa([192, 168, 1, 10], 5353);
        let v4_remote = sa([224, 0, 0, 251], 5353);
        assert_eq!(t.exe_for(IPPROTO_UDP, v4_local, v4_remote), None);

        let v6_local: SocketAddr = "[fe80::1]:5353".parse().unwrap();
        let v6_remote: SocketAddr = "[ff02::fb]:5353".parse().unwrap();
        assert_eq!(
            t.exe_for(IPPROTO_UDP, v6_local, v6_remote).as_deref(),
            Some("c:\\app\\v6only.exe")
        );
    }

    #[test]
    fn authoritative_refresh_drops_stale_ports_and_keeps_recent_ones() {
        let mut t = FlowTable::new();
        // A port learned from an old table read, whose socket has since closed.
        t.port_owners
            .insert((IPPROTO_UDP, false, 51000), "c:\\app\\dead.exe".into());
        // A port learned from a live flow event after that read.
        t.recent_port_owners
            .insert((IPPROTO_UDP, false, 51001), "c:\\app\\live.exe".into());

        // An empty but complete sweep: no sockets exist any more.
        t.apply_socket_snapshot(SocketSnapshot {
            rows: Vec::new(),
            resolved: Vec::new(),
            complete: true,
        });

        let local_dead = sa([192, 168, 1, 10], 51000);
        let local_live = sa([192, 168, 1, 10], 51001);
        let remote = sa([142, 250, 1, 1], 443);
        assert_eq!(t.exe_for(IPPROTO_UDP, local_dead, remote), None);
        assert_eq!(
            t.exe_for(IPPROTO_UDP, local_live, remote).as_deref(),
            Some("c:\\app\\live.exe")
        );
        // The overlay is consumed by the rebuild.
        assert!(t.recent_port_owners.is_empty());
    }

    #[test]
    fn partial_refresh_is_not_authoritative() {
        let mut t = FlowTable::new();
        t.port_owners
            .insert((IPPROTO_UDP, false, 51000), "c:\\app\\brave.exe".into());
        // A sweep where a table query failed must not wipe what we know.
        t.apply_socket_snapshot(SocketSnapshot {
            rows: Vec::new(),
            resolved: Vec::new(),
            complete: false,
        });
        let local = sa([192, 168, 1, 10], 51000);
        let remote = sa([142, 250, 1, 1], 443);
        assert_eq!(
            t.exe_for(IPPROTO_UDP, local, remote).as_deref(),
            Some("c:\\app\\brave.exe")
        );
    }

    #[test]
    fn exact_flow_wins_over_port_fallback() {
        let mut t = FlowTable::new();
        let local = sa([192, 168, 1, 10], 51000);
        let remote = sa([142, 250, 1, 1], 443);
        t.port_owners
            .insert((IPPROTO_UDP, false, 51000), "c:\\app\\other.exe".into());
        t.flows.insert(
            FlowKey::new(IPPROTO_UDP, local, remote),
            FlowEntry {
                pid: 7,
                exe_path: "c:\\app\\exact.exe".into(),
            },
        );
        assert_eq!(
            t.exe_for(IPPROTO_UDP, local, remote).as_deref(),
            Some("c:\\app\\exact.exe")
        );
    }

    #[test]
    fn unattributed_bytes_land_in_the_unknown_row() {
        let mut t = FlowTable::new();
        t.add_unattributed(4000, 500);
        let (procs, td, tu) = t.snapshot();
        let s = procs.get(UNKNOWN_EXE).unwrap();
        assert_eq!(s.name, "Unknown");
        assert_eq!(s.down_rate, 4000);
        assert_eq!(s.up_rate, 500);
        // Unattributed traffic is part of the link totals.
        assert_eq!(td, 4000);
        assert_eq!(tu, 500);
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
}
