#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod autostart;
mod config;
mod installer;
mod platform;
mod png_write;
mod settings_window;
mod state;
mod tray;
mod update;

use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::{MenuEvent, MenuId};

use crate::config::Settings;
use crate::platform::{Hook, KeyboardHook};
use crate::settings_window::SettingsWindow;
use crate::tray::Tray;

enum UserEvent {
    UpdateAvailable(String),
    UpdateInstalled(std::path::PathBuf),
    UpdateFailed(String, String),
    UpdateCheckDone(Result<Option<String>, String>),
    StateChanged,
    Ipc(String),
}

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--export-iconset") {
        let Some(dir) = args.next() else {
            eprintln!("usage: vnkey --export-iconset <dir>");
            std::process::exit(2);
        };
        if let Err(e) = tray::export_iconset(std::path::Path::new(&dir)) {
            eprintln!("[vnkey] failed to export iconset: {e}");
            std::process::exit(1);
        }
        return;
    }

    let mut settings = Settings::load(platform::self_app_name().as_deref());

    if autostart::migrate_legacy_launch_agent() && settings.start_at_login {
        autostart::apply(true);
    }

    let registered = autostart::effective(settings.start_at_login);
    if registered != settings.start_at_login {
        settings.start_at_login = registered;
        settings.save();
    }

    state::init(settings);

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

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

    let tray_sync = std::sync::Mutex::new(event_loop.create_proxy());
    state::set_on_change(Box::new(move || {
        if let Ok(proxy) = tray_sync.lock() {
            let _ = proxy.send_event(UserEvent::StateChanged);
        }
    }));

    let mut tray: Option<Tray> = None;
    let mut hook: Option<Hook> = None;
    let mut permission_requested = false;
    let mut settings_win: Option<SettingsWindow> = None;

    event_loop.run(move |event, target, control_flow| {
        match event {
            Event::NewEvents(StartCause::Init) => {
                let settings = state::settings();
                match Tray::new(&settings) {
                    Ok(t) => tray = Some(t),
                    Err(e) => eprintln!("[vnkey] failed to create tray: {e}"),
                }
                announce_completed_update();
                spawn_update_check(proxy.clone());
                spawn_secure_input_watch(proxy.clone());
                if std::env::args().any(|a| a == "--settings-window") {
                    match SettingsWindow::new(target, proxy.clone()) {
                        Ok(win) => {
                            win.show();
                            settings_win = Some(win);
                        }
                        Err(e) => eprintln!("[vnkey] failed to open settings: {e}"),
                    }
                }
                platform::request_permission();
                permission_requested = true;
            }
            Event::UserEvent(UserEvent::UpdateAvailable(version)) => {
                if let Some(tray) = &tray {
                    tray.set_update_available(&version);
                }
                if state::settings().auto_update && installer::app_bundle().is_some() {
                    if let Some(tray) = &tray {
                        tray.set_update_installing(&version);
                    }
                    spawn_update_install(proxy.clone(), version);
                }
            }
            Event::UserEvent(UserEvent::UpdateInstalled(app)) => {
                installer::relaunch_and_exit(&app);
            }
            Event::UserEvent(UserEvent::UpdateFailed(version, reason)) => {
                eprintln!("[vnkey] automatic update to {version} failed: {reason}");
                if let Some(tray) = &tray {
                    tray.set_update_available(&version);
                }
                installer::announce_failure(&version, &reason);
            }
            Event::UserEvent(UserEvent::UpdateCheckDone(result)) => {
                if let Some(tray) = &tray {
                    match result {
                        Ok(Some(version)) => {
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
                if let Some(win) = &settings_win {
                    win.push_state();
                }
            }
            Event::UserEvent(UserEvent::Ipc(msg)) => {
                settings_window::apply_ipc(&msg);
                if let Some(tray) = &tray {
                    tray.refresh(&state::settings(), state::current_app().as_deref());
                }
                if let Some(win) = &settings_win {
                    win.push_state();
                }
            }
            Event::WindowEvent {
                event: tao::event::WindowEvent::CloseRequested,
                window_id,
                ..
            } => {
                if let Some(win) = &settings_win {
                    if win.window_id() == window_id {
                        win.hide();
                    }
                }
            }
            _ => {}
        }

        if hook.is_none() && permission_requested && platform::permission_granted() {
            match Hook::install() {
                Ok(h) => {
                    hook = Some(h);
                    eprintln!("[vnkey] keyboard hook active");
                }
                Err(e) => eprintln!("[vnkey] {e}"),
            }
        }

        while let Ok(menu_event) = menu_channel.try_recv() {
            if let Some(tray) = &tray {
                if menu_event.id == tray.settings.id() {
                    match &settings_win {
                        Some(win) => win.show(),
                        None => match SettingsWindow::new(target, proxy.clone()) {
                            Ok(win) => {
                                win.show();
                                settings_win = Some(win);
                            }
                            Err(e) => eprintln!("[vnkey] failed to open settings: {e}"),
                        },
                    }
                    continue;
                }
                handle_menu_event(tray, &proxy, &menu_event.id, control_flow);
                if let Some(win) = &settings_win {
                    win.push_state();
                }
            }
        }

        if !matches!(*control_flow, ControlFlow::ExitWithCode(_)) {
            *control_flow = if hook.is_none() {
                ControlFlow::WaitUntil(Instant::now() + Duration::from_secs(1))
            } else {
                ControlFlow::Wait
            };
        }
    })
}

fn spawn_secure_input_watch(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let mut was = platform::secure_input_active();
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let now = platform::secure_input_active();
            if now != was {
                was = now;
                let _ = proxy.send_event(UserEvent::StateChanged);
            }
        }
    });
}

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

fn spawn_update_install(proxy: EventLoopProxy<UserEvent>, version: String) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static INSTALLING: AtomicBool = AtomicBool::new(false);
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
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

fn start_install(tray: &Tray, proxy: &EventLoopProxy<UserEvent>, version: String) {
    if installer::app_bundle().is_some() {
        tray.set_update_installing(&version);
        spawn_update_install(proxy.clone(), version);
    } else {
        update::open_url(&update::releases_url());
    }
}

fn announce_completed_update() {
    let previous = state::settings().last_seen_version;
    if let Some(prev) = previous.as_deref() {
        if prev != update::CURRENT {
            let needs_permission = !platform::permission_granted();
            installer::announce_update(prev, update::CURRENT, needs_permission);
        }
    }
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
        state::toggle_vietnamese()
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

    tray.refresh(&updated, state::current_app().as_deref());
}

pub(crate) fn open_config_file() {
    let path = Settings::config_path();
    if !path.exists() {
        state::settings().save();
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .arg("-t")
        .arg(&path)
        .spawn();
    #[cfg(not(target_os = "macos"))]
    update::open_url(&path.to_string_lossy());
}
