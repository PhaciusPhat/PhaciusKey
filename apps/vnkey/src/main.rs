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
//! An update check runs in the background — once at launch, then daily for as
//! long as the process lives (a menu-bar agent is rarely quit, so a launch-only
//! check would fire once per login, not once a day). A *failed* check retries
//! after 15 minutes instead of sitting out the rest of the day: the launch check
//! races Wi-Fi/VPN coming up at login, and long sessions hit transient failures
//! (network flaps, GitHub's per-IP rate limit) too. When a newer release exists
//! and `auto_update` is on (the default), the app downloads it, replaces its own
//! bundle, relaunches, and tells the user on the way back up — see `installer`.
//! Outside a `.app` (a `cargo run` build) it only surfaces the menu item.
//!
//! macOS ties the Accessibility grant to the code-signing identity. Published
//! releases are all signed with the shared `phaciuskey-release` certificate
//! (see CONTRIBUTING.md → Releasing), so the grant survives the swap. Ad-hoc
//! dev builds each carry a fresh identity — after updating one of those, the
//! relaunch dialog says Accessibility must be allowed again.
//! `installer::install` reports the live TCC state rather than assuming either
//! way.

// No console window on Windows release builds.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod autostart;
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
enum UserEvent {
    /// A newer release is available (version string, no leading `v`).
    UpdateAvailable(String),
    /// A newer release was downloaded and installed; relaunch into it.
    UpdateInstalled(std::path::PathBuf),
    /// An automatic update failed (version, reason).
    UpdateFailed(String, String),
    /// A user-requested "check now" finished (newer version / up to date / error).
    UpdateCheckDone(Result<Option<String>, String>),
    /// Settings or the focused app changed off the main thread (toggle
    /// shortcut, app switch) — re-sync the tray.
    StateChanged,
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
    // Re-assert the login item every launch: it self-heals a moved bundle, and
    // removes the agent if the user turned the setting off by editing the file.
    autostart::apply(settings.start_at_login);
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

    // Wake the loop when a background thread (the keyboard hook) changes state,
    // so the tray reflects a shortcut-toggle or app switch immediately. The
    // proxy goes behind a Mutex only to make the callback Sync.
    let tray_sync = std::sync::Mutex::new(event_loop.create_proxy());
    state::set_on_change(Box::new(move || {
        if let Ok(proxy) = tray_sync.lock() {
            let _ = proxy.send_event(UserEvent::StateChanged);
        }
    }));

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
                    if let Some(tray) = &tray {
                        tray.set_update_installing(&version);
                    }
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
                // Re-arm the menu item so the user can click to retry.
                if let Some(tray) = &tray {
                    tray.set_update_available(&version);
                }
                installer::announce_failure(&version, &reason);
            }
            Event::UserEvent(UserEvent::UpdateCheckDone(result)) => {
                if let Some(tray) = &tray {
                    match result {
                        Ok(Some(version)) => {
                            // The user asked for an update, not a notification:
                            // go straight into the install.
                            tray.set_update_available(&version);
                            start_install(tray, &proxy, version);
                        }
                        Ok(None) => {
                            tray.set_update_idle();
                            installer::announce(&format!(
                                "PhaciusKey {} is up to date.",
                                update::CURRENT
                            ));
                        }
                        Err(e) => {
                            tray.set_update_idle();
                            installer::announce(&format!("Could not check for updates.\n\n{e}"));
                        }
                    }
                }
            }
            Event::UserEvent(UserEvent::StateChanged) => {
                if let Some(tray) = &tray {
                    tray.refresh(&state::settings(), state::current_app().as_deref());
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
                handle_menu_event(tray, &proxy, &menu_event.id, control_flow);
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

/// Check now, then once a day for the life of the process; a failed check is
/// retried after 15 minutes (see the module docs for why both matter).
fn spawn_update_check(proxy: EventLoopProxy<UserEvent>) {
    const DAY: Duration = Duration::from_secs(24 * 60 * 60);
    const RETRY: Duration = Duration::from_secs(15 * 60);
    std::thread::spawn(move || loop {
        let wait = match update::check_for_newer() {
            Ok(Some(version)) => {
                let _ = proxy.send_event(UserEvent::UpdateAvailable(version));
                DAY
            }
            Ok(None) => DAY,
            Err(e) => {
                eprintln!("[vnkey] update check failed (retrying in 15 min): {e}");
                RETRY
            }
        };
        std::thread::sleep(wait);
    });
}

/// Install `version` in the background, then ask the loop to relaunch.
///
/// The daily check re-announces an update whose install failed the day before;
/// the guard makes a second announcement arriving while an install is still in
/// flight a no-op rather than a concurrent download of the same DMG.
fn spawn_update_install(proxy: EventLoopProxy<UserEvent>, version: String) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static INSTALLING: AtomicBool = AtomicBool::new(false);
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        // Re-arm on every exit path; on success the process relaunches anyway.
        struct Rearm;
        impl Drop for Rearm {
            fn drop(&mut self) {
                INSTALLING.store(false, Ordering::SeqCst);
            }
        }
        let _rearm = Rearm;
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

/// Kick off an immediate install of `version`, reflecting it in the menu item.
/// Outside a `.app` bundle self-update is impossible, so fall back to opening
/// the releases page for a manual download.
fn start_install(tray: &Tray, proxy: &EventLoopProxy<UserEvent>, version: String) {
    if installer::app_bundle().is_some() {
        tray.set_update_installing(&version);
        spawn_update_install(proxy.clone(), version);
    } else {
        update::open_url(&update::releases_url());
    }
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

fn handle_menu_event(
    tray: &Tray,
    proxy: &EventLoopProxy<UserEvent>,
    id: &MenuId,
    control_flow: &mut ControlFlow,
) {
    use config::{Method, Placement};

    if id == tray.quit.id() {
        *control_flow = ControlFlow::Exit;
        return;
    }
    if id == tray.update.id() {
        // Update *now*: install the known release directly, or check first and
        // install whatever the check finds. Opening the releases page (the old
        // behavior) is now only the fallback for builds that can't self-update.
        match tray.available_version() {
            Some(version) => start_install(tray, proxy, version),
            None => {
                tray.set_update_checking();
                let proxy = proxy.clone();
                std::thread::spawn(move || {
                    let _ = proxy.send_event(UserEvent::UpdateCheckDone(update::check_for_newer()));
                });
            }
        }
        return;
    }
    if id == tray.report.id() {
        update::open_url(&update::new_issue_url());
        return;
    }

    let updated = if id == tray.toggle.id() {
        state::update(|s| s.enabled = !s.enabled)
    } else if id == tray.app_toggle.id() {
        // Flip the focused app in/out of the disabled list.
        let Some(app) = state::current_app() else { return };
        state::update(|s| {
            if s.disabled_for(Some(&app)) {
                s.disabled_apps.retain(|d| !d.eq_ignore_ascii_case(&app));
            } else {
                s.disabled_apps.push(app.clone());
            }
        })
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
    } else if id == tray.start_login.id() {
        let updated = state::update(|s| s.start_at_login = !s.start_at_login);
        autostart::apply(updated.start_at_login);
        updated
    } else {
        return;
    };

    // Reflect the change back into the menu checkmarks and tray glyph.
    tray.refresh(&updated, state::current_app().as_deref());
}
