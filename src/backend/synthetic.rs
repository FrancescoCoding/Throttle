//! Policy for the synthetic process rows.
//!
//! Two executable keys do not correspond to a real image path: traffic that
//! could not be attributed at all, and kernel-owned flows (PID 4). This module
//! owns those keys, how they are displayed, and the rule that they are never
//! shaped, so the engine, the flow table, and the GUI agree on one definition.

/// Pseudo executable path for traffic that could not be attributed to any
/// process. Surfaced to the GUI as the "Unknown" row.
pub const UNKNOWN_EXE: &str = "<unknown>";
/// Pseudo executable path used for kernel-owned flows (PID 4).
pub const SYSTEM_EXE: &str = "system";

/// Whether shaping rules may be applied to this executable key.
///
/// Only the "Unknown" row is exempt: it is not a process but the shifting mix
/// of traffic we failed to identify, so a limit on it would be meaningless.
/// The kernel "System" row is a real, stable source (SMB, VPN tunnels, Windows
/// Update delivery) and may be shaped like any process; the GUI warns about
/// what it covers.
pub fn is_shapable(exe: &str) -> bool {
    exe != UNKNOWN_EXE
}

/// Whether this key is the kernel "System" row, which the GUI annotates with a
/// warning before offering shaping actions.
pub fn is_system(exe: &str) -> bool {
    exe == SYSTEM_EXE
}

/// Display name for an executable key, special-casing the synthetic rows.
pub fn display_name(exe: &str) -> String {
    match exe {
        UNKNOWN_EXE => "Unknown".to_string(),
        SYSTEM_EXE => "System".to_string(),
        other => file_name(other),
    }
}

/// Extract the filename portion of a Windows or Unix-style path.
fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_unknown_row_is_unshapable() {
        assert!(!is_shapable(UNKNOWN_EXE));
        assert!(is_shapable(SYSTEM_EXE));
        assert!(is_system(SYSTEM_EXE));
        assert!(!is_system(UNKNOWN_EXE));
        assert!(is_shapable("c:\\app\\thing.exe"));
    }

    #[test]
    fn file_name_handles_windows_paths() {
        assert_eq!(file_name("c:\\a\\b\\thing.exe"), "thing.exe");
        assert_eq!(file_name("/usr/bin/curl"), "curl");
        assert_eq!(file_name("bare.exe"), "bare.exe");
        assert_eq!(display_name(UNKNOWN_EXE), "Unknown");
        assert_eq!(display_name(SYSTEM_EXE), "System");
    }
}
