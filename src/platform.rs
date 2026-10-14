//! Operating-system integration: on macOS, files opened from Finder
//! (double-click, *Open With*, dropping on the Dock icon).
//!
//! macOS sends these as an "open documents" Apple Event. winit owns the
//! application delegate (and aborts if it is replaced), so instead we install
//! an Apple Event handler, at the moment Apple documents for it:
//! `applicationWillFinishLaunching`. That is after AppKit has installed its
//! default handlers (ours replaces its "open documents" one) and before it
//! dispatches the document a launch was for. eframe only creates the app
//! after launching finishes, so [`install`] runs before the event loop starts
//! and waits for `NSApplicationWillFinishLaunching`.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Files macOS asked us to open, waiting for the app to pick them up.
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// Wakes the UI when files arrive while it is idle.
static REPAINT: OnceLock<egui::Context> = OnceLock::new();

/// Take the files opened from Finder since the last call.
pub fn take_opened_files() -> Vec<PathBuf> {
    #[cfg(test)]
    return TEST_OPENED.with(|q| std::mem::take(&mut *q.borrow_mut()));
    #[cfg(not(test))]
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(test)]
thread_local! {
    /// Tests run in parallel, each with its own app on its own thread, so
    /// simulated opens go to the thread that made them.
    static TEST_OPENED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Let file-open events wake the UI.
pub fn set_repaint_context(ctx: &egui::Context) {
    let _ = REPAINT.set(ctx.clone());
}

/// Queue files as if macOS had asked us to open them (used by tests).
#[cfg(test)]
pub(crate) fn simulate_open(paths: impl IntoIterator<Item = PathBuf>) {
    TEST_OPENED.with(|q| q.borrow_mut().extend(paths));
}

fn push_opened(paths: impl IntoIterator<Item = PathBuf>) {
    let mut queue = OPENED.lock().unwrap_or_else(|e| e.into_inner());
    queue.extend(paths);
    if std::env::var_os("LITESHEET_LOG").is_some() {
        eprintln!("litesheet: asked to open {queue:?}");
    }
    if let Some(ctx) = REPAINT.get() {
        ctx.request_repaint();
    }
}

/// Start listening for files opened from Finder. Call on the main thread
/// before the event loop is created.
#[cfg(target_os = "macos")]
pub fn install() {
    macos::install();
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, Sel};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::NSApplicationWillFinishLaunchingNotification;
    use objc2_foundation::{
        NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter,
        NSObjectProtocol,
    };

    /// Four-character Apple Event codes.
    const K_CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
    const K_AE_OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
    const KEY_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "LiteSheetOpenHandler"]
        struct OpenHandler;

        impl OpenHandler {
            /// NSApplicationWillFinishLaunching: take over "open documents".
            #[unsafe(method(willFinishLaunching:))]
            fn will_finish_launching(&self, _notification: &NSNotification) {
                let manager = NSAppleEventManager::sharedAppleEventManager();
                let handler: &AnyObject = self.as_ref();
                let selector: Sel = sel!(handleOpenDocuments:withReply:);
                unsafe {
                    let _: () = msg_send![
                        &*manager,
                        setEventHandler: handler,
                        andSelector: selector,
                        forEventClass: K_CORE_EVENT_CLASS,
                        andEventID: K_AE_OPEN_DOCUMENTS
                    ];
                }
            }

            /// The "open documents" event: its direct object lists file URLs.
            #[unsafe(method(handleOpenDocuments:withReply:))]
            fn handle_open_documents(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
                let list: Option<Retained<NSAppleEventDescriptor>> =
                    unsafe { msg_send![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT] };
                let Some(list) = list else { return };
                let paths = (1..=list.numberOfItems())
                    .filter_map(|i| list.descriptorAtIndex(i))
                    .filter_map(|item| item.fileURLValue())
                    .filter_map(|url| url.to_file_path());
                super::push_opened(paths);
            }
        }

        unsafe impl NSObjectProtocol for OpenHandler {}
    );

    impl OpenHandler {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let handler = OpenHandler::new(mtm);
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &handler,
                sel!(willFinishLaunching:),
                Some(NSApplicationWillFinishLaunchingNotification),
                None,
            );
        }
        // Neither the notification centre nor the event manager retains it.
        std::mem::forget(handler);
    }
}
