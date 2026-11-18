//! ⌘V with only a picture on the clipboard (a screenshot, a copied image) reaches no egui event: the windowing
//! layer turns ⌘V into a text paste and drops the keystroke when there is no text. On macOS a local key monitor
//! notices the keystroke itself, so CoCaptain can attach the picture.

use std::sync::atomic::{AtomicBool, Ordering};

static PRESSED: AtomicBool = AtomicBool::new(false);

/// Start watching for ⌘V (once; later calls do nothing).
#[cfg(target_os = "macos")]
pub fn install() {
    use std::ptr::NonNull;
    use std::sync::Once;

    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};

    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let handler = RcBlock::new(|ev: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit hands the monitor a valid event for the duration of the call
            let e = unsafe { ev.as_ref() };
            let command = e.modifierFlags().contains(NSEventModifierFlags::Command);
            let v = e
                .charactersIgnoringModifiers()
                .is_some_and(|c| c.to_string().eq_ignore_ascii_case("v"));
            if command && v {
                PRESSED.store(true, Ordering::SeqCst);
            }
            // pass the event on unchanged: text pastes still work as before
            ev.as_ptr()
        });
        // SAFETY: the block returns the event it was given, a valid pointer
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler)
        };
        // the monitor lives as long as the app
        std::mem::forget(monitor);
        std::mem::forget(handler);
    });
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}

/// Whether ⌘V was pressed since the last call.
pub fn take() -> bool {
    PRESSED.swap(false, Ordering::SeqCst)
}
