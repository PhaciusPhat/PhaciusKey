use tao::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, Position, Size};
use tao::event_loop::EventLoopWindowTarget;
use tao::monitor::MonitorHandle;

use crate::UserEvent;

/// A rectangle with y growing downwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
}

/// One display, and the units its geometry has to be expressed in before it can
/// be compared with another display's.
///
/// macOS lays every display out on a single grid of points: `CGDisplayBounds`,
/// `NSWindow` frames and `NSEvent` locations are all points, so two displays of
/// unequal backing scale only line up once each one's pixel geometry is divided
/// by its own factor. The Windows virtual desktop is a grid of physical pixels,
/// where that division would pull the displays apart instead.
#[derive(Debug, Clone, Copy)]
pub struct Screen {
    scale: f64,
    points: bool,
}

impl Screen {
    pub fn new(scale: f64) -> Self {
        Self {
            scale,
            points: cfg!(target_os = "macos"),
        }
    }

    /// Both layouts, so that either can be exercised from either platform.
    #[cfg(test)]
    pub fn for_test(scale: f64, points: bool) -> Self {
        Self { scale, points }
    }

    /// The display and its full area, ready to place a window against.
    pub fn of(monitor: &MonitorHandle) -> (Self, Rect) {
        let screen = Self::new(monitor.scale_factor());
        let position = monitor.position();
        let size = monitor.size();
        let area = screen.rect_from_pixels(Rect {
            x: f64::from(position.x),
            y: f64::from(position.y),
            width: f64::from(size.width),
            height: f64::from(size.height),
        });
        (screen, area)
    }

    /// Converts a rectangle as `tao` and `tray_icon` report it.
    pub fn rect_from_pixels(&self, rect: Rect) -> Rect {
        if !self.points {
            return rect;
        }
        Rect {
            x: rect.x / self.scale,
            y: rect.y / self.scale,
            width: rect.width / self.scale,
            height: rect.height / self.scale,
        }
    }

    /// Converts a size the page laid itself out in.
    pub fn size_from_logical(&self, width: f64, height: f64) -> (f64, f64) {
        if self.points {
            (width, height)
        } else {
            (width * self.scale, height * self.scale)
        }
    }

    pub fn position(&self, x: f64, y: f64) -> Position {
        if self.points {
            LogicalPosition::new(x, y).into()
        } else {
            PhysicalPosition::new(x, y).into()
        }
    }

    pub fn size(&self, width: f64, height: f64) -> Size {
        if self.points {
            LogicalSize::new(width, height).into()
        } else {
            PhysicalSize::new(width, height).into()
        }
    }
}

/// The display the pointer is on, which is the one the user is working on: it
/// is what macOS moves the menu bar to and what Windows opens a menu on.
pub fn active_monitor(target: &EventLoopWindowTarget<UserEvent>) -> Option<MonitorHandle> {
    crate::platform::pointer_position()
        .and_then(|(x, y)| target.monitor_from_point(x, y))
        .or_else(|| target.primary_monitor())
}

/// Where to put a window of `size` so it sits in the middle of `area`, in
/// whatever units the two share.
pub fn centre_origin(size: (f64, f64), area: Rect) -> (f64, f64) {
    let x = area.x + (area.width - size.0) / 2.0;
    let y = area.y + (area.height - size.1) / 2.0;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(scale: f64) -> Screen {
        Screen::for_test(scale, true)
    }

    fn pixels(scale: f64) -> Screen {
        Screen::for_test(scale, false)
    }

    fn rect(x: f64, width: f64) -> Rect {
        Rect {
            x,
            y: 0.0,
            width,
            height: 100.0,
        }
    }

    /// The 3024px-wide display of a MacBook is 1512pt wide, so a 1x display
    /// placed to its right starts at pixel 1512 — inside it — and only stops
    /// overlapping once both are read as points.
    #[test]
    fn mixed_scale_displays_stop_overlapping_in_points() {
        let built_in = points(2.0).rect_from_pixels(rect(0.0, 3024.0));
        let external = points(1.0).rect_from_pixels(rect(1512.0, 1920.0));
        assert_eq!(built_in.right(), external.x);
    }

    #[test]
    fn a_pixel_layout_leaves_the_geometry_alone() {
        assert_eq!(
            pixels(2.0).rect_from_pixels(rect(1512.0, 1920.0)),
            rect(1512.0, 1920.0)
        );
    }

    #[test]
    fn a_point_layout_keeps_a_logical_size_as_it_is() {
        assert_eq!(points(2.0).size_from_logical(296.0, 280.0), (296.0, 280.0));
    }

    #[test]
    fn a_pixel_layout_scales_a_logical_size_up() {
        assert_eq!(pixels(2.0).size_from_logical(296.0, 280.0), (592.0, 560.0));
    }

    /// A position handed back in the units it was computed in, so that `tao`
    /// converts it with the scale of the display it is going to rather than the
    /// scale of the one the window is leaving.
    #[test]
    fn a_point_layout_hands_positions_back_as_logical() {
        assert!(matches!(
            points(2.0).position(10.0, 20.0),
            Position::Logical(_)
        ));
    }

    #[test]
    fn a_pixel_layout_hands_positions_back_as_physical() {
        assert!(matches!(
            pixels(2.0).position(10.0, 20.0),
            Position::Physical(_)
        ));
    }

    #[test]
    fn it_centres_on_the_area() {
        let area = Rect {
            x: 100.0,
            y: 0.0,
            width: 1000.0,
            height: 800.0,
        };
        assert_eq!(centre_origin((400.0, 200.0), area), (400.0, 300.0));
    }

    #[test]
    fn it_centres_on_an_area_that_does_not_start_at_zero() {
        let area = Rect {
            x: -1920.0,
            y: -1080.0,
            width: 1920.0,
            height: 1080.0,
        };
        assert_eq!(centre_origin((400.0, 200.0), area), (-1160.0, -640.0));
    }
}
