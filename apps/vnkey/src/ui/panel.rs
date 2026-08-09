use std::cell::Cell;
use std::time::{Duration, Instant};

use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::WebView;

use super::payload::state_json;
use super::Surface;
use crate::{state, UserEvent};

const CSS: &str = include_str!("assets/panel.css");
const BODY: &str = include_str!("assets/panel.html");
const SCRIPT: &str = include_str!("assets/panel.js");

/// Distance in physical pixels between the tray icon and the panel it opens.
const GAP: f64 = 8.0;

const WIDTH: f64 = 296.0;

/// Only until the page reports what it actually needs.
const INITIAL_HEIGHT: f64 = 280.0;

/// Clicking the tray icon while the panel is open delivers focus loss before
/// the click. Without a grace period the panel would hide and reopen on the
/// same gesture, which reads as the panel refusing to close.
const DISMISS_GRACE: Duration = Duration::from_millis(250);

/// A rectangle in physical pixels with y growing downwards, which is the
/// convention `tray_icon` reports icon geometry in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.width
    }

    fn bottom(&self) -> f64 {
        self.y + self.height
    }
}

impl From<tray_icon::Rect> for Rect {
    fn from(rect: tray_icon::Rect) -> Self {
        Self {
            x: rect.position.x,
            y: rect.position.y,
            width: f64::from(rect.size.width),
            height: f64::from(rect.size.height),
        }
    }
}

/// Where to put a panel of `panel` size so it hangs off `icon`.
///
/// Pure, so the placement rules are testable without a screen: centred on the
/// icon, below it when there is room and above it when there is not — a Windows
/// taskbar sits at the bottom of the work area — and never past either edge.
fn panel_origin(icon: Rect, panel: PhysicalSize<u32>, work_area: Rect) -> PhysicalPosition<i32> {
    let (width, height) = (f64::from(panel.width), f64::from(panel.height));

    let x = icon.x + icon.width / 2.0 - width / 2.0;
    let x = x.min(work_area.right() - width).max(work_area.x);

    let below = icon.bottom() + GAP;
    let y = if below + height <= work_area.bottom() {
        below
    } else {
        (icon.y - GAP - height).max(work_area.y)
    };

    PhysicalPosition::new(x.round() as i32, y.round() as i32)
}

/// The tray-anchored panel that stands in for the menu the tray icon used to
/// carry, so the two surfaces the app shows can share one theme.
pub struct Panel {
    window: Window,
    webview: WebView,
    /// The icon the panel is currently hanging off, kept so a height change can
    /// re-anchor it rather than leave it floating where it first appeared.
    anchor: Cell<Option<Rect>>,
    height: Cell<f64>,
    dismissed_at: Cell<Option<Instant>>,
}

impl Panel {
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
            // Otherwise the click that raises the panel is spent focusing it and
            // the control under the pointer does not fire.
            .with_accept_first_mouse(true)
            .with_ipc_handler(move |request| {
                let _ = proxy.send_event(UserEvent::Ipc(Surface::Panel, request.body().clone()));
            })
            .build(&window)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            window,
            webview,
            anchor: Cell::new(None),
            height: Cell::new(INITIAL_HEIGHT),
            dismissed_at: Cell::new(None),
        })
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub fn toggle(&self, icon: tray_icon::Rect, target: &EventLoopWindowTarget<UserEvent>) {
        if let Some(dismissed) = self.dismissed_at.take() {
            if dismissed.elapsed() < DISMISS_GRACE {
                return;
            }
        }
        if self.window.is_visible() {
            self.hide();
        } else {
            self.show_at(icon, target);
        }
    }

    pub fn show_at(&self, icon: tray_icon::Rect, target: &EventLoopWindowTarget<UserEvent>) {
        self.anchor.set(Some(icon.into()));
        self.push_state();
        self.place(target);
        self.window.set_visible(true);
        self.window.set_focus();
    }

    pub fn hide(&self) {
        self.dismissed_at.set(None);
        self.window.set_visible(false);
    }

    /// Hidden because focus went elsewhere, which is the one dismissal that has
    /// to be remembered — see `DISMISS_GRACE`.
    pub fn dismiss(&self) {
        self.window.set_visible(false);
        self.dismissed_at.set(Some(Instant::now()));
    }

    pub fn is_visible(&self) -> bool {
        self.window.is_visible()
    }

    /// The warning row and the update row both come and go, so the page is what
    /// knows how tall the panel should be.
    pub fn set_content_height(&self, height: f64, target: &EventLoopWindowTarget<UserEvent>) {
        if height <= 0.0 || (height - self.height.get()).abs() < 1.0 {
            return;
        }
        self.height.set(height);
        self.place(target);
    }

    fn place(&self, target: &EventLoopWindowTarget<UserEvent>) {
        let Some(icon) = self.anchor.get() else {
            return;
        };
        let Some(monitor) = target
            .monitor_from_point(icon.x + icon.width / 2.0, icon.y + icon.height / 2.0)
            .or_else(|| target.primary_monitor())
        else {
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
            .set_outer_position(panel_origin(icon, size, work_area));
    }

    pub fn push_state(&self) {
        let state = state_json(&state::settings(), state::current_app().as_deref(), &[]);
        let _ = self
            .webview
            .evaluate_script(&format!("window.__setState({state})"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deliberately not at the origin, so a clamped coordinate cannot be
    /// confused with a zero one.
    const SCREEN: Rect = Rect {
        x: 100.0,
        y: 0.0,
        width: 1000.0,
        height: 800.0,
    };

    fn icon_at(x: f64, y: f64) -> Rect {
        Rect {
            x,
            y,
            width: 24.0,
            height: 24.0,
        }
    }

    fn panel() -> PhysicalSize<u32> {
        PhysicalSize::new(300, 400)
    }

    #[test]
    fn it_hangs_centred_below_the_icon() {
        let origin = panel_origin(icon_at(500.0, 0.0), panel(), SCREEN);
        assert_eq!(origin.x, 362, "should centre on the icon");
        assert_eq!(origin.y, 32, "should sit below the icon");
    }

    /// A Windows taskbar puts the icon at the bottom of the work area, where
    /// there is no room underneath for anything.
    #[test]
    fn it_flips_above_the_icon_when_there_is_no_room_below() {
        let origin = panel_origin(icon_at(500.0, 776.0), panel(), SCREEN);
        assert_eq!(origin.y, 368, "should sit above the icon");
    }

    #[test]
    fn it_stops_at_the_left_edge() {
        let origin = panel_origin(icon_at(104.0, 0.0), panel(), SCREEN);
        assert_eq!(origin.x, 100);
    }

    #[test]
    fn it_stops_at_the_right_edge() {
        let origin = panel_origin(icon_at(1080.0, 0.0), panel(), SCREEN);
        assert_eq!(origin.x, 800);
    }

    /// The work area does not start at the origin on a second display, and a
    /// panel taller than the space above its icon must not walk off the top.
    #[test]
    fn it_stays_inside_a_work_area_that_does_not_start_at_zero() {
        let screen = Rect {
            x: -1920.0,
            y: -1080.0,
            width: 1920.0,
            height: 1080.0,
        };
        let origin = panel_origin(icon_at(-1900.0, -120.0), panel(), screen);
        assert_eq!(origin.x, -1920, "should stop at the left edge");
        assert_eq!(origin.y, -528, "should flip above the icon");
    }
}
