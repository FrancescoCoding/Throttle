//! Turns a failed WinDivert open into a report the user can act on.
//!
//! WinDivert reports Windows error 5 (access denied) both when the process is
//! not elevated and when something stops an elevated process from installing
//! or opening the driver, which is what endpoint security and driver policies
//! on managed computers do. The `windivert` crate labels that code "Running
//! without elevated access rights" either way, so the elevation is checked
//! here instead of being assumed.
//!
//! Error 2 (file not found) is just as loose. WinDivert.dll never looks for the
//! driver file itself: the code is usually whatever `StartService` returned. If a
//! `WinDivert` service is already registered, the DLL starts that one instead
//! of registering the file next to it, so a registration left behind by
//! another program (or by a copy of Throttle that has since been moved or
//! deleted) fails with "file not found" while the file sits right there.

use std::path::{Path, PathBuf};

use windivert::error::{WinDivertError, WinDivertOpenError};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::w;

const DRIVER_FILE: &str = "WinDivert64.sys";
const DLL_FILE: &str = "WinDivert.dll";
/// Windows error 1275: the driver has been blocked from loading.
const ERROR_DRIVER_BLOCKED: i32 = 1275;

/// Build the full report for a failed open: what went wrong, what to do about
/// it, and the facts someone helping (an IT team, an issue report) will ask for.
pub fn open_failure(err: &WinDivertError) -> String {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let present = |file: &str| dir.as_ref().map(|d| d.join(file).exists());
    let own = dir.as_ref().map(|d| d.join(DRIVER_FILE));
    let registered = registered_driver().map(|path| Registered {
        exists: path.exists(),
        elsewhere: own.as_ref().is_some_and(|own| !same_path(own, &path)),
        path,
    });
    report(
        err,
        is_elevated(),
        present(DRIVER_FILE),
        present(DLL_FILE),
        dir.as_deref(),
        registered.as_ref(),
    )
}

/// The driver file an existing `WinDivert` service registration points at.
struct Registered {
    path: PathBuf,
    exists: bool,
    /// Points somewhere other than the driver file next to this exe. A failed
    /// start leaves a registration for our own folder behind (WinDivert only
    /// removes it after a successful start), and that one is not stale: it
    /// works again as soon as the file is back.
    elsewhere: bool,
}

/// Windows paths compare case insensitively.
fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

fn report(
    err: &WinDivertError,
    elevated: Option<bool>,
    driver_present: Option<bool>,
    dll_present: Option<bool>,
    dir: Option<&Path>,
    registered: Option<&Registered>,
) -> String {
    let folder = dir
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let registered_line = match registered {
        Some(r) => format!("{} ({})", r.path.display(), found(Some(r.exists))),
        None => "none".to_string(),
    };
    format!(
        "{}\n\n\
         WinDivert error: {err:?}\n\
         Running as Administrator: {}\n\
         {DRIVER_FILE}: {}\n\
         {DLL_FILE}: {}\n\
         Folder: {folder}\n\
         Registered driver: {registered_line}",
        explain(err, elevated, driver_present, registered),
        yes_no(elevated),
        found(driver_present),
        found(dll_present),
    )
}

/// The human explanation for an open error, given whether the process is
/// really elevated, whether the driver file is next to the exe, and what an
/// existing driver registration points at.
fn explain(
    err: &WinDivertError,
    elevated: Option<bool>,
    driver_present: Option<bool>,
    registered: Option<&Registered>,
) -> String {
    let open = match err {
        WinDivertError::Open(open) => open,
        // The crate maps "driver blocked" to the wrong code (1257), so the
        // real one arrives as a plain OS error.
        WinDivertError::IOError(e) if e.raw_os_error() == Some(ERROR_DRIVER_BLOCKED) => {
            return format!(
                "Windows blocked {DRIVER_FILE} from loading. Security software or a \
                 driver policy on this computer does not allow it, or this is a \
                 virtual machine that cannot load drivers. Check your antivirus \
                 history for WinDivert. On a work or managed computer, ask your IT \
                 team to allow the driver."
            );
        }
        _ => return format!("Could not open the WinDivert driver: {err}."),
    };
    match open {
        WinDivertOpenError::AccessDenied if elevated == Some(true) => format!(
            "Windows denied access to the WinDivert driver even though \
             Throttle is running as Administrator. On a work or managed computer this \
             usually means security software or a driver policy is blocking \
             {DRIVER_FILE}. Check your antivirus history for a blocked or quarantined \
             file, and ask your IT team to allow the driver."
        ),
        WinDivertOpenError::AccessDenied => "Throttle is not running as Administrator. \
             Right click throttle.exe and choose \"Run as administrator\"."
            .to_string(),
        WinDivertOpenError::MissingSYS if registered.is_some_and(|r| r.elsewhere && !r.exists) => {
            format!(
                "Windows already has a WinDivert driver registered, and it points at a \
             file that no longer exists: {}. An earlier start of Throttle from another \
             folder (for example straight from inside the zip), or another program \
             that uses WinDivert, left it behind. Open a Command Prompt as \
             administrator, run \"sc delete WinDivert\", then start Throttle again.",
                registered
                    .map(|r| r.path.display().to_string())
                    .unwrap_or_default()
            )
        }
        // Seen with Bitdefender: the service is registered, its registry key
        // is gone a moment later, and StartService fails with "file not
        // found". Deleting the registration does not help, it is recreated
        // and removed again on the next start.
        WinDivertOpenError::MissingSYS if driver_present == Some(true) => format!(
            "{DRIVER_FILE} is next to throttle.exe, but Windows could not start the \
             driver. The usual cause is security software blocking the driver or \
             removing its registration. Bitdefender is known to do this, and its \
             folder exceptions do not prevent it. Check your antivirus history for \
             WinDivert. Also make sure the Throttle folder is on a local disk (not a \
             network drive or a cloud synced folder), then restart Windows and try \
             again."
        ),
        WinDivertOpenError::MissingSYS => format!(
            "{DRIVER_FILE} was not found next to throttle.exe. Extract the whole zip \
             into one folder. If the file disappears again, your antivirus is removing it."
        ),
        WinDivertOpenError::InvalidImageHash => format!(
            "Windows rejected the digital signature of {DRIVER_FILE}. Download Throttle \
             again in case the file is damaged. A driver signing policy on this \
             computer can also cause this."
        ),
        WinDivertOpenError::IncompatibleVersion => "Another program has already loaded a \
             different version of the WinDivert driver. Close that program or restart \
             Windows, then try again."
            .to_string(),
        WinDivertOpenError::BaseFilteringEngineDisabled => "The Windows service \"Base \
             Filtering Engine\" is disabled. Throttle needs it: enable and start it in \
             services.msc, then try again."
            .to_string(),
        other => format!("Could not open the WinDivert driver: {other}."),
    }
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn found(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "found",
        Some(false) => "missing",
        None => "unknown",
    }
}

/// The image path of an already registered `WinDivert` service, if there is
/// one. WinDivert.dll starts an existing registration as it is, wherever it
/// points.
fn registered_driver() -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let mut bytes = size_of_val(&buf) as u32;
    // SAFETY: the buffer outlives the call and `bytes` holds its size in bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SYSTEM\CurrentControlSet\Services\WinDivert"),
            w!("ImagePath"),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    status.ok().ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    image_path(&String::from_utf16_lossy(&buf[..len]))
}

/// Turn a service image path (`\??\C:\dir\WinDivert64.sys`) into a file path.
fn image_path(raw: &str) -> Option<PathBuf> {
    let path = raw.trim().trim_matches('"');
    let path = path.strip_prefix(r"\??\").unwrap_or(path);
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Whether this process holds an elevated token. `None` if Windows would not
/// say.
fn is_elevated() -> Option<bool> {
    // SAFETY: the token handle is only used while open and is closed before
    // returning; the buffer passed to GetTokenInformation matches its length.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        result.ok()?;
        Some(elevation.TokenIsElevated != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied() -> WinDivertError {
        WinDivertError::Open(WinDivertOpenError::AccessDenied)
    }

    #[test]
    fn access_denied_while_elevated_blames_a_block_not_the_user() {
        let text = explain(&denied(), Some(true), Some(true), None);
        assert!(text.contains("even though"));
        assert!(text.contains("IT team"));
        assert!(!text.contains("is not running as Administrator"));
    }

    #[test]
    fn access_denied_without_elevation_asks_for_administrator() {
        for elevated in [Some(false), None] {
            let text = explain(&denied(), elevated, Some(true), None);
            assert!(text.contains("is not running as Administrator"));
        }
    }

    #[test]
    fn report_lists_the_facts() {
        let text = report(
            &denied(),
            Some(true),
            Some(false),
            Some(true),
            Some(Path::new(r"C:\Apps\Throttle")),
            None,
        );
        assert!(text.contains("WinDivert error: Open(AccessDenied)"));
        assert!(text.contains("Running as Administrator: yes"));
        assert!(text.contains("WinDivert64.sys: missing"));
        assert!(text.contains("WinDivert.dll: found"));
        assert!(text.contains(r"Folder: C:\Apps\Throttle"));
        assert!(text.contains("Registered driver: none"));
    }

    fn missing_sys() -> WinDivertError {
        WinDivertError::Open(WinDivertOpenError::MissingSYS)
    }

    #[test]
    fn missing_driver_file_asks_to_extract_the_zip() {
        let text = explain(&missing_sys(), Some(true), Some(false), None);
        assert!(text.contains("was not found next to throttle.exe"));
    }

    #[test]
    fn file_not_found_with_the_file_present_does_not_claim_it_is_missing() {
        let text = explain(&missing_sys(), Some(true), Some(true), None);
        assert!(text.contains("is next to throttle.exe"));
        assert!(!text.contains("was not found"));
        // Deleting the registration does not fix a blocked driver.
        assert!(text.contains("security software"));
        assert!(!text.contains("sc delete"));
    }

    #[test]
    fn driver_blocked_code_is_explained() {
        let err = WinDivertError::IOError(std::io::Error::from_raw_os_error(1275));
        let text = explain(&err, Some(true), Some(true), None);
        assert!(text.contains("blocked WinDivert64.sys from loading"));
    }

    #[test]
    fn stale_registration_is_named_with_its_fix() {
        let stale = Registered {
            path: PathBuf::from(r"D:\Old\WinDivert64.sys"),
            exists: false,
            elsewhere: true,
        };
        let text = explain(&missing_sys(), Some(true), Some(true), Some(&stale));
        assert!(text.contains(r"D:\Old\WinDivert64.sys"));
        assert!(text.contains("sc delete WinDivert"));
        let text = report(
            &missing_sys(),
            Some(true),
            Some(true),
            Some(true),
            None,
            Some(&stale),
        );
        assert!(text.contains(r"Registered driver: D:\Old\WinDivert64.sys (missing)"));
    }

    #[test]
    fn registration_left_by_our_own_failed_start_is_not_stale() {
        // A first start without the driver file leaves a registration for
        // this folder behind; the advice is still to restore the file.
        let own = Registered {
            path: PathBuf::from(r"C:\Apps\Throttle\WinDivert64.sys"),
            exists: false,
            elsewhere: false,
        };
        let text = explain(&missing_sys(), Some(true), Some(false), Some(&own));
        assert!(text.contains("was not found next to throttle.exe"));
        assert!(!text.contains("sc delete"));
    }

    #[test]
    fn same_path_ignores_case() {
        assert!(same_path(
            Path::new(r"C:\Apps\Throttle\WinDivert64.sys"),
            Path::new(r"c:\apps\throttle\windivert64.sys"),
        ));
        assert!(!same_path(
            Path::new(r"C:\A\x.sys"),
            Path::new(r"C:\B\x.sys")
        ));
    }

    #[test]
    fn image_path_strips_the_nt_prefix() {
        assert_eq!(
            image_path(r"\??\C:\Apps\Throttle\WinDivert64.sys"),
            Some(PathBuf::from(r"C:\Apps\Throttle\WinDivert64.sys"))
        );
        assert_eq!(image_path(""), None);
    }
}
