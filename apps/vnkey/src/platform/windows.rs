use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, HGLOBAL, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, GetKeyboardState, SendInput, ToUnicodeEx, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_BACK,
    VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F12, VK_HOME, VK_INSERT,
    VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RCONTROL,
    VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_TAB, VK_UP, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use vnkey_core::EditAction;

use super::KeyboardHook;
use crate::{config, state};

const SYNTHETIC_MARKER: usize = 0x0056_4E4B_4559;

const CF_UNICODETEXT: u32 = 13;

const PASTE_SETTLE: Duration = Duration::from_millis(120);

const CLIPBOARD_ATTEMPTS: u32 = 12;
const CLIPBOARD_RETRY_DELAY: Duration = Duration::from_millis(10);

thread_local! {
    static HOOK_HANDLE: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

pub struct Hook {
    handle: HHOOK,
}

impl KeyboardHook for Hook {
    fn install() -> Result<Self, String> {
        let handle = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)
                .map_err(|e| format!("SetWindowsHookExW failed: {e}"))?
        };
        HOOK_HANDLE.with(|h| h.set(handle.0 as isize));
        Ok(Hook { handle })
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.handle);
        }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return call_next(code, wparam, lparam);
    }

    let msg = wparam.0 as u32;
    let is_key_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_key_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    if !is_key_down && !is_key_up {
        return call_next(code, wparam, lparam);
    }

    let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);

    if kb.dwExtraInfo == SYNTHETIC_MARKER {
        return call_next(code, wparam, lparam);
    }

    let vk = kb.vkCode as u16;

    if modifier_only_toggle_fired(vk, is_key_up) {
        state::toggle_vietnamese();
    }
    if !is_key_up && modifier_bit(vk).is_none() {
        chord_interrupted(vk);
    }

    // A low-level hook reports a held key as repeated key-downs with nothing to
    // mark them as repeats, so the release is what re-arms the shortcut.
    if is_key_up {
        if is_toggle_shortcut(vk) {
            TOGGLE_HELD.store(false, Ordering::Relaxed);
        }
        return call_next(code, wparam, lparam);
    }

    if is_toggle_shortcut(vk) {
        if !TOGGLE_HELD.swap(true, Ordering::Relaxed) {
            state::toggle_vietnamese();
        }
        return LRESULT(1);
    }

    if !state::vietnamese_active() {
        state::reset();
        return call_next(code, wparam, lparam);
    }

    if shortcut_modifier_down() {
        state::reset();
        return call_next(code, wparam, lparam);
    }

    if vk == VK_BACK.0 {
        let actions = state::backspace();
        if actions.is_empty() {
            return call_next(code, wparam, lparam);
        }
        if inject(&actions) {
            return LRESULT(1);
        }
        return call_next(code, wparam, lparam);
    }

    if vk == VK_ESCAPE.0 {
        let actions = state::restore_raw();
        if !actions.is_empty() && inject(&actions) {
            return LRESULT(1);
        }
        state::reset();
        return call_next(code, wparam, lparam);
    }

    if vk == VK_RETURN.0 || vk == VK_TAB.0 {
        let actions = if vk == VK_TAB.0 {
            state::commit_word()
        } else {
            state::commit_line()
        };
        if !actions.is_empty() {
            let _ = inject(&actions);
        }
        return call_next(code, wparam, lparam);
    }

    if is_navigation_key(vk) {
        state::reset();
        return call_next(code, wparam, lparam);
    }

    let ch = match translate_char(kb.vkCode) {
        Some(c) => c,
        None => return call_next(code, wparam, lparam),
    };

    if ch.is_control() {
        state::reset();
        return call_next(code, wparam, lparam);
    }

    let actions = state::process_char(ch);
    if actions.is_empty() {
        return call_next(code, wparam, lparam);
    }

    if inject(&actions) {
        LRESULT(1)
    } else {
        call_next(code, wparam, lparam)
    }
}

fn call_next(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

static TOGGLE_HELD: AtomicBool = AtomicBool::new(false);

static CHORD: Mutex<config::ChordWatch> = Mutex::new(config::ChordWatch::new());

/// The watch is only consulted for a modifier-only shortcut, so the common case — a
/// shortcut with a key — never takes this lock. The hook runs on every keystroke and is
/// watchdogged by `LowLevelHooksTimeout`.
fn modifier_only_target() -> Option<u8> {
    if state::shortcut_recording() {
        return None;
    }
    let sc = state::toggle_shortcut()?;
    sc.key.is_none().then(|| sc.modifier_mask())
}

fn chord_interrupted(vk: u16) {
    if modifier_only_target().is_none() {
        return;
    }
    if let Ok(mut watch) = CHORD.lock() {
        watch.interrupted(held_mask(vk, false));
    }
}

/// The modifier bit a virtual-key code carries, if it is a modifier at all.
fn modifier_bit(vk: u16) -> Option<u8> {
    Some(match vk {
        v if v == VK_CONTROL.0 || v == VK_LCONTROL.0 || v == VK_RCONTROL.0 => config::MOD_CTRL,
        v if v == VK_SHIFT.0 || v == VK_LSHIFT.0 || v == VK_RSHIFT.0 => config::MOD_SHIFT,
        v if v == VK_MENU.0 || v == VK_LMENU.0 || v == VK_RMENU.0 => config::MOD_ALT,
        v if v == VK_LWIN.0 || v == VK_RWIN.0 => config::MOD_CMD,
        _ => return None,
    })
}

fn held_mask(vk: u16, is_key_up: bool) -> u8 {
    let mut mask = (if down(VK_CONTROL) {
        config::MOD_CTRL
    } else {
        0
    }) | (if down(VK_SHIFT) { config::MOD_SHIFT } else { 0 })
        | (if down(VK_MENU) { config::MOD_ALT } else { 0 })
        | (if down(VK_LWIN) || down(VK_RWIN) {
            config::MOD_CMD
        } else {
            0
        });
    if let Some(bit) = modifier_bit(vk) {
        if is_key_up {
            mask &= !bit;
        } else {
            mask |= bit;
        }
    }
    mask
}

fn modifier_only_toggle_fired(vk: u16, is_key_up: bool) -> bool {
    if modifier_bit(vk).is_none() {
        return false;
    }
    let Some(target) = modifier_only_target() else {
        return false;
    };
    let Ok(mut watch) = CHORD.lock() else {
        return false;
    };
    watch.modifiers(held_mask(vk, is_key_up), target)
}

/// Mirrors `platform::macos::is_toggle_shortcut`: the combination has to match
/// exactly, so a shortcut with a modifier the user is not holding — or one extra
/// — is left for the focused application.
fn is_toggle_shortcut(vk: u16) -> bool {
    if state::shortcut_recording() {
        return false;
    }
    let Some(sc) = state::toggle_shortcut() else {
        return false;
    };
    if sc.key.and_then(config::windows_vk) != Some(vk) {
        return false;
    }
    down(VK_CONTROL) == sc.ctrl
        && down(VK_MENU) == sc.alt
        && down(VK_SHIFT) == sc.shift
        && (down(VK_LWIN) || down(VK_RWIN)) == sc.cmd
}

fn down(vk: VIRTUAL_KEY) -> bool {
    (unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000) != 0
}

fn shortcut_modifier_down() -> bool {
    [VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
        .into_iter()
        .any(down)
}

fn is_navigation_key(vk: u16) -> bool {
    const NAV: [u16; 10] = [
        VK_LEFT.0,
        VK_RIGHT.0,
        VK_UP.0,
        VK_DOWN.0,
        VK_HOME.0,
        VK_END.0,
        VK_PRIOR.0,
        VK_NEXT.0,
        VK_INSERT.0,
        VK_DELETE.0,
    ];
    NAV.contains(&vk) || (VK_F1.0..=VK_F12.0).contains(&vk)
}

unsafe fn translate_char(vk: u32) -> Option<char> {
    let mut keyboard_state = [0u8; 256];
    GetKeyboardState(&mut keyboard_state).ok()?;

    let scan = 0u32;
    let layout = GetKeyboardLayout(0);
    let mut buf = [0u16; 8];
    let n = ToUnicodeEx(vk, scan, &keyboard_state, &mut buf, 0, layout);
    if n <= 0 {
        return None;
    }
    String::from_utf16_lossy(&buf[..n as usize]).chars().next()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PasteKey {
    CtrlV,
    ShiftInsert,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum InjectMode {
    SendInput,
    Auto,
    Paste(PasteKey),
}

fn inject_mode() -> InjectMode {
    static MODE: OnceLock<InjectMode> = OnceLock::new();
    *MODE.get_or_init(|| {
        let raw = std::env::var("VNKEY_WIN_INJECT").unwrap_or_default();
        let mode = match raw.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => InjectMode::Auto,
            "sendinput" | "send_input" | "send-input" => InjectMode::SendInput,
            "paste" | "paste-ctrl-v" => InjectMode::Paste(PasteKey::CtrlV),
            "paste-shift-insert" => InjectMode::Paste(PasteKey::ShiftInsert),
            other => {
                eprintln!(
                    "[vnkey] unknown VNKEY_WIN_INJECT={other:?}; using auto \
                     (auto|sendinput|paste|paste-shift-insert)"
                );
                InjectMode::Auto
            }
        };
        if mode != InjectMode::Auto {
            eprintln!("[vnkey] text injection mode: {mode:?}");
        }
        mode
    })
}

static PASTE_LATCHED: AtomicBool = AtomicBool::new(false);

enum PasteStep {
    Backspace(u8),
    Text(String),
}

struct PasteJob {
    steps: Vec<PasteStep>,
    key: PasteKey,
}

fn inject(actions: &[EditAction]) -> bool {
    let mode = inject_mode();
    let key = match mode {
        InjectMode::Paste(key) => return queue_paste(actions, key),
        InjectMode::SendInput | InjectMode::Auto => PasteKey::CtrlV,
    };

    if PASTE_LATCHED.load(Ordering::Relaxed) {
        return queue_paste(actions, key);
    }

    let mut inputs: Vec<INPUT> = Vec::new();
    for action in actions {
        match action {
            EditAction::Backspace(count) => {
                for _ in 0..*count {
                    inputs.push(key_vk(VK_BACK.0, false));
                    inputs.push(key_vk(VK_BACK.0, true));
                }
            }
            EditAction::Insert(text) => {
                for unit in text.encode_utf16() {
                    inputs.push(key_unicode(unit, false));
                    inputs.push(key_unicode(unit, true));
                }
            }
        }
    }

    if inputs.is_empty() {
        return true;
    }

    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize;
    if sent == inputs.len() {
        return true;
    }

    if mode == InjectMode::SendInput {
        eprintln!(
            "[vnkey] SendInput delivered {sent}/{} events (blocked?); \
             set VNKEY_WIN_INJECT=paste to use the clipboard fallback",
            inputs.len()
        );
        return sent > 0;
    }

    if sent == 0 {
        eprintln!(
            "[vnkey] SendInput delivered 0/{} events; switching to clipboard-paste injection \
             for the rest of this session",
            inputs.len()
        );
        PASTE_LATCHED.store(true, Ordering::Relaxed);
        queue_paste(actions, key)
    } else {
        eprintln!(
            "[vnkey] SendInput delivered only {sent}/{} events; not retrying via paste \
             (would duplicate text)",
            inputs.len()
        );
        true
    }
}

fn queue_paste(actions: &[EditAction], key: PasteKey) -> bool {
    let steps: Vec<PasteStep> = actions
        .iter()
        .filter_map(|action| match action {
            EditAction::Backspace(0) => None,
            EditAction::Backspace(count) => Some(PasteStep::Backspace(*count)),
            EditAction::Insert(text) if text.is_empty() => None,
            EditAction::Insert(text) => Some(PasteStep::Text(text.clone())),
        })
        .collect();
    if steps.is_empty() {
        return true;
    }

    let queued = paste_worker().is_some_and(|tx| tx.send(PasteJob { steps, key }).is_ok());
    if !queued {
        eprintln!("[vnkey] paste worker unavailable; dropped one batch of Vietnamese text");
    }
    queued
}

fn paste_worker() -> Option<&'static Sender<PasteJob>> {
    static WORKER: OnceLock<Option<Sender<PasteJob>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<PasteJob>();
            let spawned = std::thread::Builder::new()
                .name("vnkey-paste".into())
                .spawn(move || {
                    for job in rx {
                        for step in &job.steps {
                            match step {
                                PasteStep::Backspace(count) => send_backspaces(*count),
                                PasteStep::Text(text) => paste_text(text, job.key),
                            }
                        }
                    }
                });
            match spawned {
                Ok(_) => Some(tx),
                Err(e) => {
                    eprintln!("[vnkey] could not start the paste worker: {e}");
                    None
                }
            }
        })
        .as_ref()
}

fn send_backspaces(count: u8) {
    let mut inputs = Vec::with_capacity(count as usize * 2);
    for _ in 0..count {
        inputs.push(key_vk(VK_BACK.0, false));
        inputs.push(key_vk(VK_BACK.0, true));
    }
    if !inputs.is_empty() {
        unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    }
}

fn paste_text(text: &str, key: PasteKey) {
    let saved = unsafe { clipboard_text() };

    if !unsafe { set_clipboard_text(text) } {
        eprintln!("[vnkey] could not put text on the clipboard; skipped pasting {text:?}");
        return;
    }

    send_paste_key(key);
    std::thread::sleep(PASTE_SETTLE);

    match saved {
        Some(previous) => {
            if !unsafe { set_clipboard_text(&previous) } {
                eprintln!("[vnkey] could not restore the previous clipboard text");
            }
        }
        None => eprintln!(
            "[vnkey] previous clipboard held no text; left the pasted text on the clipboard"
        ),
    }
}

fn send_paste_key(key: PasteKey) {
    let (modifier, main) = match key {
        PasteKey::CtrlV => (VK_CONTROL.0, VK_V.0),
        PasteKey::ShiftInsert => (VK_SHIFT.0, VK_INSERT.0),
    };
    let inputs = [
        key_vk(modifier, false),
        key_vk(main, false),
        key_vk(main, true),
        key_vk(modifier, true),
    ];
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

unsafe fn with_clipboard<T>(f: impl FnOnce() -> T) -> Option<T> {
    for _ in 0..CLIPBOARD_ATTEMPTS {
        if OpenClipboard(None).is_ok() {
            let out = f();
            let _ = CloseClipboard();
            return Some(out);
        }
        std::thread::sleep(CLIPBOARD_RETRY_DELAY);
    }
    eprintln!("[vnkey] clipboard busy: another app held it open");
    None
}

unsafe fn clipboard_text() -> Option<String> {
    with_clipboard(|| {
        if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            return None;
        }
        let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
        let hglobal = HGLOBAL(handle.0);
        let ptr = GlobalLock(hglobal) as *const u16;
        if ptr.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
        let _ = GlobalUnlock(hglobal);
        Some(text)
    })
    .flatten()
}

unsafe fn set_clipboard_text(text: &str) -> bool {
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    utf16.push(0);

    // SAFETY: on success the clipboard takes ownership of this block, so it must
    // be a fresh moveable global allocation and must not be freed here.
    let Ok(hglobal) = GlobalAlloc(GMEM_MOVEABLE, utf16.len() * size_of::<u16>()) else {
        return false;
    };
    let dst = GlobalLock(hglobal) as *mut u16;
    if dst.is_null() {
        GlobalFree(hglobal.0);
        return false;
    }
    std::ptr::copy_nonoverlapping(utf16.as_ptr(), dst, utf16.len());
    let _ = GlobalUnlock(hglobal);

    let took = with_clipboard(|| {
        if EmptyClipboard().is_err() {
            return false;
        }
        SetClipboardData(CF_UNICODETEXT, HANDLE(hglobal.0)).is_ok()
    })
    .unwrap_or(false);

    if !took {
        // SAFETY: ownership never transferred, so the block is still ours to release.
        GlobalFree(hglobal.0);
    }
    took
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalFree(hmem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

fn key_unicode(unit: u16, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: SYNTHETIC_MARKER,
            },
        },
    }
}

fn key_vk(vk: u16, key_up: bool) -> INPUT {
    let flags = if key_up {
        KEYEVENTF_KEYUP
    } else {
        Default::default()
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: SYNTHETIC_MARKER,
            },
        },
    }
}
