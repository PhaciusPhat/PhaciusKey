#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Hook;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::Hook;

pub trait KeyboardHook: Sized {
    fn install() -> Result<Self, String>;
}

pub fn request_permission() {
    #[cfg(target_os = "macos")]
    macos::request_accessibility_permission(true);
}

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
