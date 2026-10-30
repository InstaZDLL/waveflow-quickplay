//! Explorer's "Play with" / "Add to playlist" verbs, as `DropTarget` verbs
//! served by QuickPlay itself (Windows).
//!
//! A command-line verb costs one process per selected file: Explorer runs
//! `waveflow-quickplay.exe "%1"` for each of them, a thousand processes
//! for a thousand files. A `DropTarget` verb names a COM class instead
//! (src/integration.rs registers both, `LocalServer32` under
//! `HKCU\Software\Classes\CLSID`): Explorer has COM start one QuickPlay
//! with `-Embedding`, creates the class's object in it and drops the whole
//! selection on it as one data object, as if the files had been dragged
//! onto the player. The class says which verb it was. [`receive`] serves
//! the two classes until that drop and hands the selection back to
//! `main`, which goes on as for a command-line launch: to the running
//! player over the socket, or a window of its own.
//!
//! The classes are served from a multithreaded apartment on a thread of
//! their own, so the main thread is left without COM for winit, which
//! initialises OLE (single-threaded) there for the window's own drag and
//! drop.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use windows::core::{implement, IUnknown, Interface, Ref, BOOL, GUID};
use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_POINTER, POINTL};
use windows::Win32::System::Com::{
    CoInitializeEx, CoRegisterClassObject, CoRevokeClassObject, CoTaskMemFree, CoUninitialize,
    IClassFactory, IClassFactory_Impl, IDataObject, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED,
    REGCLS_MULTIPLEUSE,
};
use windows::Win32::System::Ole::{IDropTarget, IDropTarget_Impl, DROPEFFECT, DROPEFFECT_COPY};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{
    IShellItemArray, SHCreateShellItemArrayFromDataObject, SIGDN_FILESYSPATH,
};

use crate::ipc::Mode;

/// The "Play with QuickPlay" class, in the registry's string form.
pub const PLAY_CLSID: &str = "{19266F5F-F52A-45B6-8128-E2B8EA00FC3A}";
/// The "Add to QuickPlay playlist" class.
pub const ENQUEUE_CLSID: &str = "{8223808E-C129-4C9E-BA81-A5925350CA11}";

/// Longest a launch by COM waits for the drop. Explorer drops as soon as
/// the class is served; a launch nobody drops on exits after this.
const DROP_WAIT: Duration = Duration::from_secs(30);
/// Longest it then waits for Explorer to let go of the object, so the
/// classes are not withdrawn while Explorer is still returning from the
/// drop.
const RELEASE_WAIT: Duration = Duration::from_secs(2);

enum Event {
    Dropped(Mode, Vec<PathBuf>),
    Released,
}

/// Serves the two verb classes until Explorer drops a selection on one of
/// them, and returns it with the verb's mode. `None` when nothing came or
/// the classes could not be served: the launch then has nothing to do.
pub fn receive() -> Option<(Mode, Vec<PathBuf>)> {
    let (events_tx, events) = crossbeam_channel::unbounded();
    let (stop_tx, stop) = crossbeam_channel::bounded::<()>(1);
    let (ready_tx, ready) = crossbeam_channel::bounded(1);
    let live = Arc::new(AtomicUsize::new(0));
    let thread_live = live.clone();
    let server = std::thread::Builder::new()
        .name("quickplay-com".into())
        .spawn(move || serve(events_tx, thread_live, ready_tx, stop))
        .map_err(|e| tracing::warn!(error = %e, "verb server thread not started"))
        .ok()?;
    let dropped = match ready.recv() {
        Ok(Ok(())) => wait_for_drop(&events, &live),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "verb classes not served");
            None
        }
        Err(_) => None,
    };
    let _ = stop_tx.send(());
    let _ = server.join();
    if dropped.is_none() {
        tracing::warn!("started for a file-manager verb, but nothing was dropped");
    }
    dropped
}

fn wait_for_drop(events: &Receiver<Event>, live: &AtomicUsize) -> Option<(Mode, Vec<PathBuf>)> {
    let deadline = Instant::now() + DROP_WAIT;
    let dropped = loop {
        match events.recv_deadline(deadline) {
            Ok(Event::Dropped(mode, paths)) => break (mode, paths),
            Ok(Event::Released) => {}
            Err(_) => return None,
        }
    };
    let deadline = Instant::now() + RELEASE_WAIT;
    while live.load(Ordering::Acquire) > 0 {
        if events.recv_deadline(deadline).is_err() {
            break;
        }
    }
    Some(dropped)
}

/// The COM side, on its own thread: registers the class objects, reports
/// whether that worked, and withdraws them when told to stop.
fn serve(
    events: Sender<Event>,
    live: Arc<AtomicUsize>,
    ready: Sender<windows::core::Result<()>>,
    stop: Receiver<()>,
) {
    // SAFETY: plain COM calls on this thread; every registration made here
    // is revoked before the apartment is left.
    unsafe {
        if let Err(e) = CoInitializeEx(None, COINIT_MULTITHREADED).ok() {
            let _ = ready.send(Err(e));
            return;
        }
        let mut cookies = Vec::new();
        let mut result = Ok(());
        for (clsid, mode) in [(PLAY_CLSID, Mode::Play), (ENQUEUE_CLSID, Mode::Enqueue)] {
            let factory: IClassFactory = Factory {
                mode,
                events: events.clone(),
                live: live.clone(),
            }
            .into();
            match CoRegisterClassObject(
                &guid(clsid),
                &factory,
                CLSCTX_LOCAL_SERVER,
                REGCLS_MULTIPLEUSE,
            ) {
                Ok(cookie) => cookies.push(cookie),
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        let served = result.is_ok();
        let _ = ready.send(result);
        if served {
            let _ = stop.recv();
        }
        for cookie in cookies {
            let _ = CoRevokeClassObject(cookie);
        }
        CoUninitialize();
    }
}

fn guid(registry_form: &str) -> GUID {
    GUID::try_from(registry_form.trim_matches(['{', '}'])).expect("a valid CLSID literal")
}

#[implement(IClassFactory)]
struct Factory {
    mode: Mode,
    events: Sender<Event>,
    live: Arc<AtomicUsize>,
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        object: *mut *mut core::ffi::c_void,
    ) -> windows::core::Result<()> {
        if object.is_null() {
            return Err(E_POINTER.into());
        }
        // SAFETY: checked non-null above; COM's out pointer.
        unsafe { *object = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        self.live.fetch_add(1, Ordering::AcqRel);
        let target: IDropTarget = Target {
            mode: self.mode,
            events: self.events.clone(),
            live: self.live.clone(),
        }
        .into();
        // SAFETY: `iid` and `object` come from COM as they are.
        unsafe { target.query(iid, object).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

#[implement(IDropTarget)]
struct Target {
    mode: Mode,
    events: Sender<Event>,
    live: Arc<AtomicUsize>,
}

impl Drop for Target {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::AcqRel);
        let _ = self.events.send(Event::Released);
    }
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        _data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        accept(effect)
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        accept(effect)
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let paths = selection_paths(data.ok()?)?;
        tracing::info!(files = paths.len(), mode = ?self.mode, "selection dropped by the file manager");
        let _ = self.events.send(Event::Dropped(self.mode, paths));
        accept(effect)
    }
}

/// Explorer drops only when the target takes the data: it is copied.
fn accept(effect: *mut DROPEFFECT) -> windows::core::Result<()> {
    if effect.is_null() {
        return Err(E_POINTER.into());
    }
    // SAFETY: checked non-null; COM's in/out pointer.
    unsafe { *effect = DROPEFFECT_COPY };
    Ok(())
}

/// The file-system paths of the dropped items; an item that has none (a
/// virtual item, say) is skipped.
fn selection_paths(data: &IDataObject) -> windows::core::Result<Vec<PathBuf>> {
    // SAFETY: documented calls on live interfaces; each name the shell
    // allocates is copied and then freed with CoTaskMemFree.
    unsafe {
        let items: IShellItemArray = SHCreateShellItemArrayFromDataObject(data)?;
        let count = items.GetCount()?;
        let mut paths = Vec::with_capacity(count as usize);
        for i in 0..count {
            let Ok(name) = items
                .GetItemAt(i)
                .and_then(|item| item.GetDisplayName(SIGDN_FILESYSPATH))
            else {
                continue;
            };
            paths.push(PathBuf::from(OsString::from_wide(name.as_wide())));
            CoTaskMemFree(Some(name.0 as *const _));
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_class_ids_parse_and_differ() {
        assert_ne!(guid(PLAY_CLSID), guid(ENQUEUE_CLSID));
        assert_eq!(
            format!("{{{:?}}}", guid(PLAY_CLSID)),
            PLAY_CLSID,
            "the registry form round-trips"
        );
    }
}
