//! Cross-platform menu-bar / system-tray control surface.
//!
//! Replaces the Swift `MenuBarController` (NSStatusItem) with the `tray-icon` +
//! `muda` crates, which render a native `NSStatusItem` on macOS and a
//! `Shell_NotifyIcon` tray on Windows from the same code.

use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::config::{Method, Placement, Settings};

/// The tray icon plus the menu items whose state we update after each change.
pub struct Tray {
    tray: TrayIcon,
    pub toggle: CheckMenuItem,
    pub telex: CheckMenuItem,
    pub vni: CheckMenuItem,
    pub modern: CheckMenuItem,
    pub classic: CheckMenuItem,
    pub auto_restore: CheckMenuItem,
    pub quit: MenuItem,
}

impl Tray {
    /// Build the tray icon and menu, reflecting the current settings.
    pub fn new(settings: &Settings) -> Result<Self, String> {
        let header = MenuItem::new("phacius_vnkey — Vietnamese", false, None);
        let toggle = CheckMenuItem::new("Vietnamese typing", true, settings.enabled, None);

        let telex = CheckMenuItem::new("Telex", true, settings.method == Method::Telex, None);
        let vni = CheckMenuItem::new("VNI", true, settings.method == Method::Vni, None);
        let method_menu = Submenu::new("Input method", true);
        method_menu.append(&telex).map_err(|e| e.to_string())?;
        method_menu.append(&vni).map_err(|e| e.to_string())?;

        let modern = CheckMenuItem::new(
            "Modern  ·  hòa",
            true,
            settings.placement == Placement::Modern,
            None,
        );
        let classic = CheckMenuItem::new(
            "Classic  ·  hoà",
            true,
            settings.placement == Placement::Classic,
            None,
        );
        let tone_menu = Submenu::new("Tone placement", true);
        tone_menu.append(&modern).map_err(|e| e.to_string())?;
        tone_menu.append(&classic).map_err(|e| e.to_string())?;

        let auto_restore =
            CheckMenuItem::new("Auto-restore English", true, settings.auto_restore, None);
        let quit = MenuItem::new("Quit phacius_vnkey", true, None);

        let menu = Menu::new();
        let sep = || PredefinedMenuItem::separator();
        menu.append_items(&[
            &header,
            &sep(),
            &toggle,
            &sep(),
            &method_menu,
            &tone_menu,
            &auto_restore,
            &sep(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(status_icon(settings.enabled))
            .with_tooltip("phacius_vnkey — Vietnamese input")
            .build()
            .map_err(|e| e.to_string())?;

        Ok(Self {
            tray,
            toggle,
            telex,
            vni,
            modern,
            classic,
            auto_restore,
            quit,
        })
    }

    /// Push the current settings into the menu's checkmarks and the tray glyph.
    pub fn refresh(&self, settings: &Settings) {
        self.toggle.set_checked(settings.enabled);
        self.telex.set_checked(settings.method == Method::Telex);
        self.vni.set_checked(settings.method == Method::Vni);
        self.modern.set_checked(settings.placement == Placement::Modern);
        self.classic.set_checked(settings.placement == Placement::Classic);
        self.auto_restore.set_checked(settings.auto_restore);
        let _ = self.tray.set_icon(Some(status_icon(settings.enabled)));
    }
}

// ── Icon rendering ─────────────────────────────────────────────────────────────

/// Render the menu-bar glyph: a single neon letter on a dark rounded badge —
/// **V** (neon green) when Vietnamese typing is on, **E** (neon pink) when off.
/// Each letter glows. Drawn at 4× and box-downsampled for crisp, anti-aliased
/// edges.
fn status_icon(enabled: bool) -> Icon {
    let (rgba, size) = render_rgba(enabled);
    Icon::from_rgba(rgba, size, size).expect("valid rgba icon")
}

type Rgb = (f32, f32, f32);

/// Draw the badge to a straight-alpha RGBA buffer. Returns `(pixels, size)`.
fn render_rgba(enabled: bool) -> (Vec<u8>, u32) {
    const SIZE: usize = 36;
    const SS: usize = 4; // supersample factor
    const HI: usize = SIZE * SS;
    const N: usize = HI * HI;

    // Neon palette: bright tube color + a hot near-white core.
    let (neon, core): (Rgb, Rgb) = if enabled {
        ((57.0, 255.0, 20.0), (214.0, 255.0, 208.0)) // green — Vietnamese
    } else {
        ((255.0, 42.0, 130.0), (255.0, 210.0, 228.0)) // pink — English
    };
    let badge: Rgb = (12.0, 12.0, 20.0); // near-black, so neon reads as neon

    let hi_f = HI as f32;
    let inset = hi_f * 0.055;
    let radius = hi_f * 0.26;
    let (x0, y0, x1, y1) = (inset, inset, hi_f - inset, hi_f - inset);

    // Badge mask (1 inside the rounded rect).
    let mut mask = vec![0f32; N];
    for y in 0..HI {
        for x in 0..HI {
            if inside_rounded(x as f32 + 0.5, y as f32 + 0.5, x0, y0, x1, y1, radius) {
                mask[y * HI + x] = 1.0;
            }
        }
    }

    // Letter coverage (1 on the strokes).
    let mut cov = vec![0f32; N];
    let hw = hi_f * 0.058; // stroke half-width — chunky tube
    let (ty0, ty1) = (hi_f * 0.31, hi_f * 0.69);
    let lw = hi_f * 0.34;
    let cx = hi_f * 0.5;
    let lx = cx - lw * 0.5;
    if enabled {
        // V: two diagonals meeting at the bottom center.
        stroke(&mut cov, HI, lx, ty0, cx, ty1, hw);
        stroke(&mut cov, HI, cx, ty1, lx + lw, ty0, hw);
    } else {
        // E: left post + top, middle, bottom bars.
        stroke(&mut cov, HI, lx, ty0, lx, ty1, hw);
        stroke(&mut cov, HI, lx, ty0, lx + lw, ty0, hw);
        stroke(&mut cov, HI, lx, (ty0 + ty1) * 0.5, lx + lw * 0.86, (ty0 + ty1) * 0.5, hw);
        stroke(&mut cov, HI, lx, ty1, lx + lw, ty1, hw);
    }

    // Glow = blurred letter coverage.
    let glow = box_blur(&cov, HI, (hi_f * 0.05) as usize, 3);

    // Compose: badge, then additive neon glow, then the bright letter core.
    let mut hi = vec![0u8; N * 4];
    for i in 0..N {
        if mask[i] <= 0.0 {
            continue; // transparent outside the badge
        }
        let g = (glow[i] * 2.2).min(1.0);
        let mut r = badge.0 + neon.0 * g;
        let mut gg = badge.1 + neon.1 * g;
        let mut b = badge.2 + neon.2 * g;
        if cov[i] > 0.0 {
            r = core.0;
            gg = core.1;
            b = core.2;
        }
        let o = i * 4;
        hi[o] = r.min(255.0) as u8;
        hi[o + 1] = gg.min(255.0) as u8;
        hi[o + 2] = b.min(255.0) as u8;
        hi[o + 3] = 0xFF;
    }

    // Box-downsample with premultiplied alpha (avoids dark edge fringing).
    let mut out = vec![0u8; SIZE * SIZE * 4];
    let samples = (SS * SS) as u32;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (mut sr, mut sg, mut sb, mut sa) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let i = (((y * SS + sy) * HI) + (x * SS + sx)) * 4;
                    let a = hi[i + 3] as u32;
                    sr += hi[i] as u32 * a;
                    sg += hi[i + 1] as u32 * a;
                    sb += hi[i + 2] as u32 * a;
                    sa += a;
                }
            }
            let o = (y * SIZE + x) * 4;
            out[o + 3] = (sa / samples) as u8;
            // Un-premultiply. `checked_div` guards the fully-transparent block.
            if let Some(r) = sr.checked_div(sa) {
                out[o] = r as u8;
                out[o + 1] = (sg / sa) as u8;
                out[o + 2] = (sb / sa) as u8;
            }
        }
    }

    (out, SIZE as u32)
}

/// Whether a point lies inside a rounded rectangle.
fn inside_rounded(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    if px < x0 || px > x1 || py < y0 || py > y1 {
        return false;
    }
    let cx = px.clamp(x0 + r, x1 - r);
    let cy = py.clamp(y0 + r, y1 - r);
    let (dx, dy) = (px - cx, py - cy);
    dx * dx + dy * dy <= r * r
}

/// Mark a thick line segment (rounded caps) into a coverage buffer.
fn stroke(cov: &mut [f32], stride: usize, ax: f32, ay: f32, bx: f32, by: f32, half: f32) {
    let min_x = (ax.min(bx) - half).floor().max(0.0) as usize;
    let max_x = ((ax.max(bx) + half).ceil() as usize).min(stride - 1);
    let min_y = (ay.min(by) - half).floor().max(0.0) as usize;
    let max_y = ((ay.max(by) + half).ceil() as usize).min(stride - 1);
    let half_sq = half * half;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if dist_sq_to_segment(x as f32 + 0.5, y as f32 + 0.5, ax, ay, bx, by) <= half_sq {
                cov[y * stride + x] = 1.0;
            }
        }
    }
}

fn dist_sq_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq > 0.0 {
        (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    let (ex, ey) = (px - cx, py - cy);
    ex * ex + ey * ey
}

/// Separable box blur, repeated `passes` times to approximate a Gaussian.
fn box_blur(src: &[f32], stride: usize, radius: usize, passes: usize) -> Vec<f32> {
    let rows = src.len() / stride;
    let mut buf = src.to_vec();
    let mut tmp = vec![0f32; src.len()];
    let window = (radius * 2 + 1) as f32;
    for _ in 0..passes {
        // Horizontal.
        for y in 0..rows {
            let base = y * stride;
            for x in 0..stride {
                let lo = x.saturating_sub(radius);
                let hi = (x + radius).min(stride - 1);
                let mut sum = 0.0;
                for k in lo..=hi {
                    sum += buf[base + k];
                }
                tmp[base + x] = sum / window;
            }
        }
        // Vertical.
        for x in 0..stride {
            for y in 0..rows {
                let lo = y.saturating_sub(radius);
                let hi = (y + radius).min(rows - 1);
                let mut sum = 0.0;
                for k in lo..=hi {
                    sum += tmp[k * stride + x];
                }
                buf[y * stride + x] = sum / window;
            }
        }
    }
    buf
}
