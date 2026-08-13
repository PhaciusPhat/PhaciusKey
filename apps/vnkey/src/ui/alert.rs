use std::cell::Cell;

use serde_json::json;
use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder};
use wry::WebView;

use super::screen::{centre_origin, Screen};
use super::Surface;
use crate::update::Notice;
use crate::UserEvent;

const CSS: &str = include_str!("assets/alert.css");
const BODY: &str = include_str!("assets/alert.html");
const SCRIPT: &str = include_str!("assets/alert.js");

const WIDTH: f64 = 380.0;
const INITIAL_HEIGHT: f64 = 160.0;

/// `-[NSWindow orderFrontRegardless]` raises a window without activating the
/// application. `makeKeyAndOrderFront:`, which `Window::set_visible` sends,
/// would move the keyboard to this window, and it has no field to type into.
#[cfg(target_os = "macos")]
fn order_front_regardless(window: *mut std::ffi::c_void) {
    use std::ffi::{c_char, c_void};

    #[link(name = "objc")]
    extern "C" {
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn objc_msgSend();
    }

    type SendNoArgs = unsafe extern "C" fn(*mut c_void, *const c_void);

    if window.is_null() {
        return;
    }

    // SAFETY: `objc_msgSend` has to be called through a signature matching the
    // selector's own; `orderFrontRegardless` takes no argument and returns
    // nothing. The window pointer belongs to tao, which outlives this call.
    unsafe {
        let selector = sel_registerName(c"orderFrontRegardless".as_ptr());
        let send = std::mem::transmute::<unsafe extern "C" fn(), SendNoArgs>(objc_msgSend);
        send(window, selector);
    }
}

fn notice_json(notice: &Notice) -> String {
    json!({
        "title": notice.title,
        "body": notice.body,
        "warn": notice.warn,
        "action": notice.action.map(|a| json!({ "label": a.label(), "cmd": a.cmd() })),
    })
    .to_string()
}

pub struct Alert {
    window: Window,
    webview: WebView,
    height: Cell<f64>,
}

impl Alert {
    pub fn new(
        target: &EventLoopWindowTarget<UserEvent>,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, String> {
        let window = WindowBuilder::new()
            .with_title("PhaciusKey")
            .with_inner_size(LogicalSize::new(WIDTH, INITIAL_HEIGHT))
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_always_on_top(true)
            .with_visible(false)
            .build(target)
            .map_err(|e| e.to_string())?;

        let webview = wry::WebViewBuilder::new()
            .with_html(super::document(CSS, BODY, SCRIPT))
            .with_transparent(true)
            // The alert can appear while another application is frontmost, so the first
            // click has to reach the button rather than be spent focusing the window.
            .with_accept_first_mouse(true)
            .with_ipc_handler(move |request| {
                let _ = proxy.send_event(UserEvent::Ipc(Surface::Alert, request.body().clone()));
            })
            .build(&window)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            window,
            webview,
            height: Cell::new(INITIAL_HEIGHT),
        })
    }

    pub fn show(&self, notice: &Notice, target: &EventLoopWindowTarget<UserEvent>) {
        let _ = self
            .webview
            .evaluate_script(&format!("window.__setNotice({})", notice_json(notice)));
        self.place(target);
        self.reveal();
    }

    /// An update notice arrives unbidden, so it is shown without being given the
    /// keyboard.
    #[cfg(target_os = "macos")]
    fn reveal(&self) {
        use tao::platform::macos::WindowExtMacOS;

        order_front_regardless(self.window.ns_window());
    }

    #[cfg(not(target_os = "macos"))]
    fn reveal(&self) {
        self.window.set_visible(true);
        self.window.set_focus();
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    pub fn set_content_height(&self, height: f64, target: &EventLoopWindowTarget<UserEvent>) {
        if height <= 0.0 || (height - self.height.get()).abs() < 1.0 {
            return;
        }
        self.height.set(height);
        self.place(target);
    }

    fn place(&self, target: &EventLoopWindowTarget<UserEvent>) {
        let Some(monitor) = target.primary_monitor() else {
            return;
        };
        let (screen, monitor_area) = Screen::of(&monitor);
        let (width, height) = screen.size_from_logical(WIDTH, self.height.get());
        let (x, y) = centre_origin((width, height), monitor_area);

        self.window.set_inner_size(screen.size(width, height));
        self.window.set_outer_position(screen.position(x, y));
    }
}
