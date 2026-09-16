//! Win32 socket-table queries and PID-to-path resolution.
//!
//! This is the platform layer beneath the flow table: it reads the kernel's
//! TCP and UDP owner tables (`GetExtendedTcpTable` / `GetExtendedUdpTable`,
//! IPv4 and IPv6) and resolves PIDs to image paths. Everything here is a free
//! function returning plain values, on purpose: the queries perform several
//! Win32 round trips plus an `OpenProcess`/`QueryFullProcessImageNameW` pair per
//! new PID, which must never happen while the flow-table mutex (locked per
//! packet by the engine) is held. Callers gather a [`SocketSnapshot`] with no
//! lock held and then apply it under the lock.

use std::collections::HashSet;
use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
    MIB_UDP6TABLE_OWNER_PID, MIB_UDPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

use super::synthetic::SYSTEM_EXE;

/// IANA protocol number for TCP.
pub const IPPROTO_TCP: u8 = 6;
/// IANA protocol number for UDP.
pub const IPPROTO_UDP: u8 = 17;
/// `AF_INET` (IPv4) address family for the socket-table queries.
const AF_INET: u32 = 2;
/// `AF_INET6` address family for the socket-table queries.
const AF_INET6: u32 = 23;
/// `NO_ERROR` return value from Win32 table APIs.
const NO_ERROR: u32 = 0;

/// One row extracted from a socket table, already converted to host order.
/// `remote` is `None` for UDP rows (which carry no peer address).
struct SocketRow {
    local: SocketAddr,
    remote: Option<SocketAddr>,
    owning_pid: u32,
}

/// A row of the combined TCP+UDP socket tables, tagged with its protocol.
pub struct SocketTableRow {
    pub protocol: u8,
    pub local: SocketAddr,
    pub remote: Option<SocketAddr>,
    pub pid: u32,
}

/// The result of one socket-table sweep, gathered with no locks held.
pub struct SocketSnapshot {
    pub rows: Vec<SocketTableRow>,
    /// PIDs resolved during the sweep (outside the flow-table lock).
    pub resolved: Vec<(u32, Option<String>)>,
    /// True only if every one of the four table queries succeeded; a partial
    /// sweep must not be treated as authoritative.
    pub complete: bool,
}

/// Read the TCP and UDP owner tables (IPv4 and IPv6) and resolve every PID not
/// already in `known_pids`. Feed the result to
/// `FlowTable::apply_socket_snapshot`.
pub fn query_socket_tables(known_pids: &HashSet<u32>) -> SocketSnapshot {
    let mut rows: Vec<SocketTableRow> = Vec::new();
    let mut complete = true;

    {
        let mut take = |proto: u8, res: anyhow::Result<Vec<SocketRow>>, what: &str| match res {
            Ok(found) => rows.extend(found.into_iter().map(|r| SocketTableRow {
                protocol: proto,
                local: r.local,
                remote: r.remote,
                pid: r.owning_pid,
            })),
            Err(e) => {
                tracing::warn!("{what} table query failed: {e}");
                complete = false;
            }
        };
        take(IPPROTO_TCP, query_tcp_table(AF_INET), "IPv4 TCP");
        take(IPPROTO_TCP, query_tcp_table(AF_INET6), "IPv6 TCP");
        take(IPPROTO_UDP, query_udp_table(AF_INET), "IPv4 UDP");
        take(IPPROTO_UDP, query_udp_table(AF_INET6), "IPv6 UDP");
    }

    let mut seen: HashSet<u32> = HashSet::new();
    let mut resolved = Vec::new();
    for row in &rows {
        if known_pids.contains(&row.pid) || !seen.insert(row.pid) {
            continue;
        }
        resolved.push((row.pid, resolve_pid_path(row.pid)));
    }

    SocketSnapshot {
        rows,
        resolved,
        complete,
    }
}

/// PID-to-path policy shared by the cached and the lock-free resolution paths:
/// PID 0 is ignored, PID 4 is the kernel, and anything unopenable gets a stable
/// synthetic key so its traffic is still attributed somewhere.
pub fn resolve_pid_path(pid: u32) -> Option<String> {
    if pid == 0 {
        None
    } else if pid == 4 {
        Some(SYSTEM_EXE.to_string())
    } else {
        Some(query_image_path(pid).unwrap_or_else(|| format!("pid:{pid}")))
    }
}

/// Resolve a PID to its lowercase full image path via the Win32 API.
fn query_image_path(pid: u32) -> Option<String> {
    // PID 0 (System Idle) / 4 (System) can't be opened for image name; callers
    // map those to synthetic names before reaching here.
    if pid == 0 || pid == 4 {
        return None;
    }
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

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

/// Convert a `MIB_TCP*` port dword (network byte order in the low 16 bits) to a
/// host-order `u16`.
fn port_from_dword(dw: u32) -> u16 {
    let b0 = (dw & 0xff) as u8; // first network byte  = high byte of the port
    let b1 = ((dw >> 8) & 0xff) as u8; // second network byte = low byte of the port
    u16::from_be_bytes([b0, b1])
}

/// Query the TCP connection table (owner-PID variant) for `family`
/// (`AF_INET` or `AF_INET6`).
fn query_tcp_table(family: u32) -> anyhow::Result<Vec<SocketRow>> {
    unsafe {
        let mut size: u32 = 0;
        GetExtendedTcpTable(None, &mut size, false, family, TCP_TABLE_OWNER_PID_ALL, 0);
        if size == 0 {
            return Ok(Vec::new());
        }

        let mut buf = vec![0u8; size as usize];
        let ret = GetExtendedTcpTable(
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut size,
            false,
            family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
        if ret != NO_ERROR {
            anyhow::bail!("GetExtendedTcpTable(af={family}) returned error code {ret}");
        }

        if family == AF_INET {
            let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            let count = table.dwNumEntries as usize;
            // `table.table` is a flexible array member declared with length 1.
            let rows = std::slice::from_raw_parts(table.table.as_ptr(), count);
            Ok(rows
                .iter()
                .map(|r| SocketRow {
                    local: SocketAddr::new(
                        IpAddr::V4(Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes())),
                        port_from_dword(r.dwLocalPort),
                    ),
                    remote: Some(SocketAddr::new(
                        IpAddr::V4(Ipv4Addr::from(r.dwRemoteAddr.to_ne_bytes())),
                        port_from_dword(r.dwRemotePort),
                    )),
                    owning_pid: r.dwOwningPid,
                })
                .collect())
        } else {
            let table = &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
            let count = table.dwNumEntries as usize;
            let rows = std::slice::from_raw_parts(table.table.as_ptr(), count);
            Ok(rows
                .iter()
                .map(|r| SocketRow {
                    local: SocketAddr::new(
                        IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)),
                        port_from_dword(r.dwLocalPort),
                    ),
                    remote: Some(SocketAddr::new(
                        IpAddr::V6(Ipv6Addr::from(r.ucRemoteAddr)),
                        port_from_dword(r.dwRemotePort),
                    )),
                    owning_pid: r.dwOwningPid,
                })
                .collect())
        }
    }
}

/// Query the UDP endpoint table (owner-PID variant) for `family`.
fn query_udp_table(family: u32) -> anyhow::Result<Vec<SocketRow>> {
    unsafe {
        let mut size: u32 = 0;
        GetExtendedUdpTable(None, &mut size, false, family, UDP_TABLE_OWNER_PID, 0);
        if size == 0 {
            return Ok(Vec::new());
        }

        let mut buf = vec![0u8; size as usize];
        let ret = GetExtendedUdpTable(
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut size,
            false,
            family,
            UDP_TABLE_OWNER_PID,
            0,
        );
        if ret != NO_ERROR {
            anyhow::bail!("GetExtendedUdpTable(af={family}) returned error code {ret}");
        }

        if family == AF_INET {
            let table = &*(buf.as_ptr() as *const MIB_UDPTABLE_OWNER_PID);
            let count = table.dwNumEntries as usize;
            let rows = std::slice::from_raw_parts(table.table.as_ptr(), count);
            Ok(rows
                .iter()
                .map(|r| SocketRow {
                    local: SocketAddr::new(
                        IpAddr::V4(Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes())),
                        port_from_dword(r.dwLocalPort),
                    ),
                    remote: None,
                    owning_pid: r.dwOwningPid,
                })
                .collect())
        } else {
            let table = &*(buf.as_ptr() as *const MIB_UDP6TABLE_OWNER_PID);
            let count = table.dwNumEntries as usize;
            let rows = std::slice::from_raw_parts(table.table.as_ptr(), count);
            Ok(rows
                .iter()
                .map(|r| SocketRow {
                    local: SocketAddr::new(
                        IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)),
                        port_from_dword(r.dwLocalPort),
                    ),
                    remote: None,
                    owning_pid: r.dwOwningPid,
                })
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_dword_conversion_is_network_order() {
        // Port 443 in network byte order is bytes [0x01, 0xBB]; stored in the
        // low word of a dword on a little-endian machine that is 0x0000BB01.
        let dw = 0x0000_BB01u32;
        assert_eq!(port_from_dword(dw), 443);
    }

    #[test]
    fn system_and_unresolvable_pids_get_synthetic_paths() {
        assert_eq!(resolve_pid_path(4).as_deref(), Some(SYSTEM_EXE));
        assert_eq!(resolve_pid_path(0), None);
        // A PID that certainly cannot be opened still yields a stable key.
        let ghost = resolve_pid_path(0x7fff_fff0).unwrap();
        assert!(ghost.starts_with("pid:"), "got {ghost}");
    }
}
