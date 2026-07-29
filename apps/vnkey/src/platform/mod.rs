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
