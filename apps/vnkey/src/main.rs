//! vnkey — a cross-platform Vietnamese input method.
//!
//! Architecture: a shared, OS-independent engine (`vnkey-core`) driven by a thin
//! per-OS keyboard hook (`platform`), with a cross-platform tray menu (`tray`)
//! and TOML settings (`config`). The event loop (`tao`) runs the app as a
//! menu-bar / tray accessory with no Dock icon or window.
//!
//! ## Permissions & updates
//! The keyboard hook needs macOS Accessibility permission. Rather than force a
//! quit/relaunch after the user grants it, the loop **polls once a second and
//! installs the hook the moment permission appears**.
//!
//! An update *check* runs in the background and, if a newer release exists,
//! surfaces a "Download update" item. It never swaps the app in place: because
//! the builds are ad-hoc signed, every new version has a different code hash and
//! macOS would require re-granting Accessibility. Silent, permission-preserving
//! self-update needs a stable Developer ID signature — a deliberate follow-up.

// No console window on Windows release builds.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod platform;
mod png_write;
mod state;
mod tray;
mod update;

use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::{MenuEvent, MenuId};

use crate::config::Settings;
use crate::platform::{Hook, KeyboardHook};
use crate::tray::Tray;

/// Events posted to the main loop from background work.
enum UserEvent {
    /// A newer release is available (version string, no leading `v`).
    UpdateAvailable(String),
}

fn main() {
    // Hidden packaging-only path: `vnkey --export-iconset <dir>` dumps the app
    // icon's PNGs for `scripts/package-app.sh` to hand to `iconutil`. Never hit
    // during normal use.
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--export-iconset") {
        let dir = args.next().expect("usage: vnkey --export-iconset <dir>");
        tray::export_iconset(std::path::Path::new(&dir)).expect("failed to export iconset");
        return;
    }

    // Load persisted settings and initialize the shared engine state.
    let settings = Settings::load();
    state::init(settings);

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    // Menu-bar accessory: no Dock icon, no window.
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        let mut event_loop = event_loop;
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        run(event_loop);
    }

    #[cfg(not(target_os = "macos"))]
    run(event_loop);
}

fn run(event_loop: EventLoop<UserEvent>) -> ! {
    let proxy = event_loop.create_proxy();
    let menu_channel = MenuEvent::receiver();

    // Built after the loop starts (StartCause::Init), per tray-icon guidance.
    let mut tray: Option<Tray> = None;
    let mut hook: Option<Hook> = None;
    let mut permission_requested = false;

    event_loop.run(move |event, _target, control_flow| {
        match event {
            Event::NewEvents(StartCause::Init) => {
                let settings = state::settings();
                match Tray::new(&settings) {
                    Ok(t) => tray = Some(t),
                    Err(e) => eprintln!("[vnkey] failed to create tray: {e}"),
                }
                spawn_update_check(proxy.clone());
                // Prompt for Accessibility once; the poll below installs the hook
                // the moment it is granted — no relaunch needed.
                platform::request_permission();
                permission_requested = true;
            }
            Event::UserEvent(UserEvent::UpdateAvailable(version)) => {
                if let Some(tray) = &tray {
                    tray.set_update_available(&version);
                }
            }
            _ => {}
        }

        // Auto-retry: install the hook as soon as permission is present.
        if hook.is_none() && permission_requested && platform::permission_granted() {
            match Hook::install() {
                Ok(h) => {
                    hook = Some(h);
                    eprintln!("[vnkey] keyboard hook active");
                }
                Err(e) => eprintln!("[vnkey] {e}"),
            }
        }

        // Drain menu clicks.
        while let Ok(menu_event) = menu_channel.try_recv() {
            if let Some(tray) = &tray {
                handle_menu_event(tray, &menu_event.id, control_flow);
            }
        }

        // Keep polling every second until the hook is installed; then idle.
        // (Never override a pending exit.)
        if !matches!(*control_flow, ControlFlow::ExitWithCode(_)) {
            *control_flow = if hook.is_none() {
                ControlFlow::WaitUntil(Instant::now() + Duration::from_secs(1))
            } else {
                ControlFlow::Wait
            };
        }
    })
}

fn spawn_update_check(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        if let Some(version) = update::check_for_newer() {
            let _ = proxy.send_event(UserEvent::UpdateAvailable(version));
        }
    });
}

fn handle_menu_event(tray: &Tray, id: &MenuId, control_flow: &mut ControlFlow) {
    use config::{Method, Placement};

    if id == tray.quit.id() {
        *control_flow = ControlFlow::Exit;
        return;
    }
    if id == tray.update.id() {
        update::open_url(&update::releases_url());
        return;
    }
    if id == tray.report.id() {
        update::open_url(&update::new_issue_url());
        return;
    }

    let updated = if id == tray.toggle.id() {
        state::update(|s| s.enabled = !s.enabled)
    } else if id == tray.telex.id() {
        state::update(|s| s.method = Method::Telex)
    } else if id == tray.vni.id() {
        state::update(|s| s.method = Method::Vni)
    } else if id == tray.modern.id() {
        state::update(|s| s.placement = Placement::Modern)
    } else if id == tray.classic.id() {
        state::update(|s| s.placement = Placement::Classic)
    } else if id == tray.auto_restore.id() {
        state::update(|s| s.auto_restore = !s.auto_restore)
    } else {
        return;
    };

    // Reflect the change back into the menu checkmarks and tray glyph.
    tray.refresh(&updated);
}
