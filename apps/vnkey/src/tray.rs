use std::io;
use std::path::Path;

use tray_icon::menu::{Menu, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::config::Settings;
use crate::png_write;

/// The icon, and nothing else by default: what used to be a native menu is now
/// a panel we draw, so that it can share the settings window's theme.
pub struct Tray {
    tray: TrayIcon,
    fallback: Option<Fallback>,
}

/// Attached only when the panel could not be built. Without the native menu
/// there is no other way to reach settings or to quit, and a menu-bar-only
/// application the user cannot quit is worse than an unstyled menu.
pub struct Fallback {
    pub settings: MenuItem,
    pub quit: MenuItem,
}

impl Tray {
    pub fn new(settings: &Settings) -> Result<Self, String> {
        let tray = TrayIconBuilder::new()
            .with_icon(status_icon(settings.enabled).ok_or("failed to render the tray icon")?)
            .with_icon_as_template(true)
            .with_tooltip("PhaciusKey — Vietnamese input")
            .build()
            .map_err(|e| e.to_string())?;

        Ok(Self {
            tray,
            fallback: None,
        })
    }

    pub fn fallback(&self) -> Option<&Fallback> {
        self.fallback.as_ref()
    }

    /// Attaching a menu means the icon opens it instead of reporting the click,
    /// which is exactly what is wanted once there is no panel to open.
    pub fn attach_fallback_menu(&mut self) -> Result<(), String> {
        let err = |e: tray_icon::menu::Error| e.to_string();

        let settings = MenuItem::new("Settings…", true, None);
        let quit = MenuItem::new("Quit PhaciusKey", true, None);
        let menu = Menu::new();
        menu.append_items(&[&settings, &quit]).map_err(err)?;
        self.tray.set_menu(Some(Box::new(menu)));

        self.fallback = Some(Fallback { settings, quit });
        Ok(())
    }

    pub fn refresh(&self, current_app: Option<&str>) {
        let effective = crate::state::vietnamese_active();

        let status = if crate::platform::secure_input_active() {
            "⚠ Secure input on — Vietnamese paused (password field?)".to_string()
        } else {
            match current_app {
                Some(app) => format!(
                    "{app} — typing {}",
                    if effective { "Vietnamese" } else { "English" }
                ),
                None => "Type anywhere to begin".to_string(),
            }
        };
        let _ = self
            .tray
            .set_tooltip(Some(format!("PhaciusKey — {status}")));

        let _ = self.tray.set_icon(status_icon(effective));
        // `set_icon` re-sends the image with `is_template: false`, so the flag
        // has to be put back or the second icon of a session loses the theme.
        self.tray.set_icon_as_template(true);
    }
}

type Rgb = (f32, f32, f32);

/// macOS draws a template image from its alpha alone — black on a light menu
/// bar, white on a dark one — and ignores these channels; they are what every
/// other platform draws instead. The exported iconset is never a template, so
/// it keeps the app's own colour.
const MENU_BAR: Rgb = (255.0, 255.0, 255.0);
const APP_ICON: Rgb = (52.0, 199.0, 89.0);

fn status_icon(enabled: bool) -> Option<Icon> {
    let rgba = render_rgba(36, enabled, MENU_BAR);
    Icon::from_rgba(rgba, 36, 36).ok()
}

pub(crate) fn render_rgba(size: usize, enabled: bool, color: Rgb) -> Vec<u8> {
    const SS: usize = 4;
    let big = size * SS;
    let n = big * big;
    let big_f = big as f32;

    let mut cov = vec![0f32; n];
    let hw = big_f * 0.075;
    let (ty0, ty1) = (big_f * 0.14, big_f * 0.86);
    let lw = big_f * 0.56;
    let cx = big_f * 0.5;
    let lx = cx - lw * 0.5;
    if enabled {
        stroke(&mut cov, big, lx, ty0, cx, ty1, hw);
        stroke(&mut cov, big, cx, ty1, lx + lw, ty0, hw);
    } else {
        stroke(&mut cov, big, lx, ty0, lx, ty1, hw);
        stroke(&mut cov, big, lx, ty0, lx + lw, ty0, hw);
        stroke(
            &mut cov,
            big,
            lx,
            (ty0 + ty1) * 0.5,
            lx + lw * 0.86,
            (ty0 + ty1) * 0.5,
            hw,
        );
        stroke(&mut cov, big, lx, ty1, lx + lw, ty1, hw);
    }

    let mut out = vec![0u8; size * size * 4];
    let samples = (SS * SS) as f32;
    for y in 0..size {
        for x in 0..size {
            let mut sum = 0f32;
            for sy in 0..SS {
                for sx in 0..SS {
                    sum += cov[(y * SS + sy) * big + (x * SS + sx)];
                }
            }
            let o = (y * size + x) * 4;
            out[o] = color.0 as u8;
            out[o + 1] = color.1 as u8;
            out[o + 2] = color.2 as u8;
            out[o + 3] = (sum / samples * 255.0).round() as u8;
        }
    }

    out
}

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

pub(crate) fn export_iconset(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, size) in ICONSET_SIZES {
        let rgba = render_rgba(*size as usize, true, APP_ICON);
        png_write::write_png(&dir.join(name), *size, &rgba)?;
    }
    Ok(())
}

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
