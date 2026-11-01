//! Single-instance guard.
//!
//! Two Throttle processes would each open a WinDivert handle at the same
//! priority, so every packet would be captured and re-injected twice: the
//! counters split the traffic between them and a limit set in one window can be
//! bypassed by the other. A machine-wide named mutex guarantees one shaper.
//! Because Throttle always runs elevated, the mutex lives in the `Global\`
//! namespace so instances in different sessions still see each other.
//!
//! A second launch asks the running instance to show itself through a named
//! event rather than by calling `ShowWindow` on its window: winit caches the
//! window's visibility, so showing it from outside would leave winit thinking
//! it is still hidden, and the next hide to the tray would do nothing.

use std::sync::OnceLock;

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent,
    WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow,
    ShowWindow,
};
use windows::core::{HSTRING, PCWSTR, w};

/// Title of the main window, used to locate an already-running instance.
const WINDOW_TITLE: &str = "Throttle";

/// Auto-reset event a second launch signals to bring the running window back.
const SHOW_EVENT: PCWSTR = w!("Global\\ThrottleShowRequest");

/// The show event, created with the lock so a second launch during startup
/// (driver load, window creation) is not lost: the auto-reset event stays
/// signalled until `on_show_request` starts waiting on it.
static SHOW_EVENT_HANDLE: OnceLock<usize> = OnceLock::new();

/// Held for the life of the process. Dropping it would release the mutex, so
/// the handle is intentionally never closed.
pub struct InstanceLock {
    #[allow(dead_code)]
    handle: HANDLE,
}

/// Try to become the single running instance.
///
/// Returns `Some(lock)` if this process now owns the mutex. Returns `None` if
/// another instance already holds it; in that case the running instance has
/// been asked to show its window (even from the tray) so the user's
/// double-click still does something visible.
pub fn acquire() -> Option<InstanceLock> {
    // SAFETY: plain Win32 calls with valid, null-terminated static strings.
    unsafe {
        let handle = CreateMutexW(None, false, w!("Global\\ThrottleSingleInstance")).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if !request_show() {
                // A build without the show event: fall back to raising its window.
                focus_window(WINDOW_TITLE);
            }
            return None;
        }
        // The handle is valid for the process lifetime and intentionally never closed.
        match CreateEventW(None, false, false, SHOW_EVENT) {
            Ok(event) => {
                let _ = SHOW_EVENT_HANDLE.set(event.0 as usize);
            }
            Err(e) => tracing::warn!("show-request event unavailable: {e}"),
        }
        Some(InstanceLock { handle })
    }
}

/// Run `on_request` every time a second launch asks this instance to show
/// itself, including a request made before this was called. The callback runs
/// on a dedicated background thread.
pub fn on_show_request(on_request: impl Fn() + Send + 'static) {
    let Some(&event) = SHOW_EVENT_HANDLE.get() else {
        return;
    };
    std::thread::Builder::new()
        .name("show-request".into())
        .spawn(move || {
            let event = HANDLE(event as *mut _);
            // SAFETY: waiting on a valid event handle owned by this process.
            while unsafe { WaitForSingleObject(event, INFINITE) } == WAIT_OBJECT_0 {
                on_request();
            }
        })
        .expect("spawn show-request thread");
}

/// Ask the running instance to show its window. Returns false if it has no
/// show event to signal.
fn request_show() -> bool {
    // SAFETY: plain Win32 calls with a valid, null-terminated static string.
    unsafe {
        let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, SHOW_EVENT) else {
            return false;
        };
        // We are the foreground process (the user just launched us), so we may
        // let the running instance take focus when it shows its window.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        SetEvent(event).is_ok()
    }
}

/// Best-effort: restore and raise the window titled `title`. Not for a window
/// that may be hidden to the tray, since showing it from outside winit would
/// desync winit's cached visibility.
pub fn focus_window(title: &str) {
    // SAFETY: FindWindowW returns a null HWND when nothing matches; all calls
    // tolerate that and return failure rather than faulting.
    unsafe {
        let hwnd: HWND = match FindWindowW(None, &HSTRING::from(title)) {
            Ok(h) if !h.is_invalid() => h,
            _ => return,
        };
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}
