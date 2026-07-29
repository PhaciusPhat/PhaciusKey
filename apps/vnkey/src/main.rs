//! vnkey — a cross-platform Vietnamese input method.
//!
//! Architecture: a shared, OS-independent engine (`vnkey-core`) driven by a thin
//! per-OS keyboard hook (`platform`), with a cross-platform tray menu (`tray`)
//! and TOML settings (`config`). The event loop (`tao`) runs the app as a
//! menu-bar / tray accessory with no Dock icon or window.

// No console window on Windows release builds.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod platform;
mod state;
mod tray;

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoop};
use tray_icon::menu::MenuEvent;

use crate::config::Settings;
use crate::platform::{Hook, KeyboardHook};
use crate::tray::Tray;

fn main() {
    // Load persisted settings and initialize the shared engine state.
    let settings = Settings::load();
    state::init(settings);

    let event_loop = EventLoop::new();

    // Menu-bar accessory: no Dock icon, no window.
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        // `event_loop` is consumed by `run` below, so configure via a temporary
        // mutable binding first.
        let mut event_loop = event_loop;
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        run(event_loop);
    }

    #[cfg(not(target_os = "macos"))]
    run(event_loop);
}

fn run(event_loop: EventLoop<()>) -> ! {
    // Built after the loop starts (StartCause::Init), per tray-icon guidance.
    let mut tray: Option<Tray> = None;
    let mut hook: Option<Hook> = None;

    let menu_channel = MenuEvent::receiver();

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;

        if let Event::NewEvents(StartCause::Init) = event {
            let settings = state::settings();
            match Tray::new(&settings) {
                Ok(t) => tray = Some(t),
                Err(e) => eprintln!("[vnkey] failed to create tray: {e}"),
            }
            match Hook::install() {
                Ok(h) => hook = Some(h),
                Err(e) => eprintln!("[vnkey] {e}"),
            }
        }

        // Drain menu clicks. macOS wakes the loop on click, so Wait is fine.
        while let Ok(menu_event) = menu_channel.try_recv() {
            if let Some(tray) = &tray {
                handle_menu_event(tray, &menu_event.id, control_flow);
            }
        }

        // `hook` is never read after install — it is held here solely to keep
        // the OS keyboard hook alive for the lifetime of the event loop.
        let _keep_hook_alive = &hook;
    })
}

fn handle_menu_event(
    tray: &Tray,
    id: &tray_icon::menu::MenuId,
    control_flow: &mut ControlFlow,
) {
    use config::{Method, Placement};

    if id == tray.quit.id() {
        *control_flow = ControlFlow::Exit;
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
