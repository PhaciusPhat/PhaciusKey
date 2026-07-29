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

use std::mem::size_of;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardState, SendInput, ToUnicodeEx, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_BACK,
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

/// Inject the edit actions via `SendInput`, each tagged with the sentinel.
fn inject(actions: &[EditAction]) {
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

    if !inputs.is_empty() {
        unsafe {
            SendInput(&inputs, size_of::<INPUT>() as i32);
        }
    }
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
