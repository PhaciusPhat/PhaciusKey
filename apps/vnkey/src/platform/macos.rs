//! macOS keyboard hook + text injection.
//!
//! Ports the Swift `KeyEventListener` + `ActionExecutor`. We call
//! `CGEventTapCreate` through raw FFI (rather than the `core-graphics` safe
//! wrapper) because that wrapper cannot *suppress* an event — an input method
//! must swallow the original keystroke and inject replacement text, which
//! requires returning `NULL` from the tap callback.
//!
//! Event creation and injection still use the safe `core-graphics` API.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
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
use crate::config;
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

extern "C" {
    /// libproc: fills `buffer` with the executable path of `pid`. Returns the
    /// path length, or <= 0 on failure. Part of libSystem — no extra link flag.
    fn proc_pidpath(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
}

thread_local! {
    /// The live tap's mach port, so the callback can re-enable it if macOS
    /// disables the tap (on timeout or heavy user input).
    static TAP_PORT: Cell<CFMachPortRef> = const { Cell::new(ptr::null_mut()) };
}

/// Owns the event-tap thread. The tap runs on its **own thread with its own
/// `CFRunLoop`**: tao's main run loop does not service arbitrary
/// `CFRunLoopSource`s, so a tap added there never fires. A dedicated run loop is
/// the reliable pattern.
pub struct Hook {
    _thread: std::thread::JoinHandle<()>,
}

impl KeyboardHook for Hook {
    fn install() -> Result<Self, String> {
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let thread = std::thread::Builder::new()
            .name("vnkey-eventtap".into())
            .spawn(move || match create_tap() {
                Ok(keepalive) => {
                    let _ = tx.send(Ok(()));
                    // Block forever, servicing the tap on this thread's run loop.
                    CFRunLoop::run_current();
                    drop(keepalive);
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            })
            .map_err(|e| format!("failed to spawn event-tap thread: {e}"))?;

        match rx.recv() {
            Ok(Ok(())) => Ok(Hook { _thread: thread }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("event-tap thread exited before signalling".to_string()),
        }
    }
}

/// Create + enable the tap on the current thread, returning the resources that
/// must stay alive for the tap to keep working.
fn create_tap() -> Result<(CFMachPort, CFRunLoopSource), String> {
    let mask: u64 = (1 << ET_KEY_DOWN) | (1 << ET_LEFT_MOUSE_DOWN) | (1 << ET_RIGHT_MOUSE_DOWN);

    // tap=HID(0), place=HeadInsert(0), options=Default(0).
    let port_ref = unsafe { CGEventTapCreate(0, 0, 0, mask, tap_callback, ptr::null_mut()) };
    if port_ref.is_null() {
        return Err("Failed to create event tap. Grant Accessibility permission in \
                    System Settings → Privacy & Security → Accessibility."
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

    Ok((port, source))
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

    let keycode = cg.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;

    // The global on/off shortcut outranks everything below — it must work even
    // while typing is disabled for the focused app, or the user could never
    // re-enable from the keyboard. Auto-repeat is ignored, or holding the combo
    // would flip the setting many times a second.
    if is_toggle_shortcut(keycode, cg.get_flags())
        && cg.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) == 0
    {
        state::update(|s| s.enabled = !s.enabled);
        state::reset();
        return ptr::null_mut(); // the combo is ours; don't let the app see it
    }

    // Track which app receives this keystroke, so per-app disable and the tray
    // menu's "Enable in <app>" item follow the user's focus.
    let pid = cg.get_integer_value_field(EventField::EVENT_TARGET_UNIX_PROCESS_ID);
    if let Some(name) = app_name_for_pid(pid) {
        state::set_current_app(&name);
    }
    if state::current_app_disabled() {
        // Everything passes through untouched in a disabled app, including
        // Backspace — the engine holds no composition here.
        state::reset();
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

    // The Delete/Backspace key needs its own path: it must pop the last raw
    // keystroke from the composition buffer (which can represent more than
    // one on-screen character, e.g. telex "as" → "á") and redraw, rather than
    // being read as a character and fed into the buffer like ordinary input.
    if keycode == VK_DELETE {
        let actions = state::backspace();
        if actions.is_empty() {
            return event; // not composing — let the native Backspace run
        }
        inject(&actions);
        return ptr::null_mut(); // we already redrew; swallow the original
    }

    let ch = match read_char(event) {
        Some(c) => c,
        None => return event,
    };

    // Arrow keys, Home/End/Page Up/Down, and function keys report through
    // `CGEventKeyboardGetUnicodeString` as control characters or codepoints in
    // the Unicode private-use area (NSHomeFunctionKey, NSF1FunctionKey, …).
    // On full-size keyboards these arrive *without* the Fn modifier flag (only
    // laptop Fn-combos set it), so the shortcut-flag check above misses them.
    // None of these are Vietnamese input — treat them as a composition
    // boundary and pass through untouched instead of corrupting the buffer.
    if ch.is_control() || ('\u{F700}'..='\u{F8FF}').contains(&ch) {
        state::reset();
        return event;
    }

    let actions = state::process_char(ch);
    if actions.is_empty() {
        return event; // pass through
    }

    inject(&actions);
    ptr::null_mut() // swallow the original keystroke
}

/// Whether this keydown is the configured enable/disable shortcut.
///
/// Matched by *keycode* rather than the produced character: with Control held,
/// `CGEventKeyboardGetUnicodeString` reports control characters, not letters.
/// The modifier set must match exactly, so ⌃⇧V never also fires on ⌘⌃⇧V.
fn is_toggle_shortcut(keycode: u16, flags: CGEventFlags) -> bool {
    let Some(sc) = config::parse_shortcut(&state::settings().toggle_shortcut) else {
        return false;
    };
    let Some(want) = keycode_for(sc.key) else {
        return false;
    };
    if keycode != want {
        return false;
    }
    flags.contains(CGEventFlags::CGEventFlagCommand) == sc.cmd
        && flags.contains(CGEventFlags::CGEventFlagControl) == sc.ctrl
        && flags.contains(CGEventFlags::CGEventFlagAlternate) == sc.alt
        && flags.contains(CGEventFlags::CGEventFlagShift) == sc.shift
}

/// ANSI-layout virtual keycode for a shortcut key. Layout-independent for
/// letters/digits on the keyboards we can reasonably support without
/// `UCKeyTranslate`; unknown keys disable the shortcut rather than guessing.
fn keycode_for(c: char) -> Option<u16> {
    Some(match c {
        'a' => 0, 's' => 1, 'd' => 2, 'f' => 3, 'h' => 4, 'g' => 5, 'z' => 6, 'x' => 7,
        'c' => 8, 'v' => 9, 'b' => 11, 'q' => 12, 'w' => 13, 'e' => 14, 'r' => 15,
        'y' => 16, 't' => 17, '1' => 18, '2' => 19, '3' => 20, '4' => 21, '6' => 22,
        '5' => 23, '9' => 25, '7' => 26, '8' => 28, '0' => 29, 'o' => 31, 'u' => 32,
        'i' => 34, 'p' => 35, 'l' => 37, 'j' => 38, 'k' => 40, 'n' => 45, 'm' => 46,
        ' ' => 49,
        _ => return None,
    })
}

thread_local! {
    /// pid → app name, cached because the tap sees every keystroke. All lookups
    /// happen on the tap thread. A recycled pid could serve a stale name until
    /// restart; accepted, since the alternative is a path lookup per key.
    static APP_NAMES: RefCell<HashMap<i64, String>> = RefCell::new(HashMap::new());
}

/// Human-readable name of the app that owns `pid` — the `.app` bundle name when
/// there is one ("Safari"), else the executable name.
fn app_name_for_pid(pid: i64) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    APP_NAMES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(name) = cache.get(&pid) {
            return Some(name.clone());
        }
        let name = app_name_from_path(&pid_path(pid as i32)?)?;
        cache.insert(pid, name.clone());
        Some(name)
    })
}

fn pid_path(pid: i32) -> Option<String> {
    // PROC_PIDPATHINFO_MAXSIZE = 4 * MAXPATHLEN.
    let mut buf = [0u8; 4096];
    let len = unsafe { proc_pidpath(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
    if len <= 0 {
        return None;
    }
    std::str::from_utf8(&buf[..len as usize]).ok().map(str::to_string)
}

fn app_name_from_path(path: &str) -> Option<String> {
    // "…/Safari.app/Contents/MacOS/Safari" → "Safari". The *first* .app wins so
    // a helper nested inside another bundle reports the app the user knows.
    for component in path.split('/') {
        if let Some(name) = component.strip_suffix(".app") {
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    path.rsplit('/').find(|s| !s.is_empty()).map(str::to_string)
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

/// Check whether the process is trusted for Accessibility. When `prompt` is
/// true, macOS shows the "grant permission" dialog if it isn't; when false it
/// only reports the current state (safe to poll every second).
pub(super) fn request_accessibility_permission(prompt: bool) -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let value = if prompt {
            CFBoolean::true_value()
        } else {
            CFBoolean::false_value()
        };
        let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}
