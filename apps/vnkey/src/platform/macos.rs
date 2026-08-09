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

const SYNTHETIC_MARKER: i64 = 0x0056_4E4B_4559;

const VK_DELETE: u16 = 0x33;
const VK_RETURN: u16 = 0x24;
const VK_TAB: u16 = 0x30;
const VK_ESC: u16 = 0x35;
const VK_KP_ENTER: u16 = 0x4C;

// CGEventType raw values, as passed to the tap callback.
const ET_LEFT_MOUSE_DOWN: u32 = 1;
const ET_RIGHT_MOUSE_DOWN: u32 = 3;
const ET_KEY_DOWN: u32 = 10;
const ET_FLAGS_CHANGED: u32 = 12;
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
    fn IsSecureEventInputEnabled() -> bool;
}

pub(super) fn secure_input_active() -> bool {
    unsafe { IsSecureEventInputEnabled() }
}

extern "C" {
    fn proc_pidpath(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
}

thread_local! {
    static TAP_PORT: Cell<CFMachPortRef> = const { Cell::new(ptr::null_mut()) };
    static CHORD: Cell<config::ChordWatch> = const { Cell::new(config::ChordWatch::new()) };
}

fn held_mask(flags: CGEventFlags) -> u8 {
    (if flags.contains(CGEventFlags::CGEventFlagControl) {
        config::MOD_CTRL
    } else {
        0
    }) | (if flags.contains(CGEventFlags::CGEventFlagShift) {
        config::MOD_SHIFT
    } else {
        0
    }) | (if flags.contains(CGEventFlags::CGEventFlagAlternate) {
        config::MOD_ALT
    } else {
        0
    }) | (if flags.contains(CGEventFlags::CGEventFlagCommand) {
        config::MOD_CMD
    } else {
        0
    })
}

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

fn create_tap() -> Result<(CFMachPort, CFRunLoopSource), String> {
    let mask: u64 = (1 << ET_KEY_DOWN)
        | (1 << ET_FLAGS_CHANGED)
        | (1 << ET_LEFT_MOUSE_DOWN)
        | (1 << ET_RIGHT_MOUSE_DOWN);

    // tap=HID(0), place=HeadInsert(0), options=Default(0).
    let port_ref = unsafe { CGEventTapCreate(0, 0, 0, mask, tap_callback, ptr::null_mut()) };
    if port_ref.is_null() {
        return Err(
            "Failed to create event tap. Grant Accessibility permission in \
                    System Settings → Privacy & Security → Accessibility."
                .to_string(),
        );
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

unsafe extern "C" fn tap_callback(
    proxy: CGEventTapProxy,
    etype: u32,
    event: CGEventRefRaw,
    _user_info: *mut c_void,
) -> CGEventRefRaw {
    match etype {
        ET_TAP_DISABLED_BY_TIMEOUT | ET_TAP_DISABLED_BY_USER_INPUT => {
            TAP_PORT.with(|p| {
                let port = p.get();
                if !port.is_null() {
                    CGEventTapEnable(port, true);
                }
            });
            return event;
        }
        ET_FLAGS_CHANGED => {
            // Never swallowed: another application would be left believing a modifier is
            // still held.
            let cg = ManuallyDrop::new(CGEvent::from_ptr(event as *mut _));
            if modifier_only_toggle_fired(held_mask(cg.get_flags())) {
                state::toggle_vietnamese();
            }
            return event;
        }
        ET_LEFT_MOUSE_DOWN | ET_RIGHT_MOUSE_DOWN => {
            CHORD.with(|chord| {
                let mut watch = chord.get();
                watch.interrupted();
                chord.set(watch);
            });
            state::reset();
            return event;
        }
        ET_KEY_DOWN => {}
        _ => return event,
    }

    // SAFETY: borrowed, not owned — the system still owns the event.
    let cg = ManuallyDrop::new(CGEvent::from_ptr(event as *mut _));

    if cg.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) == SYNTHETIC_MARKER {
        return event;
    }

    CHORD.with(|chord| {
        let mut watch = chord.get();
        watch.interrupted();
        chord.set(watch);
    });

    let keycode = cg.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;

    // Typing into our own settings window must not file PhaciusKey into the
    // very list of applications the user is curating there.
    let pid = cg.get_integer_value_field(EventField::EVENT_TARGET_UNIX_PROCESS_ID);
    if pid != i64::from(std::process::id()) {
        if let Some(name) = app_name_for_pid(pid) {
            state::set_current_app(&name);
        }
    }

    if is_toggle_shortcut(keycode, cg.get_flags())
        && cg.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) == 0
    {
        state::toggle_vietnamese();
        return ptr::null_mut();
    }

    if !state::vietnamese_active() {
        state::reset();
        return event;
    }

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

    if keycode == VK_DELETE {
        let actions = state::backspace();
        if actions.is_empty() {
            return event;
        }
        if inject(proxy, &actions) {
            return ptr::null_mut();
        }
        return event;
    }

    if keycode == VK_ESC {
        let actions = state::restore_raw();
        if !actions.is_empty() && inject(proxy, &actions) {
            return ptr::null_mut();
        }
        state::reset();
        return event;
    }

    if matches!(keycode, VK_RETURN | VK_KP_ENTER | VK_TAB) {
        let actions = if keycode == VK_TAB {
            state::commit_word()
        } else {
            state::commit_line()
        };
        if !actions.is_empty() {
            let _ = inject(proxy, &actions);
        }
        return event;
    }

    let ch = match read_char(event) {
        Some(c) => c,
        None => return event,
    };

    if ch.is_control() || ('\u{F700}'..='\u{F8FF}').contains(&ch) {
        state::reset();
        return event;
    }

    let actions = state::process_char(ch);
    if actions.is_empty() {
        return event;
    }

    if inject(proxy, &actions) {
        ptr::null_mut()
    } else {
        event
    }
}

fn is_toggle_shortcut(keycode: u16, flags: CGEventFlags) -> bool {
    if state::shortcut_recording() {
        return false;
    }
    let Some(sc) = state::toggle_shortcut() else {
        return false;
    };
    let Some(want) = sc.key.and_then(keycode_for) else {
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

fn modifier_only_toggle_fired(held: u8) -> bool {
    if state::shortcut_recording() {
        return false;
    }
    let Some(sc) = state::toggle_shortcut() else {
        return false;
    };
    if sc.key.is_some() {
        return false;
    }
    CHORD.with(|chord| {
        let mut watch = chord.get();
        let fired = watch.modifiers(held, sc.modifier_mask());
        chord.set(watch);
        fired
    })
}

#[rustfmt::skip]
fn keycode_for(c: char) -> Option<u16> {
    // ANSI-layout virtual keycodes.
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
    static APP_NAMES: RefCell<HashMap<i64, String>> = RefCell::new(HashMap::new());
}

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
    std::str::from_utf8(&buf[..len as usize])
        .ok()
        .map(str::to_string)
}

fn app_name_from_path(path: &str) -> Option<String> {
    for component in path.split('/') {
        if let Some(name) = component.strip_suffix(".app") {
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    path.rsplit('/').find(|s| !s.is_empty()).map(str::to_string)
}

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

fn collect_apps(dir: &Path, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
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

unsafe fn read_char(event: CGEventRefRaw) -> Option<char> {
    let mut buf = [0u16; 4];
    let mut len: usize = 0;
    CGEventKeyboardGetUnicodeString(event, buf.len(), &mut len, buf.as_mut_ptr());
    if len == 0 {
        return None;
    }
    String::from_utf16_lossy(&buf[..len.min(buf.len())])
        .chars()
        .next()
}

const AUTOCOMPLETE_SENTINEL: &str = "\u{202F}";

fn with_autocomplete_fix(actions: &[EditAction]) -> Vec<EditAction> {
    let Some(EditAction::Backspace(count)) = actions.first() else {
        return actions.to_vec();
    };
    let Some(count) = count.checked_add(1) else {
        return actions.to_vec();
    };

    let mut out = Vec::with_capacity(actions.len() + 1);
    out.push(EditAction::Insert(AUTOCOMPLETE_SENTINEL.to_string()));
    out.push(EditAction::Backspace(count));
    out.extend_from_slice(&actions[1..]);
    out
}

fn inject(proxy: CGEventTapProxy, actions: &[EditAction]) -> bool {
    let source = match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let patched;
    let actions = if state::autocomplete_fix_here() {
        patched = with_autocomplete_fix(actions);
        &patched[..]
    } else {
        actions
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

    let pause = state::slow_typing_here().then(|| std::time::Duration::from_millis(3));
    for ev in &events {
        unsafe { CGEventTapPostEvent(proxy, ev.as_ptr() as CGEventRefRaw) };
        if let Some(pause) = pause {
            std::thread::sleep(pause);
        }
    }
    true
}

fn make_key(source: &CGEventSource, keycode: u16, down: bool) -> Option<CGEvent> {
    let ev = CGEvent::new_keyboard_event(source.clone(), keycode, down).ok()?;
    ev.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTHETIC_MARKER);
    Some(ev)
}

#[allow(dead_code)]
pub(super) fn open_accessibility_settings() {
    crate::update::open_url(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    );
}

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
    fn autocomplete_fix_prefixes_a_sentinel_and_deletes_it_again() {
        let actions = vec![EditAction::Backspace(2), EditAction::Insert("đ".into())];
        assert_eq!(
            with_autocomplete_fix(&actions),
            vec![
                EditAction::Insert(AUTOCOMPLETE_SENTINEL.to_string()),
                EditAction::Backspace(3),
                EditAction::Insert("đ".into()),
            ]
        );
    }

    #[test]
    fn autocomplete_fix_leaves_a_plain_insert_alone() {
        let actions = vec![EditAction::Insert("a".into())];
        assert_eq!(with_autocomplete_fix(&actions), actions);
    }

    #[test]
    fn autocomplete_fix_declines_a_batch_at_the_backspace_ceiling() {
        let actions = vec![
            EditAction::Backspace(u8::MAX),
            EditAction::Insert("x".into()),
        ];
        assert_eq!(with_autocomplete_fix(&actions), actions);
    }

    #[test]
    fn collects_app_bundles_one_folder_deep() {
        let root = std::env::temp_dir().join(format!("vnkey-apps-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
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
