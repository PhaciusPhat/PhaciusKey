//! Launch-at-login registration.
//!
//! macOS registers login items through `SMAppService` (ServiceManagement,
//! 13.0+), and that is what this uses — the same API `m_capture` settled on.
//!
//! It replaced a hand-written LaunchAgent plist in `~/Library/LaunchAgents`,
//! which registered the login item behind the system's back. Two things were
//! wrong with that:
//!
//! * macOS 13 routes every background item through Background Task
//!   Management. A plist dropped straight into the directory never passes
//!   through that registration, so the item lands unapproved and the system
//!   is free to ignore it.
//! * The agent pointed at `Contents/MacOS/vnkey`, so launchd started the bare
//!   executable instead of the `.app`. The Accessibility grant that lets the
//!   event tap exist is attached to the *bundle*, so the process macOS
//!   started at login did not carry it — the tap could not be created and
//!   Vietnamese typing was dead until the app was launched by hand.
//!
//! `SMAppService` registers the bundle itself, through the system, which is
//! what both problems were asking for.
//!
//! Errors are logged and swallowed: failing to register a login item must
//! never take the input method down with it.

/// What macOS currently reports about our login item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItem {
    /// Registered and allowed to launch.
    Enabled,
    /// Not registered.
    Disabled,
    /// Registered, but macOS is waiting for the user to allow it under
    /// System Settings → General → Login Items.
    NeedsApproval,
    /// The system cannot answer — not a bundle (a `cargo run` binary), or an
    /// OS without `SMAppService`.
    Unavailable,
}

/// Whether the login item should read as on, given what the user last asked
/// for.
///
/// The system is the source of truth whenever it has a definite answer, so
/// turning the item off in System Settings is reflected here rather than
/// fought. `NeedsApproval` and `Unavailable` are not answers about the user's
/// intent — the request stands, macOS is simply not acting on it yet — so the
/// stored preference carries.
fn effective_from(state: LoginItem, stored: bool) -> bool {
    match state {
        LoginItem::Enabled => true,
        LoginItem::Disabled => false,
        LoginItem::NeedsApproval | LoginItem::Unavailable => stored,
    }
}

/// [`effective_from`] against the live system state.
pub fn effective(stored: bool) -> bool {
    effective_from(state(), stored)
}

#[cfg(target_os = "macos")]
mod sys {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_void};
    use std::ptr;

    use super::LoginItem;

    /// Label of the LaunchAgent plist previous versions installed by hand.
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
        /// Never called through this declaration. The ABI passes a message's
        /// arguments exactly as the method's own signature would, so every
        /// call site transmutes this symbol to that signature first.
        fn objc_msgSend();
    }

    /// Transmute `objc_msgSend` to `$ty` — the signature of the message about
    /// to be sent. Wrong types here are wrong argument registers, so each use
    /// spells out the method it mirrors.
    macro_rules! msg_send_fn {
        ($ty:ty) => {
            std::mem::transmute::<*const (), $ty>(objc_msgSend as *const ())
        };
    }

    fn selector(name: &str) -> Sel {
        let name = CString::new(name).expect("selector name has no interior NUL");
        unsafe { sel_registerName(name.as_ptr()) }
    }

    /// `[SMAppService mainAppService]` — the login item standing for this
    /// bundle. `None` before macOS 13, where the class does not exist.
    ///
    /// The returned object is autoreleased. Every caller uses it and drops it
    /// within the call, so it is never held past the pool that owns it.
    fn main_app_service() -> Option<Id> {
        let name = CString::new("SMAppService").ok()?;
        let class = unsafe { objc_getClass(name.as_ptr()) };
        if class.is_null() {
            return None;
        }
        // + (SMAppService *)mainAppService
        let send = unsafe { msg_send_fn!(unsafe extern "C" fn(Id, Sel) -> Id) };
        let service = unsafe { send(class, selector("mainAppService")) };
        (!service.is_null()).then_some(service)
    }

    pub fn state() -> LoginItem {
        let Some(service) = main_app_service() else {
            return LoginItem::Unavailable;
        };
        // @property (readonly) SMAppServiceStatus status
        let send = unsafe { msg_send_fn!(unsafe extern "C" fn(Id, Sel) -> isize) };
        match unsafe { send(service, selector("status")) } {
            STATUS_ENABLED => LoginItem::Enabled,
            STATUS_NOT_REGISTERED => LoginItem::Disabled,
            STATUS_REQUIRES_APPROVAL => LoginItem::NeedsApproval,
            // SMAppServiceStatusNotFound: macOS cannot see a bundle to
            // register, which is the `cargo run` case.
            _ => LoginItem::Unavailable,
        }
    }

    pub fn apply(enabled: bool) {
        let Some(service) = main_app_service() else {
            eprintln!("[vnkey] no login item to configure — this build is not an .app bundle");
            return;
        };

        let method = if enabled {
            "registerAndReturnError:"
        } else {
            "unregisterAndReturnError:"
        };
        let mut error: Id = ptr::null_mut();
        // - (BOOL)registerAndReturnError:(NSError **)error
        // BOOL is a signed char on x86_64 and a _Bool on arm64; reading it as
        // i8 is right on both, where Rust's bool would be UB on the former.
        let send = unsafe { msg_send_fn!(unsafe extern "C" fn(Id, Sel, *mut Id) -> i8) };
        if unsafe { send(service, selector(method), &mut error) } != 0 {
            return;
        }

        // Registering something already registered is an error we asked for
        // by re-asserting; so is unregistering something absent. Neither is
        // worth reporting if the end state is the one we wanted.
        if super::effective_from(state(), !enabled) == enabled {
            return;
        }

        let verb = if enabled { "register" } else { "unregister" };
        eprintln!("[vnkey] could not {verb} the login item: {}", describe(error));
        if enabled && state() == LoginItem::NeedsApproval {
            eprintln!(
                "[vnkey] macOS is holding the login item for approval — allow PhaciusKey \
                 under System Settings → General → Login Items."
            );
        }
    }

    /// `[[error localizedDescription] UTF8String]`, for the log line.
    fn describe(error: Id) -> String {
        if error.is_null() {
            return "no error reported".to_string();
        }
        let send = unsafe { msg_send_fn!(unsafe extern "C" fn(Id, Sel) -> Id) };
        let description = unsafe { send(error, selector("localizedDescription")) };
        if description.is_null() {
            return "no description".to_string();
        }
        let utf8 = unsafe { msg_send_fn!(unsafe extern "C" fn(Id, Sel) -> *const c_char) };
        let text = unsafe { utf8(description, selector("UTF8String")) };
        if text.is_null() {
            return "no description".to_string();
        }
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    }

    /// Delete the LaunchAgent plist older versions wrote by hand. Returns
    /// whether one was there, so the caller can carry the user's intent over
    /// to `SMAppService` instead of silently dropping their login item.
    ///
    /// Removing the file is enough: launchd only reads this directory at
    /// login, so an agent already loaded this session stays up (which is just
    /// the running app) and is simply not there next time.
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

    /// No registration mechanism wired up on this OS yet (Windows would use
    /// the `Run` registry key once the Windows shell is verified on hardware).
    pub fn apply(_enabled: bool) {}

    pub fn state() -> LoginItem {
        LoginItem::Unavailable
    }

    pub fn migrate_legacy_launch_agent() -> bool {
        false
    }
}

/// Make the login-item registration match `enabled`.
pub fn apply(enabled: bool) {
    sys::apply(enabled);
}

/// What macOS currently reports about the login item.
pub fn state() -> LoginItem {
    sys::state()
}

/// Remove the LaunchAgent plist older versions installed, reporting whether
/// there was one.
pub fn migrate_legacy_launch_agent() -> bool {
    sys::migrate_legacy_launch_agent()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_wins_when_it_has_a_definite_answer() {
        // The user turning the login item off in System Settings has to stick,
        // rather than being overwritten by our stored copy on the next launch.
        assert!(effective_from(LoginItem::Enabled, false));
        assert!(!effective_from(LoginItem::Disabled, true));
    }

    #[test]
    fn a_request_awaiting_approval_keeps_the_users_intent() {
        // macOS reports "not enabled" until the user allows it, but they did
        // ask for it — reporting false here would flip the switch back under
        // them while the approval prompt is still on screen.
        assert!(effective_from(LoginItem::NeedsApproval, true));
        assert!(!effective_from(LoginItem::NeedsApproval, false));
    }

    /// Exercises the whole Objective-C path, which the compiler cannot check:
    /// a mistyped selector raises an unrecognized-selector exception and takes
    /// the process down, so merely getting an answer back proves the message
    /// sends are right. The test binary is not an `.app`, so macOS has no
    /// bundle to register and reports `NotFound`.
    #[cfg(target_os = "macos")]
    #[test]
    fn asking_the_system_from_an_unbundled_binary_reports_unavailable() {
        assert_eq!(state(), LoginItem::Unavailable);
    }

    #[test]
    fn a_build_the_system_cannot_register_keeps_the_users_intent() {
        // `cargo run` is not a bundle, so the switch would otherwise be stuck
        // off and unexplainable during development.
        assert!(effective_from(LoginItem::Unavailable, true));
        assert!(!effective_from(LoginItem::Unavailable, false));
    }
}
