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
//! An update check runs in the background. When a newer release exists and
//! `auto_update` is on (the default), the app downloads it, replaces its own
//! bundle, relaunches, and tells the user on the way back up — see `installer`.
//! Outside a `.app` (a `cargo run` build) it only surfaces the menu item.
//!
//! One caveat is unavoidable today: macOS ties the Accessibility grant to the
//! code signature, and `package-app.sh` ad-hoc signs, so each build has a
//! different identity and the grant does **not** survive the swap. The update
//! still installs silently; the dialog afterwards says Accessibility must be
//! allowed again. A Developer ID signature is what makes it fully seamless, and
//! needs no change to this code — `installer::install` reports the live TCC
//! state rather than assuming it.

// No console window on Windows release builds.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod installer;
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
// Every variant is about updates; the shared prefix is the clearest naming here.
#[allow(clippy::enum_variant_names)]
enum UserEvent {
    /// A newer release is available (version string, no leading `v`).
    UpdateAvailable(String),
    /// A newer release was downloaded and installed; relaunch into it.
    UpdateInstalled(std::path::PathBuf),
    /// An automatic update failed (version, reason).
    UpdateFailed(String, String),
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
                announce_completed_update();
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
                // Only self-update from a real bundle, and only if the user has
                // not turned it off.
                if state::settings().auto_update && installer::app_bundle().is_some() {
                    spawn_update_install(proxy.clone(), version);
                }
            }
            Event::UserEvent(UserEvent::UpdateInstalled(app)) => {
                // The relaunched instance shows the dialog: by then the new
                // version is running and its real permission state is known.
                installer::relaunch_and_exit(&app);
            }
            Event::UserEvent(UserEvent::UpdateFailed(version, reason)) => {
                eprintln!("[vnkey] automatic update to {version} failed: {reason}");
                installer::announce_failure(&version, &reason);
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

/// Install `version` in the background, then ask the loop to relaunch.
fn spawn_update_install(proxy: EventLoopProxy<UserEvent>, version: String) {
    std::thread::spawn(move || {
        eprintln!("[vnkey] installing update {version}…");
        match installer::install(&version) {
            Ok(outcome) => {
                // `last_seen_version` already holds this build's version (written at
                // startup), which is exactly what the relaunched build compares
                // against to discover it was updated.
                if let Some(app) = installer::app_bundle() {
                    let _ = proxy.send_event(UserEvent::UpdateInstalled(app));
                }
                eprintln!(
                    "[vnkey] installed {} (accessibility kept: {})",
                    outcome.version, outcome.permission_kept
                );
            }
            Err(e) => {
                let _ = proxy.send_event(UserEvent::UpdateFailed(version, e));
            }
        }
    });
}

/// If the version on disk changed since the last run, we were self-updated —
/// tell the user, and note whether they must re-grant Accessibility.
fn announce_completed_update() {
    let previous = state::settings().last_seen_version;
    if let Some(prev) = previous.as_deref() {
        if prev != update::CURRENT {
            let needs_permission = !platform::permission_granted();
            installer::announce_update(prev, update::CURRENT, needs_permission);
        }
    }
    // Always record the running version, including on a first launch.
    if previous.as_deref() != Some(update::CURRENT) {
        state::update(|s| s.last_seen_version = Some(update::CURRENT.to_string()));
    }
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
