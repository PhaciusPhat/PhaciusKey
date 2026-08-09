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
mod state;
mod tray;
mod ui;
mod update;

use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::MenuEvent;
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

use crate::config::Settings;
use crate::platform::{Hook, KeyboardHook};
use crate::tray::Tray;
use crate::ui::{Alert, Panel, SettingsWindow, Surface, WindowAction};

enum UserEvent {
    UpdateAvailable(String),
    UpdateInstalled(std::path::PathBuf),
    UpdateFailed(String, String),
    UpdateCheckDone(Result<Option<String>, String>),
    StateChanged,
    Ipc(Surface, String),
}

fn main() {
    let mut args = std::env::args().skip(1);
    let args_start = args.next();
    if args_start.as_deref() == Some("--export-iconset") {
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

    let mut forced_alert = None;
    if args_start.as_deref() == Some("--show-alert") {
        let Some(kind) = args.next() else {
            eprintln!(
                "usage: vnkey --show-alert <updated-needs-permission|install-failed|up-to-date|check-failed>"
            );
            std::process::exit(2);
        };
        forced_alert = Some(match kind.as_str() {
            "updated-needs-permission" => update::notice_updated("0.0.24", update::CURRENT, true),
            "install-failed" => update::notice_install_failed(update::CURRENT, "a sample failure"),
            "up-to-date" => update::notice_up_to_date(),
            "check-failed" => update::notice_check_failed("a sample failure"),
            other => {
                eprintln!("[vnkey] unknown alert kind: {other}");
                std::process::exit(2);
            }
        });
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
        run(event_loop, forced_alert);
    }

    #[cfg(not(target_os = "macos"))]
    run(event_loop, forced_alert);
}

fn run(event_loop: EventLoop<UserEvent>, forced_alert: Option<update::Notice>) -> ! {
    let proxy = event_loop.create_proxy();
    let menu_channel = MenuEvent::receiver();
    let tray_channel = TrayIconEvent::receiver();

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
    let mut panel: Option<Panel> = None;
    let mut alert: Option<Alert> = None;
    let mut forced_alert = forced_alert;

    event_loop.run(move |event, target, control_flow| {
        match event {
            Event::NewEvents(StartCause::Init) => {
                let settings = state::settings();
                match Tray::new(&settings) {
                    Ok(t) => tray = Some(t),
                    Err(e) => eprintln!("[vnkey] failed to create tray: {e}"),
                }
                match Panel::new(target, proxy.clone()) {
                    Ok(p) => panel = Some(p),
                    Err(e) => {
                        eprintln!("[vnkey] falling back to a native menu: {e}");
                        if let Some(tray) = &mut tray {
                            if let Err(e) = tray.attach_fallback_menu() {
                                eprintln!("[vnkey] failed to attach the fallback menu: {e}");
                            }
                        }
                    }
                }
                match forced_alert.take() {
                    Some(notice) => show_alert(&mut alert, &notice, target, &proxy),
                    None => {
                        if let Some(notice) = completed_update_notice() {
                            show_alert(&mut alert, &notice, target, &proxy);
                        }
                    }
                }
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
                update::set_status(update::Status::Available(version.clone()));
                if state::settings().auto_update && installer::app_bundle().is_some() {
                    update::set_status(update::Status::Installing(version.clone()));
                    spawn_update_install(proxy.clone(), version);
                }
            }
            Event::UserEvent(UserEvent::UpdateInstalled(app)) => {
                installer::relaunch_and_exit(&app);
            }
            Event::UserEvent(UserEvent::UpdateFailed(version, reason)) => {
                eprintln!("[vnkey] automatic update to {version} failed: {reason}");
                update::set_status(update::Status::Failed(reason.clone()));
                let notice = update::notice_install_failed(&version, &reason);
                show_alert(&mut alert, &notice, target, &proxy);
            }
            Event::UserEvent(UserEvent::UpdateCheckDone(result)) => match result {
                Ok(Some(version)) => {
                    update::set_status(update::Status::Available(version.clone()));
                    start_install(&proxy, version);
                }
                Ok(None) => {
                    update::set_status(update::Status::Idle);
                    let notice = update::notice_up_to_date();
                    show_alert(&mut alert, &notice, target, &proxy);
                }
                Err(e) => {
                    update::set_status(update::Status::Failed(e.clone()));
                    let notice = update::notice_check_failed(&e);
                    show_alert(&mut alert, &notice, target, &proxy);
                }
            },
            Event::UserEvent(UserEvent::StateChanged) => {
                push_state(&tray, &settings_win, &panel);
            }
            Event::UserEvent(UserEvent::Ipc(surface, msg)) => {
                match ui::apply_ipc(&msg) {
                    Some(WindowAction::Close) => match surface {
                        Surface::Panel => {
                            if let Some(panel) = &panel {
                                panel.hide();
                            }
                        }
                        Surface::Settings => {
                            if let Some(win) = &settings_win {
                                win.hide();
                            }
                        }
                        Surface::Alert => {
                            if let Some(win) = &alert {
                                win.hide();
                            }
                        }
                    },
                    Some(WindowAction::Drag) => {
                        if let Some(win) = &settings_win {
                            win.drag();
                        }
                    }
                    Some(WindowAction::Resize(height)) => match surface {
                        Surface::Alert => {
                            if let Some(win) = &alert {
                                win.set_content_height(f64::from(height), target);
                            }
                        }
                        Surface::Panel | Surface::Settings => {
                            if let Some(panel) = &panel {
                                panel.set_content_height(f64::from(height), target);
                            }
                        }
                    },
                    Some(WindowAction::OpenSettings) => {
                        if let Some(panel) = &panel {
                            panel.hide();
                        }
                        open_settings(&mut settings_win, target, &proxy);
                    }
                    Some(WindowAction::CheckUpdates) => match update::available_version() {
                        Some(version) => start_install(&proxy, version),
                        None => {
                            update::set_status(update::Status::Checking);
                            let proxy = proxy.clone();
                            std::thread::spawn(move || {
                                let _ = proxy.send_event(UserEvent::UpdateCheckDone(
                                    update::check_for_newer(),
                                ));
                            });
                        }
                    },
                    Some(WindowAction::Quit) => *control_flow = ControlFlow::Exit,
                    None => {}
                }
                push_state(&tray, &settings_win, &panel);
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
            // Clicking away from the panel dismisses it, the way the menu it
            // replaced behaved.
            Event::WindowEvent {
                event: tao::event::WindowEvent::Focused(false),
                window_id,
                ..
            } => {
                if let Some(panel) = &panel {
                    if panel.window_id() == window_id {
                        panel.dismiss();
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

        while let Ok(tray_event) = tray_channel.try_recv() {
            if let TrayIconEvent::Click {
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = tray_event
            {
                if let Some(panel) = &panel {
                    panel.toggle(rect, target);
                }
            }
        }

        while let Ok(menu_event) = menu_channel.try_recv() {
            if let Some(fallback) = tray.as_ref().and_then(Tray::fallback) {
                if menu_event.id == fallback.settings.id() {
                    open_settings(&mut settings_win, target, &proxy);
                } else if menu_event.id == fallback.quit.id() {
                    *control_flow = ControlFlow::Exit;
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

/// Every surface renders the same state, so they are all refreshed together
/// rather than each one being remembered at each call site.
fn push_state(tray: &Option<Tray>, settings_win: &Option<SettingsWindow>, panel: &Option<Panel>) {
    if let Some(tray) = tray {
        tray.refresh(&state::settings(), state::current_app().as_deref());
    }
    if let Some(win) = settings_win {
        win.push_state();
    }
    if let Some(panel) = panel {
        if panel.is_visible() {
            panel.push_state();
        }
    }
}

fn show_alert(
    alert: &mut Option<Alert>,
    notice: &update::Notice,
    target: &tao::event_loop::EventLoopWindowTarget<UserEvent>,
    proxy: &EventLoopProxy<UserEvent>,
) {
    if alert.is_none() {
        match Alert::new(target, proxy.clone()) {
            Ok(win) => *alert = Some(win),
            Err(e) => {
                eprintln!("[vnkey] failed to create the alert window: {e}");
                return;
            }
        }
    }
    if let Some(win) = alert {
        win.show(notice, target);
    }
}

fn open_settings(
    settings_win: &mut Option<SettingsWindow>,
    target: &tao::event_loop::EventLoopWindowTarget<UserEvent>,
    proxy: &EventLoopProxy<UserEvent>,
) {
    match settings_win {
        Some(win) => win.show(),
        None => match SettingsWindow::new(target, proxy.clone()) {
            Ok(win) => {
                win.show();
                *settings_win = Some(win);
            }
            Err(e) => eprintln!("[vnkey] failed to open settings: {e}"),
        },
    }
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

fn start_install(proxy: &EventLoopProxy<UserEvent>, version: String) {
    if installer::app_bundle().is_some() {
        update::set_status(update::Status::Installing(version.clone()));
        spawn_update_install(proxy.clone(), version);
    } else {
        update::open_url(&update::releases_url());
    }
}

fn completed_update_notice() -> Option<update::Notice> {
    let previous = state::settings().last_seen_version;
    let notice = previous
        .as_deref()
        .filter(|prev| *prev != update::CURRENT)
        .map(|prev| update::notice_updated(prev, update::CURRENT, !platform::permission_granted()));
    if previous.as_deref() != Some(update::CURRENT) {
        state::update(|s| s.last_seen_version = Some(update::CURRENT.to_string()));
    }
    notice
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
