use std::{
    ptr::NonNull,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use block2::RcBlock;
use core_foundation::{
    base::{CFType, TCFType},
    boolean::CFBoolean,
    dictionary::{CFDictionary, CFDictionaryRef},
    string::CFString,
};
use objc2_app_kit::{NSEvent, NSEventMask};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
}

static LAST_INPUT_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn watch_input() {
    LAST_INPUT_MS.store(now_ms(), Ordering::Relaxed);
    let mask = NSEventMask::KeyDown
        | NSEventMask::LeftMouseDown
        | NSEventMask::RightMouseDown
        | NSEventMask::OtherMouseDown
        | NSEventMask::ScrollWheel;
    let handler = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        LAST_INPUT_MS.store(now_ms(), Ordering::Relaxed);
        event.as_ptr()
    });
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) };
    std::mem::forget(monitor);
}

pub fn last_input() -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(LAST_INPUT_MS.load(Ordering::Relaxed))
}

pub fn screen_locked() -> bool {
    let raw = unsafe { CGSessionCopyCurrentDictionary() };
    if raw.is_null() {
        return true;
    }
    let dict: CFDictionary<CFString, CFType> = unsafe { CFDictionary::wrap_under_create_rule(raw) };
    let flag = |name: &'static str| {
        dict.find(CFString::from_static_string(name))
            .and_then(|v| v.downcast::<CFBoolean>())
            .map(bool::from)
    };
    let locked = flag("CGSSessionScreenIsLocked").unwrap_or(false);
    let on_console = flag("kCGSSessionOnConsoleKey").unwrap_or(true);
    locked || !on_console
}

#[cfg(test)]
mod tests {
    #[test]
    fn session_state_is_readable() {
        assert!(!super::screen_locked());
    }
}
