//! macOS keyboard hook + text injection.
//!
//! Ports the Swift `KeyEventListener` + `ActionExecutor`. We call
//! `CGEventTapCreate` through raw FFI (rather than the `core-graphics` safe
//! wrapper) because that wrapper cannot *suppress* an event — an input method
//! must swallow the original keystroke and inject replacement text, which
//! requires returning `NULL` from the tap callback.
//!
//! Event creation uses the safe `core-graphics` API; posting goes through raw
//! `CGEventTapPostEvent` (the wrapper only exposes `CGEventPost`, which is the
//! wrong call from inside a tap callback — see `inject`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::os::raw::c_void;
use std::path::{Path, PathBuf};
use std::ptr;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::mach_port::{CFMachPort, CFMachPortRef};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop, CFRunLoopSource};
use core_foundation::string::{CFString, CFStringRef};

use core_graphics::event::{CGEvent, CGEventFlags, EventField};
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
/// Return, Tab, Escape and keypad-Enter — handled by keycode, before the
/// character path.
const VK_RETURN: u16 = 0x24;
const VK_TAB: u16 = 0x30;
const VK_ESC: u16 = 0x35;
const VK_KP_ENTER: u16 = 0x4C;

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
    /// Posts an event from inside a tap callback at this tap's position in the
    /// event stream — the API designed for replacing a tapped keystroke.
    /// Unlike `CGEventPost(kCGHIDEventTap, …)`, the posted event keeps its
    /// order relative to keystrokes already in flight and is only seen by taps
    /// *after* ours, so injected events never re-enter this callback.
    fn CGEventTapPostEvent(proxy: CGEventTapProxy, event: CGEventRefRaw);
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

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    /// Whether any process holds Secure Event Input (HIToolbox).
    fn IsSecureEventInputEnabled() -> bool;
}

/// See [`super::secure_input_active`]. Safe to poll.
pub(super) fn secure_input_active() -> bool {
    unsafe { IsSecureEventInputEnabled() }
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
    proxy: CGEventTapProxy,
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

    // Track which app receives this keystroke, so per-app state and the tray
    // menu's "Enable in <app>" item follow the user's focus. Done before the
    // shortcut check: in per-app mode the toggle must apply to the app that
    // received the combo, not to whichever app got the previous keystroke.
    let pid = cg.get_integer_value_field(EventField::EVENT_TARGET_UNIX_PROCESS_ID);
    if let Some(name) = app_name_for_pid(pid) {
        state::set_current_app(&name);
    }

    // The on/off shortcut outranks everything below — it must work even while
    // typing is disabled for the focused app, or the user could never re-enable
    // from the keyboard. Auto-repeat is ignored, or holding the combo would
    // flip the setting many times a second.
    if is_toggle_shortcut(keycode, cg.get_flags())
        && cg.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) == 0
    {
        state::toggle_vietnamese();
        return ptr::null_mut(); // the combo is ours; don't let the app see it
    }

    if !state::vietnamese_active() {
        // Everything passes through untouched while typing is off here,
        // including Backspace — the engine holds no composition.
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
        if inject(proxy, &actions) {
            return ptr::null_mut(); // we already redrew; swallow the original
        }
        // Injection unavailable: the native Backspace still deletes one
        // displayed character, which is exactly what the engine recorded.
        return event;
    }

    // Esc restores the raw keystrokes of the word being composed ("đấy" →
    // "ddaays") and is consumed by that; with nothing to restore it is the
    // app's Esc as usual (and a word boundary).
    if keycode == VK_ESC {
        let actions = state::restore_raw();
        if !actions.is_empty() && inject(proxy, &actions) {
            return ptr::null_mut();
        }
        state::reset();
        return event;
    }

    // Enter and Tab commit the word (so macros expand) but always pass
    // through — the app needs the key itself. CGEventTapPostEvent puts the
    // expansion into the stream *before* the returned event, so the edit
    // lands ahead of the Enter/Tab.
    if matches!(keycode, VK_RETURN | VK_KP_ENTER | VK_TAB) {
        let actions = state::commit_word();
        if !actions.is_empty() {
            let _ = inject(proxy, &actions);
        }
        return event;
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

    if inject(proxy, &actions) {
        ptr::null_mut() // swallow the original keystroke
    } else {
        // Injection unavailable: pass the raw keystroke through rather than
        // swallowing it with no replacement — a plain letter beats a lost one.
        event
    }
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

/// Names of every `.app` bundle installed in the standard application
/// folders, using the same display name convention as [`app_name_from_path`]
/// ("Safari.app" → "Safari") so entries line up with the per-app settings.
pub(super) fn installed_apps() -> Vec<String> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    let mut names = Vec::new();
    for root in &roots {
        collect_apps(root, 1, &mut names);
    }
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

/// Push the name of every `.app` bundle under `dir`, descending `depth`
/// levels into plain subfolders (e.g. /Applications/Utilities) but never into
/// a bundle itself — helpers nested inside another app aren't user-facing.
fn collect_apps(dir: &Path, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(stem) = name.strip_suffix(".app") {
            if !stem.is_empty() {
                out.push(stem.to_string());
            }
        } else if depth > 0 && !name.starts_with('.') && entry.path().is_dir() {
            collect_apps(&entry.path(), depth - 1, out);
        }
    }
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

/// Synthesize the edit actions as `CGEvent`s, each tagged as synthetic, and
/// post them at this tap's position via `CGEventTapPostEvent`.
///
/// Returns `false` when nothing was posted — the caller must then pass the
/// original keystroke through instead of swallowing it, or the character is
/// silently lost (the "sometimes the first letter of a word vanishes" bug).
/// Every event is built before any is posted, so a mid-sequence creation
/// failure falls back to the untouched keystroke rather than a half-applied
/// edit.
fn inject(proxy: CGEventTapProxy, actions: &[EditAction]) -> bool {
    let source = match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let mut events: Vec<CGEvent> = Vec::new();
    for action in actions {
        match action {
            EditAction::Backspace(count) => {
                for _ in 0..*count {
                    let (Some(down), Some(up)) = (
                        make_key(&source, VK_DELETE, true),
                        make_key(&source, VK_DELETE, false),
                    ) else {
                        return false;
                    };
                    events.push(down);
                    events.push(up);
                }
            }
            // An empty insert is the engine saying "swallow the key, the
            // screen is already right" — nothing to post. A keycode-0 keydown
            // with no string could be read as the letter 'a' by apps that
            // fall back to the keycode.
            EditAction::Insert(text) if text.is_empty() => {}
            EditAction::Insert(text) => {
                let (Some(down), Some(up)) =
                    (make_key(&source, 0, true), make_key(&source, 0, false))
                else {
                    return false;
                };
                let utf16: Vec<u16> = text.encode_utf16().collect();
                down.set_string_from_utf16_unchecked(&utf16);
                events.push(down);
                events.push(up);
            }
        }
    }

    // "Slow typing" compatibility: a small pause between events for apps
    // that drop rapid synthetic bursts. Only for apps the user listed, and
    // short enough that even a long rewrite stays far under the event-tap
    // watchdog (~8 events × 3 ms).
    let pause = state::slow_typing_here().then(|| std::time::Duration::from_millis(3));
    for ev in &events {
        unsafe { CGEventTapPostEvent(proxy, ev.as_ptr() as CGEventRefRaw) };
        if let Some(pause) = pause {
            std::thread::sleep(pause);
        }
    }
    true
}

/// A synthetic key event, tagged so the tap ignores it if it ever loops back.
fn make_key(source: &CGEventSource, keycode: u16, down: bool) -> Option<CGEvent> {
    let ev = CGEvent::new_keyboard_event(source.clone(), keycode, down).ok()?;
    ev.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTHETIC_MARKER);
    Some(ev)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_app_bundles_one_folder_deep() {
        let root = std::env::temp_dir().join(format!("vnkey-apps-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // Helpers nested inside a bundle and folders below the depth limit
        // must both be skipped; loose files and dot-folders are ignored.
        std::fs::create_dir_all(root.join("Safari.app/Contents/Helper.app")).unwrap();
        std::fs::create_dir_all(root.join("Utilities/Terminal.app")).unwrap();
        std::fs::create_dir_all(root.join("Utilities/Deeper/Hidden.app")).unwrap();
        std::fs::create_dir_all(root.join(".Trashes/Ghost.app")).unwrap();
        std::fs::write(root.join("notes.txt"), "").unwrap();

        let mut names = Vec::new();
        collect_apps(&root, 1, &mut names);
        names.sort();
        assert_eq!(names, ["Safari", "Terminal"]);

        let _ = std::fs::remove_dir_all(&root);
    }
}
