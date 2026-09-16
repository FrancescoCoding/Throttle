//! Single-instance guard.
//!
//! Two Throttle processes would each open a WinDivert handle at the same
//! priority, so every packet would be captured and re-injected twice: the
//! counters split the traffic between them and a limit set in one window can be
//! bypassed by the other. A machine-wide named mutex guarantees one shaper.
//! Because Throttle always runs elevated, the mutex lives in the `Global\`
//! namespace so instances in different sessions still see each other.

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
};
use windows::core::{PCWSTR, w};

/// Title of the main window, used to locate an already-running instance.
const WINDOW_TITLE: PCWSTR = w!("Throttle");

/// Held for the life of the process. Dropping it would release the mutex, so
/// the handle is intentionally never closed.
pub struct InstanceLock {
    #[allow(dead_code)]
    handle: HANDLE,
}

/// Try to become the single running instance.
///
/// Returns `Some(lock)` if this process now owns the mutex. Returns `None` if
/// another instance already holds it; in that case the existing main window,
/// if it can be found, has been restored and brought to the foreground so the
/// user's double-click still does something visible.
pub fn acquire() -> Option<InstanceLock> {
    // SAFETY: plain Win32 calls with valid, null-terminated static strings.
    unsafe {
        let handle = CreateMutexW(None, false, w!("Global\\ThrottleSingleInstance")).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            focus_existing_window();
            return None;
        }
        Some(InstanceLock { handle })
    }
}

/// Best-effort: restore and raise the existing instance's main window.
fn focus_existing_window() {
    // SAFETY: FindWindowW returns a null HWND when nothing matches; all calls
    // tolerate that and return failure rather than faulting.
    unsafe {
        let hwnd: HWND = match FindWindowW(None, WINDOW_TITLE) {
            Ok(h) if !h.is_invalid() => h,
            _ => return,
        };
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}
