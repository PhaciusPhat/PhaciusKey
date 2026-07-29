//! macOS keyboard hook + text injection.
//!
//! Ports the Swift `KeyEventListener` + `ActionExecutor`. We call
//! `CGEventTapCreate` through raw FFI (rather than the `core-graphics` safe
//! wrapper) because that wrapper cannot *suppress* an event — an input method
//! must swallow the original keystroke and inject replacement text, which
//! requires returning `NULL` from the tap callback.
//!
//! Event creation and injection still use the safe `core-graphics` API.

use std::cell::Cell;
use std::mem::ManuallyDrop;
use std::os::raw::c_void;
use std::ptr;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::mach_port::{CFMachPort, CFMachPortRef};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop, CFRunLoopSource};
use core_foundation::string::{CFString, CFStringRef};

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, EventField};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use foreign_types::ForeignType;

use vnkey_core::EditAction;

use super::KeyboardHook;
use crate::state;

/// Sentinel written into `eventSourceUserData` of every event we synthesize, so
/// the tap callback recognizes and ignores our own injected keystrokes rather
/// than re-feeding them into the engine. ("VNKEY" in ASCII.)
const SYNTHETIC_MARKER: i64 = 0x0056_4E4B_4559;

/// Virtual key code for the Delete (Backspace) key.
const VK_DELETE: u16 = 0x33;

// CGEventType raw values (passed to the tap callback as a u32).
const ET_LEFT_MOUSE_DOWN: u32 = 1;
const ET_RIGHT_MOUSE_DOWN: u32 = 3;
const ET_KEY_DOWN: u32 = 10;
const ET_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const ET_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

type CGEventRefRaw = *mut c_void;
type CGEventTapProxy = *const c_void;
type TapCallback =
    unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRefRaw, *mut c_void) -> CGEventRefRaw;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventKeyboardGetUnicodeString(
        event: CGEventRefRaw,
        max_len: usize,
        actual_len: *mut usize,
        unicode_string: *mut u16,
    );
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

thread_local! {
    /// The live tap's mach port, so the callback can re-enable it if macOS
    /// disables the tap (on timeout or heavy user input).
    static TAP_PORT: Cell<CFMachPortRef> = const { Cell::new(ptr::null_mut()) };
}

/// Owns the event tap resources. Dropping it tears the hook down.
pub struct Hook {
    _port: CFMachPort,
    _source: CFRunLoopSource,
}

impl KeyboardHook for Hook {
    fn install() -> Result<Self, String> {
        // Prompt for Accessibility permission (required for event taps).
        request_accessibility_permission();

        let mask: u64 = (1 << ET_KEY_DOWN) | (1 << ET_LEFT_MOUSE_DOWN) | (1 << ET_RIGHT_MOUSE_DOWN);

        // tap=HID(0), place=HeadInsert(0), options=Default(0).
        let port_ref = unsafe {
            CGEventTapCreate(0, 0, 0, mask, tap_callback, ptr::null_mut())
        };
        if port_ref.is_null() {
            return Err("Failed to create event tap. Grant Accessibility permission in \
                        System Settings → Privacy & Security → Accessibility, then relaunch."
                .to_string());
        }

        let port = unsafe { CFMachPort::wrap_under_create_rule(port_ref) };
        let source = port
            .create_runloop_source(0)
            .map_err(|_| "Failed to create run loop source for event tap.".to_string())?;

        let run_loop = CFRunLoop::get_current();
        unsafe {
            run_loop.add_source(&source, kCFRunLoopCommonModes);
            CGEventTapEnable(port.as_concrete_TypeRef(), true);
        }
        TAP_PORT.with(|p| p.set(port.as_concrete_TypeRef()));

        Ok(Hook { _port: port, _source: source })
    }
}

/// The C-ABI tap callback. Returns the original event pointer to pass a key
/// through, or `NULL` to swallow it.
unsafe extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    etype: u32,
    event: CGEventRefRaw,
    _user_info: *mut c_void,
) -> CGEventRefRaw {
    match etype {
        // macOS disables the tap if our callback runs too long; re-enable it,
        // otherwise typing silently stops working for good.
        ET_TAP_DISABLED_BY_TIMEOUT | ET_TAP_DISABLED_BY_USER_INPUT => {
            TAP_PORT.with(|p| {
                let port = p.get();
                if !port.is_null() {
                    CGEventTapEnable(port, true);
                }
            });
            return event;
        }
        // Reset composition when the user clicks somewhere new.
        ET_LEFT_MOUSE_DOWN | ET_RIGHT_MOUSE_DOWN => {
            state::reset();
            return event;
        }
        ET_KEY_DOWN => {}
        _ => return event,
    }

    // Borrow the event without taking ownership (the system still owns it).
    let cg = ManuallyDrop::new(CGEvent::from_ptr(event as *mut _));

    // Ignore events we injected ourselves.
    if cg.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) == SYNTHETIC_MARKER {
        return event;
    }

    // Keys pressed with Command/Control/Option are shortcuts (⌘Tab, ⌃C, …), and
    // Fn-flagged keys are arrows / function keys. None are Vietnamese input:
    // reset composition (a shortcut is a word boundary) and pass them through
    // untouched. Shift is intentionally excluded so capitals still compose.
    const SHORTCUT_FLAGS: CGEventFlags = CGEventFlags::from_bits_truncate(
        CGEventFlags::CGEventFlagCommand.bits()
            | CGEventFlags::CGEventFlagControl.bits()
            | CGEventFlags::CGEventFlagAlternate.bits()
            | CGEventFlags::CGEventFlagSecondaryFn.bits(),
    );
    if cg.get_flags().intersects(SHORTCUT_FLAGS) {
        state::reset();
        return event;
    }

    let ch = match read_char(event) {
        Some(c) => c,
        None => return event,
    };

    let actions = state::process_char(ch);
    if actions.is_empty() {
        return event; // pass through
    }

    inject(&actions);
    ptr::null_mut() // swallow the original keystroke
}

/// Read the Unicode character the key would have produced.
unsafe fn read_char(event: CGEventRefRaw) -> Option<char> {
    let mut buf = [0u16; 4];
    let mut len: usize = 0;
    CGEventKeyboardGetUnicodeString(event, buf.len(), &mut len, buf.as_mut_ptr());
    if len == 0 {
        return None;
    }
    String::from_utf16_lossy(&buf[..len.min(buf.len())]).chars().next()
}

/// Synthesize the edit actions as `CGEvent`s, each tagged as synthetic.
fn inject(actions: &[EditAction]) {
    let source = match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
        Ok(s) => s,
        Err(_) => return,
    };

    for action in actions {
        match action {
            EditAction::Backspace(count) => {
                for _ in 0..*count {
                    post_key(&source, VK_DELETE, true);
                    post_key(&source, VK_DELETE, false);
                }
            }
            EditAction::Insert(text) => {
                let utf16: Vec<u16> = text.encode_utf16().collect();
                if let Ok(down) = CGEvent::new_keyboard_event(source.clone(), 0, true) {
                    down.set_string_from_utf16_unchecked(&utf16);
                    down.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTHETIC_MARKER);
                    down.post(CGEventTapLocation::HID);
                }
                if let Ok(up) = CGEvent::new_keyboard_event(source.clone(), 0, false) {
                    up.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTHETIC_MARKER);
                    up.post(CGEventTapLocation::HID);
                }
            }
        }
    }
}

fn post_key(source: &CGEventSource, keycode: u16, down: bool) {
    if let Ok(ev) = CGEvent::new_keyboard_event(source.clone(), keycode, down) {
        ev.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTHETIC_MARKER);
        ev.post(CGEventTapLocation::HID);
    }
}

/// Ask macOS for Accessibility permission, showing the system prompt if needed.
/// Returns whether the process is currently trusted.
fn request_accessibility_permission() -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let value = CFBoolean::true_value();
        let options =
            CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}
