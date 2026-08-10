use std::cell::Cell;

use serde_json::json;
use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder};
use wry::WebView;

use super::screen::{Rect, Screen};
use super::Surface;
use crate::update::Notice;
use crate::UserEvent;

const CSS: &str = include_str!("assets/alert.css");
const BODY: &str = include_str!("assets/alert.html");
const SCRIPT: &str = include_str!("assets/alert.js");

const WIDTH: f64 = 380.0;
const INITIAL_HEIGHT: f64 = 160.0;

fn centre_origin(size: (f64, f64), monitor_area: Rect) -> (f64, f64) {
    let x = monitor_area.x + (monitor_area.width - size.0) / 2.0;
    let y = monitor_area.y + (monitor_area.height - size.1) / 2.0;
    (x, y)
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

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 100.0,
        y: 0.0,
        width: 1000.0,
        height: 800.0,
    };

    #[test]
    fn it_centres_on_the_work_area() {
        let (x, y) = centre_origin((400.0, 200.0), SCREEN);
        assert_eq!(x, 400.0);
        assert_eq!(y, 300.0);
    }

    #[test]
    fn it_centres_on_a_work_area_that_does_not_start_at_zero() {
        let screen = Rect {
            x: -1920.0,
            y: -1080.0,
            width: 1920.0,
            height: 1080.0,
        };
        let (x, y) = centre_origin((400.0, 200.0), screen);
        assert_eq!(x, -1160.0);
        assert_eq!(y, -640.0);
    }
}
