use std::cell::Cell;

use serde_json::json;
use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::WebView;

use super::panel::Rect;
use super::Surface;
use crate::update::Notice;
use crate::UserEvent;

#[allow(dead_code)]
const CSS: &str = include_str!("assets/alert.css");
#[allow(dead_code)]
const BODY: &str = include_str!("assets/alert.html");
#[allow(dead_code)]
const SCRIPT: &str = include_str!("assets/alert.js");

#[allow(dead_code)]
const WIDTH: f64 = 380.0;
#[allow(dead_code)]
const INITIAL_HEIGHT: f64 = 160.0;

#[allow(dead_code)]
fn centre_origin(size: PhysicalSize<u32>, work_area: Rect) -> PhysicalPosition<i32> {
    let x = work_area.x + (work_area.width - f64::from(size.width)) / 2.0;
    let y = work_area.y + (work_area.height - f64::from(size.height)) / 2.0;
    PhysicalPosition::new(x.round() as i32, y.round() as i32)
}

#[allow(dead_code)]
fn notice_json(notice: &Notice) -> String {
    json!({
        "title": notice.title,
        "body": notice.body,
        "warn": notice.warn,
        "action": notice.action.map(|a| json!({ "label": a.label(), "cmd": a.cmd() })),
    })
    .to_string()
}

#[allow(dead_code)]
pub struct Alert {
    window: Window,
    webview: WebView,
    height: Cell<f64>,
}

#[allow(dead_code)]
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

    pub fn window_id(&self) -> WindowId {
        self.window.id()
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
        let scale = monitor.scale_factor();
        let size = PhysicalSize::new(
            (WIDTH * scale).round() as u32,
            (self.height.get() * scale).round() as u32,
        );
        let position = monitor.position();
        let monitor_size = monitor.size();
        let work_area = Rect {
            x: f64::from(position.x),
            y: f64::from(position.y),
            width: f64::from(monitor_size.width),
            height: f64::from(monitor_size.height),
        };

        self.window.set_inner_size(size);
        self.window
            .set_outer_position(centre_origin(size, work_area));
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
        let origin = centre_origin(PhysicalSize::new(400, 200), SCREEN);
        assert_eq!(origin.x, 400);
        assert_eq!(origin.y, 300);
    }

    #[test]
    fn it_centres_on_a_work_area_that_does_not_start_at_zero() {
        let screen = Rect {
            x: -1920.0,
            y: -1080.0,
            width: 1920.0,
            height: 1080.0,
        };
        let origin = centre_origin(PhysicalSize::new(400, 200), screen);
        assert_eq!(origin.x, -1160);
        assert_eq!(origin.y, -640);
    }
}
