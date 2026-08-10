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

/// Our own application name, as the keyboard hook would report it.
pub fn self_app_name() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    if let Some(path) = exe.to_str() {
        for component in path.split('/') {
            if let Some(name) = component.strip_suffix(".app") {
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }
    exe.file_stem()?.to_str().map(str::to_string)
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

pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    macos::open_accessibility_settings();
}

/// The pointer, in the coordinate space the platform lays its displays out in.
pub fn pointer_position() -> Option<(f64, f64)> {
    #[cfg(target_os = "macos")]
    {
        macos::pointer_position()
    }
    #[cfg(target_os = "windows")]
    {
        windows::pointer_position()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
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
