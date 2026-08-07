//! OS-specific keyboard interception + text injection.
//!
//! Everything else in the app is cross-platform. Only these two jobs differ per
//! OS, so they live behind one trait with a per-OS implementation:
//!
//! | Job                         | macOS                | Windows              |
//! |-----------------------------|----------------------|----------------------|
//! | intercept + swallow a key   | `CGEventTap`         | `WH_KEYBOARD_LL`     |
//! | inject backspaces + text    | `CGEvent` unicode    | `SendInput` unicode  |
//!
//! Each implementation routes captured characters through [`crate::state`] and
//! executes the returned [`vnkey_core::EditAction`]s.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Hook;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::Hook;

/// A platform keyboard hook.
///
/// `install` must be called on the main thread; the implementation registers
/// itself with the current run loop / message loop, which the caller then runs.
/// The returned value owns the OS resources and must be kept alive for the hook
/// to stay active. `install` does not prompt for permission — call
/// [`request_permission`] once first, then retry `install` once
/// [`permission_granted`] returns true.
pub trait KeyboardHook: Sized {
    fn install() -> Result<Self, String>;
}

/// Show the OS permission prompt required to intercept keystrokes (macOS
/// Accessibility). No-op where no permission is needed. Call once at startup.
pub fn request_permission() {
    #[cfg(target_os = "macos")]
    macos::request_accessibility_permission(true);
}

/// Names of the applications installed on this machine, sorted and deduped
/// case-insensitively. Feeds the settings window's app search, so the user can
/// configure any installed app — not only the ones that already ran this
/// session. Empty where no lookup is implemented.
pub fn installed_apps() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        macos::installed_apps()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// Whether some process currently holds Secure Event Input (a focused
/// password field, some terminals). While it does, no event tap sees
/// keystrokes, so Vietnamese typing silently runs raw — the tray surfaces
/// this instead of leaving the user to wonder. Always false where the
/// concept doesn't exist.
pub fn secure_input_active() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::secure_input_active()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Whether the process currently holds the permission needed to intercept
/// keystrokes. Never prompts — safe to poll. Always true where none is needed.
pub fn permission_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::request_accessibility_permission(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}
