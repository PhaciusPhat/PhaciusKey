use std::cell::RefCell;

use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::WebView;

use super::payload::state_json;
use super::screen::{active_monitor, centre_origin, Screen};
use super::Surface;
use crate::{platform, state, UserEvent};

const CSS: &str = include_str!("assets/settings.css");
const BODY: &str = include_str!("assets/settings.html");
const SCRIPT: &str = include_str!("assets/settings.js");

/// Fixed, and deliberately so: nothing here reflows usefully, and a fixed
/// window means the header and the nav stay put while only the pane scrolls.
const SIZE: LogicalSize<f64> = LogicalSize::new(720.0, 560.0);

pub struct SettingsWindow {
    window: Window,
    webview: WebView,
    installed_apps: RefCell<Vec<String>>,
}

impl SettingsWindow {
    pub fn new(
        target: &EventLoopWindowTarget<UserEvent>,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, String> {
        let window = WindowBuilder::new()
            .with_title("PhaciusKey Settings")
            .with_inner_size(SIZE)
            .with_resizable(false)
            .with_decorations(false)
            // Transparent so the rounded corners and the shadow are drawn by
            // the page rather than cut out of an opaque rectangle.
            .with_transparent(true)
            .with_visible(false)
            .build(target)
            .map_err(|e| e.to_string())?;

        let webview = wry::WebViewBuilder::new()
            .with_html(super::document(CSS, BODY, SCRIPT))
            .with_transparent(true)
            .with_ipc_handler(move |request| {
                let _ = proxy.send_event(UserEvent::Ipc(Surface::Settings, request.body().clone()));
            })
            .build(&window)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            window,
            webview,
            installed_apps: RefCell::new(platform::installed_apps()),
        })
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub fn show(&self, target: &EventLoopWindowTarget<UserEvent>) {
        *self.installed_apps.borrow_mut() = platform::installed_apps();
        // Only on the way out of hidden, so that reopening settings from the
        // tray does not drag a window the user has already placed.
        if !self.window.is_visible() {
            self.place(target);
        }
        self.window.set_visible(true);
        self.window.set_focus();
    }

    fn place(&self, target: &EventLoopWindowTarget<UserEvent>) {
        let Some(monitor) = active_monitor(target) else {
            return;
        };
        let (screen, area) = Screen::of(&monitor);
        let (width, height) = screen.size_from_logical(SIZE.width, SIZE.height);
        let (x, y) = centre_origin((width, height), area);

        self.window.set_inner_size(screen.size(width, height));
        self.window.set_outer_position(screen.position(x, y));
    }

    pub fn hide(&self) {
        state::set_shortcut_recording(false);
        self.window.set_visible(false);
    }

    /// Hands the gesture to the window manager, which then owns it: there is
    /// no titlebar for it to move the window by.
    pub fn drag(&self) {
        let _ = self.window.drag_window();
    }

    pub fn push_state(&self) {
        let state = state_json(
            &state::settings(),
            state::current_app().as_deref(),
            &self.installed_apps.borrow(),
        );
        let _ = self
            .webview
            .evaluate_script(&format!("window.__setState({state})"));
    }
}
