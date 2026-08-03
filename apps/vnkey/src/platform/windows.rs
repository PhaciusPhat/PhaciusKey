//! Windows keyboard hook + text injection.
//!
//! ⚠️ UNTESTED SCAFFOLD. This mirrors the macOS implementation's structure using
//! a `WH_KEYBOARD_LL` low-level hook to intercept/suppress keystrokes and
//! `SendInput` (with `KEYEVENTF_UNICODE`) to inject backspaces + Vietnamese
//! text. It compiles only under `cfg(windows)` and must be verified on a real
//! Windows machine before release — see the project TODO.
//!
//! Design parity with macOS:
//! - The hook proc translates the virtual key to a character, routes it through
//!   [`crate::state`], and returns `1` (non-zero) to swallow the original key
//!   when the engine produces edit actions.
//! - Injected events carry a sentinel in `dwExtraInfo` so the hook ignores our
//!   own synthesized keystrokes (the same trick as macOS's `eventSourceUserData`).
//!
//! # Text injection: `SendInput`, with a clipboard-paste fallback
//!
//! `SendInput` with `KEYEVENTF_UNICODE` is the primary path and is what every
//! well-behaved app accepts. Two classes of app defeat it:
//!
//! 1. **Higher-integrity windows.** UIPI silently discards input synthesized by
//!    a lower-integrity process, so an unelevated PhaciusKey cannot type into an
//!    elevated app. `SendInput` reports this by returning a short count.
//! 2. **Apps that mishandle synthetic Unicode keystrokes** — historically some
//!    browser address bars, search fields and games. These are *not* detectable:
//!    `SendInput` reports full success and the characters are dropped or
//!    reordered downstream.
//!
//! The fallback puts the text on the clipboard and sends `Ctrl+V` (or
//! `Shift+Insert`), which travels as a normal command rather than as synthetic
//! character input. The previous clipboard text is saved and restored.
//!
//! Because case 2 is undetectable, the fallback is reachable two ways, via
//! `VNKEY_WIN_INJECT`:
//!
//! | Value                 | Behaviour                                          |
//! |-----------------------|----------------------------------------------------|
//! | unset / `auto`        | `SendInput`; latch to paste if it delivers nothing  |
//! | `sendinput`           | `SendInput` only; never touch the clipboard         |
//! | `paste`               | Always clipboard + `Ctrl+V`                         |
//! | `paste-shift-insert`  | Always clipboard + `Shift+Insert`                    |
//!
//! In `auto`, a *zero*-delivery `SendInput` latches paste mode on for the rest
//! of the session and the batch is retried. A **partial** delivery is reported
//! but never retried — some characters already landed, and re-sending them via
//! paste would duplicate text.
//!
//! ## Why the paste runs on a worker thread
//!
//! A `WH_KEYBOARD_LL` hook proc that overruns `LowLevelHooksTimeout` (300 ms by
//! default) is silently removed by Windows. Opening the clipboard can block on
//! another process, and the clipboard must stay ours until the target app has
//! read it — so paste jobs go to a single worker thread over a channel, which
//! keeps them ordered and keeps `hook_proc` prompt. Once paste mode latches,
//! every batch takes that path, so ordering stays consistent.
//!
//! ## Known limitation of the paste path (verify on hardware)
//!
//! Paste jobs are ordered relative to *each other*, but not relative to keys the
//! engine passes straight through. A pass-through key returns via
//! `CallNextHookEx` and reaches the app immediately, while a queued paste lands
//! a moment later — so fast typing across a paste boundary can interleave. This
//! is inherent to injecting text asynchronously, and is likely why EVKey's own
//! `_globaldef.h` defaults to `PUSH_BY_MESSAGE` and keeps the clipboard path as
//! a fallback rather than the default. Fixing it properly means queueing *all*
//! keystrokes behind the injector, which is a larger change to the hook design.
//! Prefer `auto` (the default) so the clipboard is only used when `SendInput`
//! provably cannot deliver.

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
    GetKeyboardLayout, GetKeyboardState, SendInput, ToUnicodeEx, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_BACK, VK_CONTROL, VK_INSERT, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN,
};

use vnkey_core::EditAction;

use super::KeyboardHook;
use crate::state;

/// Sentinel stored in `dwExtraInfo` of injected events so the hook ignores
/// keystrokes we synthesized ourselves. ("VNKEY" in ASCII.)
const SYNTHETIC_MARKER: usize = 0x0056_4E4B_4559;

/// `CF_UNICODETEXT`. Declared here rather than pulling in the `windows` crate's
/// heavyweight `Win32_System_Ole` feature for a single integer.
const CF_UNICODETEXT: u32 = 13;

/// How long to leave our text on the clipboard before restoring the previous
/// contents. The paste is asynchronous: the target app reads the clipboard when
/// it processes the `Ctrl+V` message, so restoring immediately would race it.
const PASTE_SETTLE: Duration = Duration::from_millis(120);

/// `OpenClipboard` fails while another process holds the clipboard.
const CLIPBOARD_ATTEMPTS: u32 = 12;
const CLIPBOARD_RETRY_DELAY: Duration = Duration::from_millis(10);

thread_local! {
    static HOOK_HANDLE: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// Owns the installed hook; unhooks on drop.
pub struct Hook {
    handle: HHOOK,
}

impl KeyboardHook for Hook {
    fn install() -> Result<Self, String> {
        // A NULL module handle is valid for WH_KEYBOARD_LL.
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
    if msg != WM_KEYDOWN && msg != WM_SYSKEYDOWN {
        return call_next(code, wparam, lparam);
    }

    let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);

    // Ignore keystrokes we injected ourselves.
    if kb.dwExtraInfo == SYNTHETIC_MARKER {
        return call_next(code, wparam, lparam);
    }

    let ch = match translate_char(kb.vkCode) {
        Some(c) => c,
        None => return call_next(code, wparam, lparam),
    };

    let actions = state::process_char(ch);
    if actions.is_empty() {
        return call_next(code, wparam, lparam);
    }

    inject(&actions);
    LRESULT(1) // swallow the original keystroke
}

fn call_next(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Translate a virtual key code to the character it would produce, honoring the
/// current keyboard layout and modifier state.
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

/// Which key combination pastes in the target app.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PasteKey {
    CtrlV,
    ShiftInsert,
}

/// Configured injection strategy — see the module docs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum InjectMode {
    /// `SendInput` only.
    SendInput,
    /// `SendInput`, latching to paste if it delivers nothing.
    Auto,
    /// Always clipboard + paste.
    Paste(PasteKey),
}

/// Read `VNKEY_WIN_INJECT` once.
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

/// Set once `SendInput` has proved it cannot deliver, so later batches skip
/// straight to the paste path and stay in order behind it.
static PASTE_LATCHED: AtomicBool = AtomicBool::new(false);

/// One ordered unit of work for the paste worker.
enum PasteStep {
    Backspace(u8),
    Text(String),
}

struct PasteJob {
    steps: Vec<PasteStep>,
    key: PasteKey,
}

/// Inject the edit actions, preferring `SendInput` and falling back to
/// clipboard + paste as configured. See the module docs.
fn inject(actions: &[EditAction]) {
    let mode = inject_mode();
    let key = match mode {
        // Explicit paste mode: never try SendInput for the text.
        InjectMode::Paste(key) => {
            queue_paste(actions, key);
            return;
        }
        InjectMode::SendInput | InjectMode::Auto => PasteKey::CtrlV,
    };

    // Already proved SendInput cannot deliver — stay on the paste path so jobs
    // keep their order behind the ones already queued.
    if PASTE_LATCHED.load(Ordering::Relaxed) {
        queue_paste(actions, key);
        return;
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
        return;
    }

    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize;
    if sent == inputs.len() {
        return;
    }

    if mode == InjectMode::SendInput {
        eprintln!(
            "[vnkey] SendInput delivered {sent}/{} events (blocked?); \
             set VNKEY_WIN_INJECT=paste to use the clipboard fallback",
            inputs.len()
        );
        return;
    }

    if sent == 0 {
        // Nothing landed, so replaying the batch cannot duplicate anything.
        // Typically UIPI: the focused window outranks this process.
        eprintln!(
            "[vnkey] SendInput delivered 0/{} events; switching to clipboard-paste injection \
             for the rest of this session",
            inputs.len()
        );
        PASTE_LATCHED.store(true, Ordering::Relaxed);
        queue_paste(actions, key);
    } else {
        // Partial delivery: some characters already reached the app. Pasting the
        // batch again would duplicate them, so report and leave it alone.
        eprintln!(
            "[vnkey] SendInput delivered only {sent}/{} events; not retrying via paste \
             (would duplicate text)",
            inputs.len()
        );
    }
}

/// Hand the batch to the paste worker, preserving action order.
fn queue_paste(actions: &[EditAction], key: PasteKey) {
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
        return;
    }

    let sender = paste_worker();
    let queued = sender
        .lock()
        .map(|tx| tx.send(PasteJob { steps, key }).is_ok())
        .unwrap_or(false);
    if !queued {
        eprintln!("[vnkey] paste worker unavailable; dropped one batch of Vietnamese text");
    }
}

/// The single worker thread that performs clipboard pastes, started on first use.
///
/// Keeping this off the hook thread is required: see the module docs on
/// `LowLevelHooksTimeout`.
fn paste_worker() -> &'static Mutex<Sender<PasteJob>> {
    static WORKER: OnceLock<Mutex<Sender<PasteJob>>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<PasteJob>();
        std::thread::Builder::new()
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
            })
            .expect("spawn vnkey-paste thread");
        Mutex::new(tx)
    })
}

/// Delete `count` characters with real `VK_BACK` keystrokes. Backspaces cannot
/// be pasted, so this stays on `SendInput` even in paste mode.
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

/// Put `text` on the clipboard, paste it, then restore the previous clipboard
/// text. Runs only on the paste worker thread.
fn paste_text(text: &str, key: PasteKey) {
    let saved = unsafe { clipboard_text() };

    if !unsafe { set_clipboard_text(text) } {
        eprintln!("[vnkey] could not put text on the clipboard; skipped pasting {text:?}");
        return;
    }

    send_paste_key(key);
    std::thread::sleep(PASTE_SETTLE);

    // Restore whatever the user had. If the clipboard held something we cannot
    // represent as text (an image, say), we can only leave our text behind.
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

/// Send the paste chord, tagged so our own hook ignores it.
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

/// Run `f` with the clipboard open, retrying while another process holds it.
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

/// Current clipboard contents as text, if it holds any.
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

/// Replace the clipboard with `text`. Returns whether it took.
unsafe fn set_clipboard_text(text: &str) -> bool {
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    utf16.push(0);

    // The clipboard takes ownership of this block on success, so it must be a
    // fresh moveable global allocation and must not be freed here.
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
        // Ownership never transferred, so the block is still ours to release.
        GlobalFree(hglobal.0);
    }
    took
}

// `GlobalFree` has no binding in the `windows` crate's `Win32_System_Memory`
// feature, so link it directly. Only the failure paths above need it — on
// success the clipboard owns the block and freeing it would be a double free.
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
    let flags = if key_up { KEYEVENTF_KEYUP } else { Default::default() };
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
