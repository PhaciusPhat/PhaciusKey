#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItem {
    Enabled,
    Disabled,
    NeedsApproval,
    Unavailable,
}

fn effective_from(state: LoginItem, stored: bool) -> bool {
    match state {
        LoginItem::Enabled => true,
        LoginItem::Disabled => false,
        LoginItem::NeedsApproval | LoginItem::Unavailable => stored,
    }
}

pub fn effective(stored: bool) -> bool {
    effective_from(state(), stored)
}

#[cfg(target_os = "macos")]
mod sys {
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_void};
    use std::ptr;

    use super::LoginItem;

    const LEGACY_LABEL: &str = "com.phacius.vnkey";

    // SMAppServiceStatus, from <ServiceManagement/SMAppService.h>.
    const STATUS_NOT_REGISTERED: isize = 0;
    const STATUS_ENABLED: isize = 1;
    const STATUS_REQUIRES_APPROVAL: isize = 2;

    type Id = *mut c_void;
    type Sel = *const c_void;

    #[link(name = "ServiceManagement", kind = "framework")]
    extern "C" {}

    extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Sel;
        fn objc_msgSend();
    }

    /// SAFETY: `$ty` must be the exact signature of the message being sent —
    /// a wrong type here means wrong argument registers.
    macro_rules! msg_send_fn {
        ($ty:ty) => {
            std::mem::transmute::<*const (), $ty>(objc_msgSend as *const ())
        };
    }

    fn selector(name: &CStr) -> Sel {
        unsafe { sel_registerName(name.as_ptr()) }
    }

    fn main_app_service() -> Option<Id> {
        let class = unsafe { objc_getClass(c"SMAppService".as_ptr()) };
        if class.is_null() {
            return None;
        }
        let send = unsafe { msg_send_fn!(unsafe extern "C" fn(Id, Sel) -> Id) };
        let service = unsafe { send(class, selector(c"mainAppService")) };
        (!service.is_null()).then_some(service)
    }

    pub fn state() -> LoginItem {
        let Some(service) = main_app_service() else {
            return LoginItem::Unavailable;
        };
        let send = unsafe { msg_send_fn!(unsafe extern "C" fn(Id, Sel) -> isize) };
        match unsafe { send(service, selector(c"status")) } {
            STATUS_ENABLED => LoginItem::Enabled,
            STATUS_NOT_REGISTERED => LoginItem::Disabled,
            STATUS_REQUIRES_APPROVAL => LoginItem::NeedsApproval,
            _ => LoginItem::Unavailable,
        }
    }

    pub fn apply(enabled: bool) {
        let Some(service) = main_app_service() else {
            eprintln!("[vnkey] no login item to configure — this build is not an .app bundle");
            return;
        };

        let method = if enabled {
            c"registerAndReturnError:"
        } else {
            c"unregisterAndReturnError:"
        };
        let mut error: Id = ptr::null_mut();
        // SAFETY: `BOOL` is a signed char on x86_64 and a `_Bool` on arm64;
        // reading it as i8 is right on both, where Rust's bool would be UB on
        // the former.
        let send = unsafe { msg_send_fn!(unsafe extern "C" fn(Id, Sel, *mut Id) -> i8) };
        if unsafe { send(service, selector(method), &mut error) } != 0 {
            return;
        }

        if super::effective_from(state(), !enabled) == enabled {
            return;
        }

        let verb = if enabled { "register" } else { "unregister" };
        eprintln!(
            "[vnkey] could not {verb} the login item: {}",
            describe(error)
        );
        if enabled && state() == LoginItem::NeedsApproval {
            eprintln!(
                "[vnkey] macOS is holding the login item for approval — allow PhaciusKey \
                 under System Settings → General → Login Items."
            );
        }
    }

    fn describe(error: Id) -> String {
        if error.is_null() {
            return "no error reported".to_string();
        }
        let send = unsafe { msg_send_fn!(unsafe extern "C" fn(Id, Sel) -> Id) };
        let description = unsafe { send(error, selector(c"localizedDescription")) };
        if description.is_null() {
            return "no description".to_string();
        }
        let utf8 = unsafe { msg_send_fn!(unsafe extern "C" fn(Id, Sel) -> *const c_char) };
        let text = unsafe { utf8(description, selector(c"UTF8String")) };
        if text.is_null() {
            return "no description".to_string();
        }
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn migrate_legacy_launch_agent() -> bool {
        let Some(home) = dirs::home_dir() else {
            return false;
        };
        let plist = home.join(format!("Library/LaunchAgents/{LEGACY_LABEL}.plist"));
        if !plist.exists() {
            return false;
        }
        match std::fs::remove_file(&plist) {
            Ok(()) => {
                eprintln!(
                    "[vnkey] removed the hand-written LaunchAgent at {}; the login item is \
                     now registered through macOS",
                    plist.display()
                );
                true
            }
            Err(e) => {
                eprintln!("[vnkey] could not remove {}: {e}", plist.display());
                false
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    use super::LoginItem;

    pub fn apply(_enabled: bool) {}

    pub fn state() -> LoginItem {
        LoginItem::Unavailable
    }

    pub fn migrate_legacy_launch_agent() -> bool {
        false
    }
}

pub fn apply(enabled: bool) {
    sys::apply(enabled);
}

pub fn state() -> LoginItem {
    sys::state()
}

pub fn migrate_legacy_launch_agent() -> bool {
    sys::migrate_legacy_launch_agent()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_wins_when_it_has_a_definite_answer() {
        assert!(effective_from(LoginItem::Enabled, false));
        assert!(!effective_from(LoginItem::Disabled, true));
    }

    #[test]
    fn a_request_awaiting_approval_keeps_the_users_intent() {
        assert!(effective_from(LoginItem::NeedsApproval, true));
        assert!(!effective_from(LoginItem::NeedsApproval, false));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn asking_the_system_from_an_unbundled_binary_reports_unavailable() {
        assert_eq!(state(), LoginItem::Unavailable);
    }

    #[test]
    fn a_build_the_system_cannot_register_keeps_the_users_intent() {
        assert!(effective_from(LoginItem::Unavailable, true));
        assert!(!effective_from(LoginItem::Unavailable, false));
    }
}
