//! Cross-platform menu-bar / system-tray control surface.
//!
//! Replaces the Swift `MenuBarController` (NSStatusItem) with the `tray-icon` +
//! `muda` crates, which render a native `NSStatusItem` on macOS and a
//! `Shell_NotifyIcon` tray on Windows from the same code.

use std::cell::RefCell;
use std::io;
use std::path::Path;

use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::config::{parse_shortcut, Method, Placement, Settings, Shortcut};
use crate::png_write;
use crate::update;

/// The tray icon plus the menu items whose state we update after each change.
pub struct Tray {
    tray: TrayIcon,
    pub toggle: CheckMenuItem,
    pub app_toggle: CheckMenuItem,
    pub telex: CheckMenuItem,
    pub vni: CheckMenuItem,
    pub modern: CheckMenuItem,
    pub classic: CheckMenuItem,
    pub auto_restore: CheckMenuItem,
    pub start_login: CheckMenuItem,
    pub update: MenuItem,
    pub report: MenuItem,
    pub quit: MenuItem,
    /// Newest known release, once a check has found one. `RefCell`: the tray
    /// lives on the main thread only (the menu items aren't `Send` anyway).
    available: RefCell<Option<String>>,
}

impl Tray {
    /// Build the tray icon and menu, reflecting the current settings.
    pub fn new(settings: &Settings) -> Result<Self, String> {
        let header = MenuItem::new(
            format!("PhaciusKey  v{}", update::CURRENT),
            false,
            None,
        );
        // The accelerator is a *hint*: the global toggle actually fires from the
        // keyboard hook (an accessory app's menu key-equivalents only work while
        // its menu is open). Unparseable shortcut string → no hint, no shortcut.
        let accel = parse_shortcut(&settings.toggle_shortcut).and_then(accelerator_for);
        let toggle = CheckMenuItem::new("Vietnamese typing", true, settings.enabled, accel);
        // Text and enabled-state follow the focused app; until the first
        // keystroke reveals one, there is nothing to toggle.
        let app_toggle = CheckMenuItem::new("Enable in current app", false, true, None);

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
        let start_login =
            CheckMenuItem::new("Start at login", true, settings.start_at_login, None);
        let update = MenuItem::new("Check for updates…", true, None);
        let report = MenuItem::new("Report an issue…", true, None);
        let quit = MenuItem::new("Quit PhaciusKey", true, None);

        let menu = Menu::new();
        let sep = || PredefinedMenuItem::separator();
        menu.append_items(&[
            &header,
            &sep(),
            &toggle,
            &app_toggle,
            &sep(),
            &method_menu,
            &tone_menu,
            &auto_restore,
            &start_login,
            &sep(),
            &update,
            &report,
            &sep(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(status_icon(settings.enabled))
            .with_tooltip("PhaciusKey — Vietnamese input")
            .build()
            .map_err(|e| e.to_string())?;

        Ok(Self {
            tray,
            toggle,
            app_toggle,
            telex,
            vni,
            modern,
            classic,
            auto_restore,
            start_login,
            update,
            report,
            quit,
            available: RefCell::new(None),
        })
    }

    /// Highlight the "Check for updates" item when a newer version is available
    /// and remember it, so a click can install that version immediately instead
    /// of just opening the releases page.
    pub fn set_update_available(&self, version: &str) {
        *self.available.borrow_mut() = Some(version.to_string());
        self.update.set_text(format!("⬇ Update to v{version} now"));
        self.update.set_enabled(true);
    }

    /// The release a click on the update item would install, when one is known.
    pub fn available_version(&self) -> Option<String> {
        self.available.borrow().clone()
    }

    /// A manual check is in flight — freeze the item so a second click can't
    /// start a second one.
    pub fn set_update_checking(&self) {
        self.update.set_text("Checking for updates…");
        self.update.set_enabled(false);
    }

    /// A download+install is in flight. On success the app relaunches; on
    /// failure `set_update_available` re-arms the item for a retry.
    pub fn set_update_installing(&self, version: &str) {
        self.update.set_text(format!("Installing v{version}…"));
        self.update.set_enabled(false);
    }

    /// Back to the resting state (up to date, or a check failed).
    pub fn set_update_idle(&self) {
        self.update.set_text("Check for updates…");
        self.update.set_enabled(true);
    }

    /// Push the current settings into the menu's checkmarks and the tray glyph.
    /// `current_app` is the app receiving keystrokes, when the hook knows it.
    pub fn refresh(&self, settings: &Settings, current_app: Option<&str>) {
        self.toggle.set_checked(settings.enabled);
        match current_app {
            Some(app) => {
                self.app_toggle.set_text(format!("Enable in {app}"));
                self.app_toggle.set_enabled(true);
                self.app_toggle.set_checked(!settings.disabled_for(Some(app)));
            }
            None => {
                self.app_toggle.set_text("Enable in current app");
                self.app_toggle.set_enabled(false);
                self.app_toggle.set_checked(true);
            }
        }
        self.telex.set_checked(settings.method == Method::Telex);
        self.vni.set_checked(settings.method == Method::Vni);
        self.modern.set_checked(settings.placement == Placement::Modern);
        self.classic.set_checked(settings.placement == Placement::Classic);
        self.auto_restore.set_checked(settings.auto_restore);
        self.start_login.set_checked(settings.start_at_login);
        // The glyph shows what a keystroke would do *right now*, so a per-app
        // disable turns it pink even while the master toggle is on.
        let effective = settings.enabled && !settings.disabled_for(current_app);
        let _ = self.tray.set_icon(Some(status_icon(effective)));
    }
}

/// The menu-item accelerator hint matching a parsed [`Shortcut`].
fn accelerator_for(sc: Shortcut) -> Option<Accelerator> {
    let mut mods = Modifiers::empty();
    if sc.ctrl {
        mods |= Modifiers::CONTROL;
    }
    if sc.shift {
        mods |= Modifiers::SHIFT;
    }
    if sc.alt {
        mods |= Modifiers::ALT;
    }
    if sc.cmd {
        mods |= Modifiers::SUPER;
    }
    let code = match sc.key {
        'a' => Code::KeyA, 'b' => Code::KeyB, 'c' => Code::KeyC, 'd' => Code::KeyD,
        'e' => Code::KeyE, 'f' => Code::KeyF, 'g' => Code::KeyG, 'h' => Code::KeyH,
        'i' => Code::KeyI, 'j' => Code::KeyJ, 'k' => Code::KeyK, 'l' => Code::KeyL,
        'm' => Code::KeyM, 'n' => Code::KeyN, 'o' => Code::KeyO, 'p' => Code::KeyP,
        'q' => Code::KeyQ, 'r' => Code::KeyR, 's' => Code::KeyS, 't' => Code::KeyT,
        'u' => Code::KeyU, 'v' => Code::KeyV, 'w' => Code::KeyW, 'x' => Code::KeyX,
        'y' => Code::KeyY, 'z' => Code::KeyZ,
        '0' => Code::Digit0, '1' => Code::Digit1, '2' => Code::Digit2, '3' => Code::Digit3,
        '4' => Code::Digit4, '5' => Code::Digit5, '6' => Code::Digit6, '7' => Code::Digit7,
        '8' => Code::Digit8, '9' => Code::Digit9,
        ' ' => Code::Space,
        _ => return None,
    };
    Some(Accelerator::new(Some(mods), code))
}

// ── Icon rendering ─────────────────────────────────────────────────────────────

/// Render the menu-bar glyph: a single neon letter on a dark rounded badge —
/// **V** (neon green) when Vietnamese typing is on, **E** (neon pink) when off.
/// Each letter glows. Drawn at 4× and box-downsampled for crisp, anti-aliased
/// edges.
fn status_icon(enabled: bool) -> Icon {
    let rgba = render_rgba(36, enabled);
    Icon::from_rgba(rgba, 36, 36).expect("valid rgba icon")
}

type Rgb = (f32, f32, f32);

/// Draw the neon badge to a straight-alpha RGBA buffer of `size`×`size` pixels.
/// Shared by the small tray glyph and the large app icon.
pub(crate) fn render_rgba(size: usize, enabled: bool) -> Vec<u8> {
    const SS: usize = 4; // supersample factor
    let big = size * SS;
    let n = big * big;

    // Neon palette: bright tube color + a hot near-white core.
    let (neon, core): (Rgb, Rgb) = if enabled {
        ((57.0, 255.0, 20.0), (214.0, 255.0, 208.0)) // green — Vietnamese
    } else {
        ((255.0, 42.0, 130.0), (255.0, 210.0, 228.0)) // pink — English
    };
    let badge: Rgb = (12.0, 12.0, 20.0); // near-black, so neon reads as neon

    let big_f = big as f32;
    let inset = big_f * 0.055;
    let radius = big_f * 0.26;
    let (x0, y0, x1, y1) = (inset, inset, big_f - inset, big_f - inset);

    // Badge mask (1 inside the rounded rect).
    let mut mask = vec![0f32; n];
    for y in 0..big {
        for x in 0..big {
            if inside_rounded(x as f32 + 0.5, y as f32 + 0.5, x0, y0, x1, y1, radius) {
                mask[y * big + x] = 1.0;
            }
        }
    }

    // Letter coverage (1 on the strokes).
    let mut cov = vec![0f32; n];
    let hw = big_f * 0.058; // stroke half-width — chunky tube
    let (ty0, ty1) = (big_f * 0.31, big_f * 0.69);
    let lw = big_f * 0.34;
    let cx = big_f * 0.5;
    let lx = cx - lw * 0.5;
    if enabled {
        // V: two diagonals meeting at the bottom center.
        stroke(&mut cov, big, lx, ty0, cx, ty1, hw);
        stroke(&mut cov, big, cx, ty1, lx + lw, ty0, hw);
    } else {
        // E: left post + top, middle, bottom bars.
        stroke(&mut cov, big, lx, ty0, lx, ty1, hw);
        stroke(&mut cov, big, lx, ty0, lx + lw, ty0, hw);
        stroke(&mut cov, big, lx, (ty0 + ty1) * 0.5, lx + lw * 0.86, (ty0 + ty1) * 0.5, hw);
        stroke(&mut cov, big, lx, ty1, lx + lw, ty1, hw);
    }

    // Glow = blurred letter coverage.
    let glow = box_blur(&cov, big, (big_f * 0.05) as usize, 3);

    // Compose: badge, then additive neon glow, then the bright letter core.
    let mut hi = vec![0u8; n * 4];
    for i in 0..n {
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
    let mut out = vec![0u8; size * size * 4];
    let samples = (SS * SS) as u32;
    for y in 0..size {
        for x in 0..size {
            let (mut sr, mut sg, mut sb, mut sa) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let i = (((y * SS + sy) * big) + (x * SS + sx)) * 4;
                    let a = hi[i + 3] as u32;
                    sr += hi[i] as u32 * a;
                    sg += hi[i + 1] as u32 * a;
                    sb += hi[i + 2] as u32 * a;
                    sa += a;
                }
            }
            let o = (y * size + x) * 4;
            out[o + 3] = (sa / samples) as u8;
            // Un-premultiply. `checked_div` guards the fully-transparent block.
            if let Some(r) = sr.checked_div(sa) {
                out[o] = r as u8;
                out[o + 1] = (sg / sa) as u8;
                out[o + 2] = (sb / sa) as u8;
            }
        }
    }

    out
}

/// Standard macOS iconset filenames and the pixel size each must be rendered
/// at (the `@2x` entries are the same logical size at double resolution).
const ICONSET_SIZES: &[(&str, u32)] = &[
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
];

/// Render the app icon (the "enabled" neon-green glyph, since the Finder/Dock
/// icon has no on/off state) at every size macOS's `.iconset` format expects,
/// writing PNGs into `dir`. Only used by `scripts/package-app.sh` via the
/// hidden `--export-iconset` flag — never invoked during normal app use.
pub(crate) fn export_iconset(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, size) in ICONSET_SIZES {
        let rgba = render_rgba(*size as usize, true);
        png_write::write_png(&dir.join(name), *size, &rgba)?;
    }
    Ok(())
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
        // Horizontal — sliding window sum (O(width) per row, radius-independent).
        for y in 0..rows {
            let base = y * stride;
            let mut sum: f32 = (0..=radius.min(stride - 1)).map(|k| buf[base + k]).sum();
            for x in 0..stride {
                tmp[base + x] = sum / window;
                if let Some(add) = (x + radius + 1 < stride).then_some(x + radius + 1) {
                    sum += buf[base + add];
                }
                if x >= radius {
                    sum -= buf[base + x - radius];
                }
            }
        }
        // Vertical.
        for x in 0..stride {
            let mut sum: f32 = (0..=radius.min(rows - 1)).map(|k| tmp[k * stride + x]).sum();
            for y in 0..rows {
                buf[y * stride + x] = sum / window;
                if y + radius + 1 < rows {
                    sum += tmp[(y + radius + 1) * stride + x];
                }
                if y >= radius {
                    sum -= tmp[(y - radius) * stride + x];
                }
            }
        }
    }
    buf
}
