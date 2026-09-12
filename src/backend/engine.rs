//! WinDivert network-layer capture + token-bucket traffic shaping.
//!
//! Two threads cooperate here:
//!   * [`run_engine`] receives every packet, attributes it to a process via the
//!     flow table, looks up that process's rule, and applies a verdict: pass,
//!     drop (block), or, if a speed limit applies and there aren't enough
//!     tokens, enqueue the packet in a per-`(exe, direction)` bucket for later
//!     injection.
//!   * [`run_drainer`] wakes every few milliseconds, refills the buckets, and
//!     re-injects queued packets as tokens become available.
//!
//! Both threads share one WinDivert handle (see [`EngineHandle`]).
//!
//! Byte counting reflects shaped throughput: bytes are only counted when a
//! packet is actually injected (immediately by the engine, or later by the
//! drainer). Dropped/blocked packets are not counted.

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windivert::prelude::*;
use windivert_sys::ChecksumFlags;

use super::synthetic;
use super::Shared;

/// Receive buffer size. WinDivert never hands us anything larger than a single
/// (possibly offloaded) IP packet; 64 KiB covers the maximum.
const RECV_BUFFER: usize = 65_535;
/// How often the drainer wakes to refill buckets and flush queues.
const DRAIN_INTERVAL: Duration = Duration::from_millis(5);
/// Per-bucket queue caps (whichever is hit first). Beyond these, packets are
/// dropped, which for TCP just applies additional back-pressure.
const MAX_QUEUE_PKTS: usize = 256;
const MAX_QUEUE_BYTES: usize = 1024 * 1024;
/// How many distinct unattributed 5-tuples to log per 5s report.
const UNATTRIBUTED_SAMPLES: usize = 5;

/// The single network-layer WinDivert handle, shared by the engine (recv+send)
/// and drainer (send) threads.
///
/// A second handle is not an option: re-injected packets are re-presented to
/// network-layer handles of lower priority, and a lower-priority handle
/// silently drops them, breaking connectivity. Injecting through the same
/// handle that captured the packet skips re-capture entirely.
///
/// Safety: the WinDivert C API documents handles as thread-safe; the Rust
/// wrapper is conservative in not implementing Send/Sync.
pub struct EngineHandle(pub WinDivert<NetworkLayer>);
unsafe impl Send for EngineHandle {}
unsafe impl Sync for EngineHandle {}

/// WinDivert filter for the network-layer handle.
///
/// Loopback traffic is excluded on purpose. It is never link traffic, so there
/// is nothing to meter or shape, and re-injecting it is harmful: security
/// software such as NordVPN Threat Protection redirects browser connections to
/// a local proxy through a WFP connect-redirect, and a re-injected loopback
/// packet loses that redirect state, so the connection never completes and the
/// browser reports "connection closed" while every non-browser app still works.
const NETWORK_FILTER: &str = "!loopback";

/// Open the shared network-layer capture handle. Fails if not elevated or if
/// the WinDivert driver files are missing.
pub fn open() -> Result<EngineHandle, WinDivertError> {
    Ok(EngineHandle(WinDivert::network(
        NETWORK_FILTER,
        0,
        WinDivertFlags::new(),
    )?))
}

/// Traffic direction, from the local host's point of view.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Dir {
    /// Inbound / download.
    Down,
    /// Outbound / upload.
    Up,
}

/// Bucket identity: one token bucket per executable per direction.
pub type BucketKey = (String, Dir);

/// A token bucket plus the queue of packets waiting on it.
pub struct ShapedBucket {
    pub tb: TokenBucket,
    pub queue: VecDeque<WinDivertPacket<'static, NetworkLayer>>,
    pub queued_bytes: usize,
}

impl ShapedBucket {
    pub fn new(rate: u64) -> Self {
        Self {
            tb: TokenBucket::new(rate),
            queue: VecDeque::new(),
            queued_bytes: 0,
        }
    }
}

/// A byte-denominated token bucket. Capacity equals `rate`, giving a one-second
/// burst allowance; tokens refill continuously at `rate` bytes/second.
#[derive(Debug)]
pub struct TokenBucket {
    rate: u64,
    capacity: f64,
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    pub fn new(rate: u64) -> Self {
        Self::new_at(rate, Instant::now())
    }

    /// Construct with an explicit start instant (used by tests for determinism).
    pub fn new_at(rate: u64, now: Instant) -> Self {
        let capacity = rate as f64;
        Self {
            rate,
            capacity,
            tokens: capacity,
            last: now,
        }
    }

    /// Update the fill rate (e.g. after a rule edit), clamping current tokens to
    /// the new capacity.
    pub fn set_rate(&mut self, rate: u64) {
        if rate != self.rate {
            self.rate = rate;
            self.capacity = rate as f64;
            if self.tokens > self.capacity {
                self.tokens = self.capacity;
            }
        }
    }

    /// Add tokens for elapsed time since the last refill, capped at capacity.
    pub fn refill(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        if dt > 0.0 {
            self.tokens = (self.tokens + dt * self.rate as f64).min(self.capacity);
            self.last = now;
        }
    }

    /// Try to spend `n` bytes' worth of tokens. Returns `true` if consumed.
    ///
    /// A packet larger than the whole capacity can never fit under the normal
    /// rule, so as a special case we let it through once the bucket is full
    /// (draining all tokens); otherwise such a packet would stall forever.
    pub fn try_take(&mut self, n: u64) -> bool {
        let n = n as f64;
        if n > self.capacity {
            if self.tokens >= self.capacity {
                self.tokens = 0.0;
                true
            } else {
                false
            }
        } else if self.tokens >= n {
            self.tokens -= n;
            true
        } else {
            false
        }
    }
}

/// Engine thread entry point: capture, classify, verdict, inject.
pub fn run_engine(shared: Arc<Shared>, handle: Arc<EngineHandle>) {
    let divert = &handle.0;
    tracing::info!("engine thread started");

    let mut buf = vec![0u8; RECV_BUFFER];
    let mut stats = EngineStats::default();
    let mut last_report = Instant::now();
    while shared.running.load(Ordering::Relaxed) {
        let packet = match divert.recv(Some(&mut buf)) {
            Ok(p) => p,
            Err(WinDivertError::Recv(WinDivertRecvError::NoData)) => break,
            Err(e) => {
                stats.recv_err += 1;
                tracing::warn!("engine: recv error: {e}");
                continue;
            }
        };
        stats.recv_ok += 1;
        // Own the packet and recalculate checksums before any re-injection.
        // Captured packets can carry incomplete checksums due to hardware
        // offload; re-injecting them unmodified makes WinDivertSend report
        // success while the packets are dropped downstream.
        let mut packet = packet.into_owned();
        if packet.recalculate_checksums(ChecksumFlags::new()).is_err() {
            stats.calc_err += 1;
        }
        handle_packet(divert, &shared, packet, &mut stats);

        if last_report.elapsed() >= Duration::from_secs(5) {
            tracing::info!(
                "engine 5s: recv_ok={} recv_err={} sent={} send_err={} calc_err={} unattributed={} queued={} dropped={}",
                stats.recv_ok, stats.recv_err, stats.sent, stats.send_err,
                stats.calc_err, stats.unattributed, stats.queued, stats.dropped
            );
            for sample in &stats.unattributed_samples {
                tracing::info!("engine 5s: unattributed sample {sample}");
            }
            stats = EngineStats::default();
            last_report = Instant::now();
        }
    }
    tracing::info!("engine thread exiting");
}

/// Rolling diagnostics for the engine loop, reported every 5 seconds.
#[derive(Default)]
struct EngineStats {
    recv_ok: u64,
    recv_err: u64,
    sent: u64,
    send_err: u64,
    calc_err: u64,
    unattributed: u64,
    queued: u64,
    dropped: u64,
    /// Up to `UNATTRIBUTED_SAMPLES` example 5-tuples we failed to attribute,
    /// logged with the 5s report so misattribution can be diagnosed.
    unattributed_samples: Vec<String>,
}

/// Classify one captured packet and apply a shaping verdict.
fn handle_packet(
    divert: &WinDivert<NetworkLayer>,
    shared: &Shared,
    packet: WinDivertPacket<'static, NetworkLayer>,
    stats: &mut EngineStats,
) {
    let outbound = packet.address.outbound();
    let len = packet.data.len() as u64;

    // Attribute the packet to a process via the flow table.
    let parsed = parse_packet(&packet.data);
    let exe = match parsed {
        Some((proto, src, dst)) => {
            let (local, remote) = if outbound { (src, dst) } else { (dst, src) };
            shared
                .flows
                .lock()
                .unwrap()
                .exe_for(proto, local, remote)
        }
        None => None,
    };

    if exe.is_none() {
        stats.unattributed += 1;
        if stats.unattributed_samples.len() < UNATTRIBUTED_SAMPLES {
            let desc = match parsed {
                Some((proto, src, dst)) => {
                    format!("proto={proto} src={src} dst={dst} outbound={outbound} len={len}")
                }
                None => format!("unparsed len={len} outbound={outbound}"),
            };
            if !stats.unattributed_samples.contains(&desc) {
                stats.unattributed_samples.push(desc);
            }
        }
    }

    // Look up the rule (cloned so we don't hold the rules lock). The "Unknown"
    // row is never shaped: see `synthetic::is_shapable`.
    let rule = exe
        .as_ref()
        .filter(|e| synthetic::is_shapable(e))
        .and_then(|e| shared.rules.lock().unwrap().get(e).cloned());

    match rule {
        // No process match, or no rule: pass through untouched.
        None => send_and_count(divert, shared, &packet, exe.as_deref(), outbound, len, stats),
        // Blocked: drop everything for this process.
        Some(r) if r.blocked => {
            stats.dropped += 1;
        }
        Some(r) => {
            let limit = if outbound { r.up_limit } else { r.down_limit };
            match limit {
                // This direction is unlimited: pass through.
                None => send_and_count(divert, shared, &packet, exe.as_deref(), outbound, len, stats),
                // An explicit zero limit means "block this direction".
                Some(0) => {
                    stats.dropped += 1;
                }
                Some(rate) => {
                    let exe = exe.expect("rule implies a resolved exe path");
                    let dir = if outbound { Dir::Up } else { Dir::Down };
                    let plen = packet.data.len();

                    let mut buckets = shared.buckets.lock().unwrap();
                    let bucket = buckets
                        .entry((exe.clone(), dir))
                        .or_insert_with(|| ShapedBucket::new(rate));
                    bucket.tb.set_rate(rate);
                    bucket.tb.refill(Instant::now());

                    if bucket.tb.try_take(len) {
                        // Tokens available: release the lock and inject now.
                        drop(buckets);
                        send_and_count(divert, shared, &packet, Some(&exe), outbound, len, stats);
                    } else if bucket.queue.len() < MAX_QUEUE_PKTS
                        && bucket.queued_bytes + plen <= MAX_QUEUE_BYTES
                    {
                        // Hold the packet for the drainer to inject later.
                        bucket.queued_bytes += plen;
                        bucket.queue.push_back(packet);
                        stats.queued += 1;
                    } else {
                        // Queue full: drop (back-pressure).
                        stats.dropped += 1;
                    }
                }
            }
        }
    }
}

/// Inject a packet and, if attributed, count its bytes as shaped throughput.
fn send_and_count(
    divert: &WinDivert<NetworkLayer>,
    shared: &Shared,
    packet: &WinDivertPacket<NetworkLayer>,
    exe: Option<&str>,
    outbound: bool,
    len: u64,
    stats: &mut EngineStats,
) {
    if let Err(e) = divert.send(packet) {
        stats.send_err += 1;
        if stats.send_err <= 5 || stats.send_err % 500 == 0 {
            tracing::warn!("engine: send failed (#{}) len={len} outbound={outbound}: {e}", stats.send_err);
        }
        return;
    }
    stats.sent += 1;
    let (down, up) = if outbound { (0, len) } else { (len, 0) };
    let mut flows = shared.flows.lock().unwrap();
    match exe {
        Some(exe) => flows.add_bytes(exe, down, up),
        // Unattributed traffic still counts, under the "Unknown" row, so the
        // totals and the graph reflect actual link usage.
        None => flows.add_unattributed(down, up),
    }
}

/// Drainer thread entry point: flush queued packets as tokens refill.
///
/// Re-injects through the same handle the engine captures on; see
/// [`EngineHandle`] for why a second handle is not an option.
pub fn run_drainer(shared: Arc<Shared>, handle: Arc<EngineHandle>) {
    let divert = &handle.0;
    tracing::info!("drainer thread started");

    while shared.running.load(Ordering::Relaxed) {
        let now = Instant::now();
        // Collected under the buckets lock, executed after releasing it so we
        // never hold two locks at once. The engine takes flows then buckets;
        // keeping the drainer to one lock at a time avoids any ordering hazard.
        let mut to_send: Vec<WinDivertPacket<'static, NetworkLayer>> = Vec::new();
        let mut to_count: Vec<(String, u64, u64)> = Vec::new();

        {
            let mut buckets = shared.buckets.lock().unwrap();
            for ((exe, dir), bucket) in buckets.iter_mut() {
                bucket.tb.refill(now);
                while let Some(front) = bucket.queue.front() {
                    let plen = front.data.len();
                    if bucket.tb.try_take(plen as u64) {
                        let pkt = bucket.queue.pop_front().unwrap();
                        bucket.queued_bytes = bucket.queued_bytes.saturating_sub(plen);
                        let (down, up) = match dir {
                            Dir::Down => (plen as u64, 0),
                            Dir::Up => (0, plen as u64),
                        };
                        to_count.push((exe.clone(), down, up));
                        to_send.push(pkt);
                    } else {
                        break;
                    }
                }
            }
        }

        for pkt in &to_send {
            if let Err(e) = divert.send(pkt) {
                tracing::warn!("drainer: send failed: {e}");
            }
        }
        if !to_count.is_empty() {
            let mut flows = shared.flows.lock().unwrap();
            for (exe, down, up) in to_count {
                flows.add_bytes(&exe, down, up);
            }
        }

        std::thread::sleep(DRAIN_INTERVAL);
    }
    tracing::info!("drainer thread exiting");
}

/// Zero-copy parse of an IPv4/IPv6 packet into `(protocol, src, dst)` socket
/// addresses. Ports are extracted for TCP/UDP; other protocols yield port 0
/// (which won't match a flow-table entry). Returns `None` for truncated or
/// non-IP data.
fn parse_packet(data: &[u8]) -> Option<(u8, SocketAddr, SocketAddr)> {
    let version = data.first()? >> 4;
    match version {
        4 => {
            if data.len() < 20 {
                return None;
            }
            let ihl = (data[0] & 0x0f) as usize * 4;
            if ihl < 20 || data.len() < ihl {
                return None;
            }
            let proto = data[9];
            let src_ip = Ipv4Addr::new(data[12], data[13], data[14], data[15]);
            let dst_ip = Ipv4Addr::new(data[16], data[17], data[18], data[19]);
            let (sp, dp) = l4_ports(proto, &data[ihl..]);
            Some((
                proto,
                SocketAddr::new(IpAddr::V4(src_ip), sp),
                SocketAddr::new(IpAddr::V4(dst_ip), dp),
            ))
        }
        6 => {
            if data.len() < 40 {
                return None;
            }
            // IPv6 extension headers are not walked; `next_header` is treated as
            // the L4 protocol. Non-TCP/UDP yields port 0.
            let proto = data[6];
            let src_ip = Ipv6Addr::from(<[u8; 16]>::try_from(&data[8..24]).ok()?);
            let dst_ip = Ipv6Addr::from(<[u8; 16]>::try_from(&data[24..40]).ok()?);
            let (sp, dp) = l4_ports(proto, &data[40..]);
            Some((
                proto,
                SocketAddr::new(IpAddr::V6(src_ip), sp),
                SocketAddr::new(IpAddr::V6(dst_ip), dp),
            ))
        }
        _ => None,
    }
}

/// Extract `(src_port, dst_port)` for TCP (6) / UDP (17); `(0, 0)` otherwise.
fn l4_ports(proto: u8, l4: &[u8]) -> (u16, u16) {
    if (proto == 6 || proto == 17) && l4.len() >= 4 {
        (
            u16::from_be_bytes([l4[0], l4[1]]),
            u16::from_be_bytes([l4[2], l4[3]]),
        )
    } else {
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_bucket_allows_burst_up_to_capacity() {
        let t0 = Instant::now();
        let mut b = TokenBucket::new_at(1000, t0);
        // Starts full: can spend the whole capacity at once.
        assert!(b.try_take(1000));
        // Now empty: even one byte fails.
        assert!(!b.try_take(1));
    }

    #[test]
    fn refill_is_proportional_to_elapsed_time() {
        let t0 = Instant::now();
        let mut b = TokenBucket::new_at(1000, t0);
        assert!(b.try_take(1000)); // drain
        // After 0.5s, ~500 tokens are back.
        b.refill(t0 + Duration::from_millis(500));
        assert!(b.try_take(500));
        assert!(!b.try_take(1));
    }

    #[test]
    fn refill_never_exceeds_capacity() {
        let t0 = Instant::now();
        let mut b = TokenBucket::new_at(1000, t0);
        assert!(b.try_take(1000));
        // Idle for 10s, but capacity caps the burst at 1000.
        b.refill(t0 + Duration::from_secs(10));
        assert!(b.try_take(1000));
        assert!(!b.try_take(1));
    }

    #[test]
    fn oversized_packet_passes_only_when_full() {
        let t0 = Instant::now();
        let mut b = TokenBucket::new_at(500, t0); // capacity 500
        // A 1500-byte packet is larger than capacity: passes while full.
        assert!(b.try_take(1500));
        // Bucket now drained; the next oversized packet must wait.
        assert!(!b.try_take(1500));
        // After a full second it's full again and passes.
        b.refill(t0 + Duration::from_secs(1));
        assert!(b.try_take(1500));
    }

    #[test]
    fn set_rate_clamps_tokens_to_new_capacity() {
        let t0 = Instant::now();
        let mut b = TokenBucket::new_at(1000, t0); // full: 1000 tokens
        b.set_rate(400); // capacity shrinks; tokens clamp to 400
        assert!(b.try_take(400));
        assert!(!b.try_take(1));
    }

    #[test]
    fn parses_ipv4_tcp_5tuple() {
        // Minimal IPv4 header (IHL=5 => 20 bytes), protocol=TCP(6), then ports.
        let mut pkt = vec![0u8; 24];
        pkt[0] = 0x45; // version 4, IHL 5
        pkt[9] = 6; // TCP
        pkt[12..16].copy_from_slice(&[10, 0, 0, 1]); // src
        pkt[16..20].copy_from_slice(&[93, 184, 216, 34]); // dst
        pkt[20..22].copy_from_slice(&50000u16.to_be_bytes()); // src port
        pkt[22..24].copy_from_slice(&443u16.to_be_bytes()); // dst port

        let (proto, src, dst) = parse_packet(&pkt).unwrap();
        assert_eq!(proto, 6);
        assert_eq!(src, "10.0.0.1:50000".parse().unwrap());
        assert_eq!(dst, "93.184.216.34:443".parse().unwrap());
    }

    #[test]
    fn rejects_truncated_and_non_ip() {
        assert!(parse_packet(&[]).is_none());
        assert!(parse_packet(&[0x45, 0x00]).is_none()); // too short for v4
        assert!(parse_packet(&[0x00; 20]).is_none()); // version 0
    }
}
