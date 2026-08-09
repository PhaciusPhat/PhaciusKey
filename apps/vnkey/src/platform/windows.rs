//! Windows keyboard hook + text injection.
//!
//! ⚠️ UNTESTED SCAFFOLD. This mirrors the macOS implementation's structure using
//! a `WH_KEYBOARD_LL` low-level hook to intercept/suppress keystrokes and
//! `SendInput` (with `KEYEVENTF_UNICODE`) to inject backspaces + Vietnamese
//! text. It compiles only under `cfg(windows)` and must be verified on a real
//! Windows machine before release — see the project TODO. Two pieces below are
//! reasoned from the documented API contracts but cannot be tested here:
//! modifier state inside a low-level hook (see [`shortcut_modifier_down`]) and
//! the ordering of injected text against a passed-through Enter/Tab (see
//! `hook_proc`).
//!
//! Design parity with macOS:
//! - Editing and navigation keys are dispatched by virtual-key code *before*
//!   the character path: Backspace pops the composition buffer, Esc restores
//!   the raw keystrokes, Enter/Tab commit the word, and shortcuts, arrows and
//!   function keys end it. Only what is left is translated to a character and
//!   fed to the engine — a control character reaching the buffer leaves the
//!   engine's idea of the screen wrong for the rest of the word, which corrupts
//!   every keystroke after it.
//! - The hook proc returns `1` (non-zero) to swallow the original key when the
//!   engine produces edit actions *and* those actions reached the app.
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
    GetAsyncKeyState, GetKeyboardLayout, GetKeyboardState, SendInput, ToUnicodeEx, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_BACK, VK_CONTROL, VK_DELETE,
    VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F12, VK_HOME, VK_INSERT, VK_LEFT, VK_LWIN, VK_MENU,
    VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_RWIN, VK_SHIFT, VK_TAB, VK_UP, VK_V,
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

    // `vkCode` is documented as 1–254, so it always fits a VIRTUAL_KEY.
    let vk = kb.vkCode as u16;

    if !state::vietnamese_active() {
        // Everything passes through untouched while typing is off, Backspace
        // included — the engine holds no composition.
        state::reset();
        return call_next(code, wparam, lparam);
    }

    // Keys pressed with Ctrl, Alt or Win are shortcuts (Ctrl+C, Win+E), never
    // Vietnamese input: a shortcut is a word boundary, and the key itself is
    // the app's. Shift is intentionally excluded so capitals still compose.
    if shortcut_modifier_down() {
        state::reset();
        return call_next(code, wparam, lparam);
    }

    // Backspace needs its own path: it must pop the last raw keystroke from the
    // composition buffer (which can stand for more than one on-screen
    // character, e.g. telex "as" → "á") and redraw. Translated as a character
    // it would push U+0008 into the buffer instead, which desynchronizes the
    // engine from the screen for the rest of the word.
    if vk == VK_BACK.0 {
        let actions = state::backspace();
        if actions.is_empty() {
            return call_next(code, wparam, lparam); // not composing — native Backspace
        }
        if inject(&actions) {
            return LRESULT(1); // we already redrew; swallow the original
        }
        // Injection did not reach the app: the native Backspace still deletes
        // one displayed character, which is exactly what the engine recorded.
        return call_next(code, wparam, lparam);
    }

    // Esc restores the raw keystrokes of the word being composed ("đấy" →
    // "ddaays") and is consumed by that; with nothing to restore it is the
    // app's Esc as usual (and a word boundary).
    if vk == VK_ESCAPE.0 {
        let actions = state::restore_raw();
        if !actions.is_empty() && inject(&actions) {
            return LRESULT(1);
        }
        state::reset();
        return call_next(code, wparam, lparam);
    }

    // Enter and Tab commit the word (so macros expand) but always pass through
    // — the app needs the key itself. Numpad Enter is VK_RETURN too; only the
    // extended-key flag distinguishes it, so it is covered here.
    //
    // Ordering is NOT guaranteed the way macOS's `CGEventTapPostEvent` is. The
    // raw input thread is blocked waiting for this hook proc to return, so
    // events handed to `SendInput` here cannot be processed before we return,
    // and the key we let through is dispatched as part of that return: the
    // expansion most likely lands *after* the Enter/Tab, not before it. On the
    // clipboard-paste path it certainly does — that job runs on another thread.
    // Verify on hardware. If it is wrong, the fix is to swallow the key and
    // append its own down/up to the tail of the same `SendInput` batch, since a
    // single `SendInput` call is not interleaved with other input.
    if vk == VK_RETURN.0 || vk == VK_TAB.0 {
        // Enter ends the line, so the next word opens a sentence; Tab commits
        // a word without ending anything.
        let actions = if vk == VK_TAB.0 { state::commit_word() } else { state::commit_line() };
        if !actions.is_empty() {
            let _ = inject(&actions);
        }
        return call_next(code, wparam, lparam);
    }

    // Arrows, Home/End, Page Up/Down, Insert, forward Delete and F1–F12 move
    // the caret or act on the app, so the engine's buffer no longer describes
    // the text at the cursor. Treat them as a composition boundary.
    if is_navigation_key(vk) {
        state::reset();
        return call_next(code, wparam, lparam);
    }

    let ch = match translate_char(kb.vkCode) {
        Some(c) => c,
        None => return call_next(code, wparam, lparam),
    };

    // Defense in depth for the control keys not named above (Pause, the media
    // keys on layouts that map them, anything a future Windows adds): none are
    // Vietnamese input, and feeding one to the engine is the corruption this
    // whole dispatch exists to prevent.
    if ch.is_control() {
        state::reset();
        return call_next(code, wparam, lparam);
    }

    let actions = state::process_char(ch);
    if actions.is_empty() {
        return call_next(code, wparam, lparam);
    }

    if inject(&actions) {
        LRESULT(1) // swallow the original keystroke
    } else {
        // Injection unavailable: pass the raw keystroke through rather than
        // swallowing it with no replacement — a plain letter beats a lost one.
        call_next(code, wparam, lparam)
    }
}

fn call_next(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Whether a shortcut modifier — Ctrl, Alt, or either Windows key — is held.
///
/// `GetAsyncKeyState` and not `GetKeyState`/`GetKeyboardState`: those report the
/// state derived from the *calling thread's* input queue, and a `WH_KEYBOARD_LL`
/// proc runs on the thread that installed the hook, which is not the thread
/// receiving the keystrokes. That queue never sees the modifier go down, so
/// `GetKeyState` would report Ctrl as up for the whole chord. `GetAsyncKeyState`
/// reads the global physical key state and is correct from any thread. The high
/// bit means "currently down"; the low bit (pressed since the last call) is
/// deliberately ignored.
///
/// Layouts that have AltGr report it as Ctrl+Alt, so AltGr combinations count as
/// shortcuts here. Telex and VNI never need AltGr, but on a US-International
/// layout this would block AltGr characters — worth checking on hardware.
fn shortcut_modifier_down() -> bool {
    [VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN].iter().any(|vk| {
        let state = unsafe { GetAsyncKeyState(vk.0 as i32) };
        (state as u16 & 0x8000) != 0
    })
}

/// Whether the key moves the caret or otherwise leaves the composition stale.
///
/// `VK_DELETE` here is *forward* delete, not Backspace (that is `VK_BACK`): it
/// removes text the engine is not tracking, so it ends the word rather than
/// editing the buffer.
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

/// Translate a virtual key code to the character it would produce, honoring the
/// current keyboard layout and modifier state.
///
/// `GetKeyboardState` is subject to the same hook-thread caveat described on
/// [`shortcut_modifier_down`], so Shift and Caps Lock may not be reflected here
/// and capitals may come back lowercase. Unverified, and left alone deliberately
/// — the async state has no Caps Lock *toggle* bit, so the fix is more than a
/// swapped call. Check this first on real hardware.
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
///
/// Returns `false` only when nothing was delivered and nothing is queued: the
/// caller must then pass the original keystroke through instead of swallowing
/// it, or the character is silently lost (the "sometimes the first letter of a
/// word vanishes" bug).
fn inject(actions: &[EditAction]) -> bool {
    let mode = inject_mode();
    let key = match mode {
        // Explicit paste mode: never try SendInput for the text.
        InjectMode::Paste(key) => return queue_paste(actions, key),
        InjectMode::SendInput | InjectMode::Auto => PasteKey::CtrlV,
    };

    // Already proved SendInput cannot deliver — stay on the paste path so jobs
    // keep their order behind the ones already queued.
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

    // Nothing to send: the engine asked for no visible edit (an empty insert
    // means "swallow the key, the screen is already right"), which the caller
    // must still honor by swallowing.
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
        // Nothing landed, so replaying the batch cannot duplicate anything.
        // Typically UIPI: the focused window outranks this process.
        eprintln!(
            "[vnkey] SendInput delivered 0/{} events; switching to clipboard-paste injection \
             for the rest of this session",
            inputs.len()
        );
        PASTE_LATCHED.store(true, Ordering::Relaxed);
        queue_paste(actions, key)
    } else {
        // Partial delivery: some characters already reached the app. Pasting the
        // batch again would duplicate them, so report and leave it alone — and
        // report success, because letting the original keystroke through would
        // add a raw character on top of a half-applied edit.
        eprintln!(
            "[vnkey] SendInput delivered only {sent}/{} events; not retrying via paste \
             (would duplicate text)",
            inputs.len()
        );
        true
    }
}

/// Hand the batch to the paste worker, preserving action order. Returns whether
/// the text is on its way — see [`inject`].
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
    // No visible edit to perform — see the same case in [`inject`].
    if steps.is_empty() {
        return true;
    }

    let sender = paste_worker();
    let queued = sender
        .lock()
        .map(|tx| tx.send(PasteJob { steps, key }).is_ok())
        .unwrap_or(false);
    if !queued {
        eprintln!("[vnkey] paste worker unavailable; dropped one batch of Vietnamese text");
    }
    queued
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
