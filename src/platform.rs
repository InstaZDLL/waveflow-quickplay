//! The few native calls egui / winit do not wrap.

/// Makes the window titled `owned_title` an *owned* window of the one
/// titled `owner_title` (both created by this thread). Windows then treats
/// the pair as PotPlayer's player + playlist: one taskbar button, the owned
/// window always above its owner, hidden and shown with it when the owner
/// is minimized and restored. egui-winit creates every viewport as an
/// independent top-level window, hence the lookup by title after the fact.
///
/// Returns `true` once the relationship is in place.
#[cfg(windows)]
pub fn attach_owned_window(owner_title: &str, owned_title: &str) -> bool {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetWindowLongPtrW, GetWindowTextW, SetWindowLongPtrW, SetWindowPos,
        GWLP_HWNDPARENT, GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, WS_EX_APPWINDOW,
    };

    struct Search {
        owner: Vec<u16>,
        owned: Vec<u16>,
        owner_hwnd: HWND,
        owned_hwnd: HWND,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: `lparam` is the `Search` the caller passed, which outlives
        // the synchronous `EnumThreadWindows` call.
        let search = unsafe { &mut *(lparam as *mut Search) };
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        let title = &buf[..n.max(0) as usize];
        if title == search.owner.as_slice() {
            search.owner_hwnd = hwnd;
        } else if title == search.owned.as_slice() {
            search.owned_hwnd = hwnd;
        }
        1
    }

    let mut search = Search {
        owner: owner_title.encode_utf16().collect(),
        owned: owned_title.encode_utf16().collect(),
        owner_hwnd: std::ptr::null_mut(),
        owned_hwnd: std::ptr::null_mut(),
    };
    // SAFETY: plain Win32 calls on this thread's own windows; the callback
    // only reads titles and writes into `search`.
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(visit),
            &mut search as *mut Search as LPARAM,
        );
        if search.owner_hwnd.is_null() || search.owned_hwnd.is_null() {
            return false;
        }
        SetWindowLongPtrW(
            search.owned_hwnd,
            GWLP_HWNDPARENT,
            search.owner_hwnd as isize,
        );
        // winit gives every window WS_EX_APPWINDOW, which forces a taskbar
        // button even on an owned window; drop it so the pair has one.
        let ex = GetWindowLongPtrW(search.owned_hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            search.owned_hwnd,
            GWL_EXSTYLE,
            ex & !(WS_EX_APPWINDOW as isize),
        );
        SetWindowPos(
            search.owned_hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
    tracing::debug!("playlist window attached to the player");
    true
}

/// Other platforms: nothing to do (X11 has no owner concept egui exposes;
/// the playlist still skips the taskbar through the viewport builder).
#[cfg(not(windows))]
pub fn attach_owned_window(_owner_title: &str, _owned_title: &str) -> bool {
    true
}
