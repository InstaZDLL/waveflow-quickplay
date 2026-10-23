//! macOS: the files Finder opens in QuickPlay.
//!
//! Finder does not put a file it opens on the command line, as Explorer
//! and the Linux desktops do: it starts the app (or finds it running) and
//! sends it an "open documents" Apple event, which AppKit turns into
//! `application:openURLs:` on the application delegate. winit 0.30
//! installs its own delegate and panics if it is replaced, and that
//! delegate does not answer `application:openURLs:`, so [`run_native`]
//! builds the event loop itself, adds the method to winit's delegate class
//! through the Objective-C runtime before the loop starts (a file opened
//! at launch arrives before the first frame) and hands the loop to eframe.
//! The method queues the paths; the app takes them on its next frame
//! ([`take_opened`]) and plays them as a `play` hand-over from a second
//! launch: the queue is replaced.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{ffi, sel};
use objc2_foundation::{NSArray, NSURL};

/// The class winit registers for its application delegate.
const WINIT_DELEGATE: &str = "WinitApplicationDelegate";

/// Paths Finder opened that the app has not taken yet.
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// Wakes the app when Finder opens files while it is idle.
static REPAINT: OnceLock<egui::Context> = OnceLock::new();

/// `-[WinitApplicationDelegate application:openURLs:]`. One call per open:
/// a multi-selection in Finder arrives together. Called by AppKit on the
/// main thread; it must not unwind, and nothing in it panics.
unsafe extern "C" fn open_urls(
    _this: *mut AnyObject,
    _cmd: Sel,
    _application: *mut AnyObject,
    urls: *mut NSArray<NSURL>,
) {
    // SAFETY: AppKit passes a valid NSArray<NSURL> for the call's duration.
    let Some(urls) = (unsafe { urls.as_ref() }) else {
        return;
    };
    let paths: Vec<PathBuf> = urls
        .iter()
        // SAFETY: plain NSURL getters on a live object.
        .filter(|url| unsafe { url.isFileURL() })
        .filter_map(|url| unsafe { url.path() })
        .map(|path| PathBuf::from(path.to_string()))
        .collect();
    tracing::info!(files = paths.len(), "opened from Finder");
    if paths.is_empty() {
        return;
    }
    OPENED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .extend(paths);
    if let Some(ctx) = REPAINT.get() {
        ctx.request_repaint();
    }
}

/// Give winit's delegate class an `application:openURLs:`. Must run after
/// the event loop exists (which registers the class) and before it runs.
fn answer_open_urls() {
    let Some(class) = AnyClass::get(WINIT_DELEGATE) else {
        tracing::warn!("{WINIT_DELEGATE} not found: files opened from Finder will not arrive");
        return;
    };
    type OpenUrls = unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>);
    // SAFETY: an `Imp` is called with the arguments of the selector it is
    // registered for; `open_urls` takes exactly those of
    // `application:openURLs:` (self, _cmd, NSApplication *, NSArray *),
    // and "v@:@@" says so to the runtime.
    let imp: Imp = unsafe { std::mem::transmute::<OpenUrls, Imp>(open_urls) };
    let added = unsafe {
        ffi::class_addMethod(
            class as *const AnyClass as *mut ffi::objc_class,
            sel!(application:openURLs:).as_ptr(),
            Some(imp),
            c"v@:@@".as_ptr(),
        )
    };
    if added == ffi::NO {
        tracing::warn!("{WINIT_DELEGATE} already answers application:openURLs:");
    }
}

/// `eframe::run_native` with Finder's files delivered. Same options, same
/// app; the only difference is who builds the event loop.
pub fn run_native(
    app_name: &str,
    options: eframe::NativeOptions,
    app_creator: eframe::AppCreator<'_>,
) -> eframe::Result {
    let event_loop = winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event()
        .build()
        .map_err(eframe::Error::WinitEventLoop)?;
    answer_open_urls();
    let mut eframe_app = eframe::create_native(app_name, options, app_creator, &event_loop);
    event_loop
        .run_app(&mut eframe_app)
        .map_err(eframe::Error::WinitEventLoop)
}

/// Let Finder wake the app when it opens files while the app is idle.
pub fn set_repaint(ctx: &egui::Context) {
    let _ = REPAINT.set(ctx.clone());
}

/// The files Finder opened since the last call.
pub fn take_opened() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(|e| e.into_inner()))
}
