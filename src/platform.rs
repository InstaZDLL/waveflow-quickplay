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

/// Release builds use the GUI subsystem on Windows (no console window when
/// launched from Explorer), which also means `--help` or `--register` run
/// from a terminal print nothing. Attaching to the parent's console, when
/// there is one, gets that output back. No-op elsewhere.
#[cfg(windows)]
pub fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{
        AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
    };
    // SAFETY: plain Win32 calls; a failed attach (no parent console) is
    // harmless. Attaching replaces the standard handles, so it is only done
    // when none was inherited: a redirect or a pipe (`> file`, `| less`)
    // must keep going where the caller sent it.
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out.is_null() || out == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
pub fn attach_parent_console() {}

/// Whether process `pid` runs as the same user as this one: the user SIDs
/// of the two process tokens are equal. `false` when the other token
/// cannot be read, as for another user's process, which is the case this
/// tells apart.
#[cfg(windows)]
pub fn same_user_process(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// The `TOKEN_USER` of `process`, in a buffer aligned for it.
    ///
    /// SAFETY: `process` is a valid process handle.
    unsafe fn token_user(process: HANDLE) -> Option<Vec<u64>> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: documented calls; `token` is closed below, the buffer is
        // sized from the length the first call reports.
        unsafe {
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            let ok = len > 0
                && GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len)
                    != 0;
            CloseHandle(token);
            ok.then_some(buf)
        }
    }

    // SAFETY: the pseudo-handle of this process is always valid; the other
    // handle is checked and closed; both buffers start with a TOKEN_USER
    // written by GetTokenInformation, whose SIDs point into the buffers.
    unsafe {
        let Some(mine_buf) = token_user(GetCurrentProcess()) else {
            return false;
        };
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let theirs = token_user(process);
        CloseHandle(process);
        let Some(theirs_buf) = theirs else {
            return false;
        };
        let mine = &*mine_buf.as_ptr().cast::<TOKEN_USER>();
        let theirs = &*theirs_buf.as_ptr().cast::<TOKEN_USER>();
        EqualSid(mine.User.Sid, theirs.User.Sid) != 0
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn this_process_is_ours_the_system_one_is_not() {
        assert!(super::same_user_process(std::process::id()));
        // PID 4 is the System process, which no user runs.
        assert!(!super::same_user_process(4));
    }
}

/// Bytes waiting to be read in the pipe `handle`, without reading them.
#[cfg(windows)]
pub fn pipe_bytes_available(handle: std::os::windows::io::RawHandle) -> std::io::Result<u32> {
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;
    let mut available = 0u32;
    // SAFETY: documented call on a pipe handle the caller holds open; no
    // buffer is read into, only the count is written.
    let ok = unsafe {
        PeekNamedPipe(
            handle,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(available)
}
