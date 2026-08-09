use std::cell::RefCell;
use std::io;
use std::path::Path;

use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::config::{parse_shortcut, Method, Placement, Settings, Shortcut};
use crate::png_write;
use crate::update;

pub struct Tray {
    tray: TrayIcon,
    status: MenuItem,
    pub toggle: CheckMenuItem,
    method_menu: Submenu,
    tone_menu: Submenu,
    pub telex: CheckMenuItem,
    pub vni: CheckMenuItem,
    pub modern: CheckMenuItem,
    pub classic: CheckMenuItem,
    pub auto_restore: CheckMenuItem,
    pub settings: MenuItem,
    pub update: MenuItem,
    pub report: MenuItem,
    pub quit: MenuItem,
    available: RefCell<Option<String>>,
}

impl Tray {
    pub fn new(settings: &Settings) -> Result<Self, String> {
        let err = |e: tray_icon::menu::Error| e.to_string();

        let header = MenuItem::new(format!("PhaciusKey  v{}", update::CURRENT), false, None);
        let status = MenuItem::new("Type anywhere to begin", false, None);

        let accel = parse_shortcut(&settings.toggle_shortcut).and_then(accelerator_for);
        let toggle = CheckMenuItem::new("Vietnamese typing", true, settings.enabled, accel);

        let telex = CheckMenuItem::new("Telex", true, settings.method == Method::Telex, None);
        let vni = CheckMenuItem::new("VNI", true, settings.method == Method::Vni, None);
        let method_menu = Submenu::new(method_title(settings), true);
        method_menu.append_items(&[&telex, &vni]).map_err(err)?;

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
        let tone_menu = Submenu::new(tone_title(settings), true);
        tone_menu.append_items(&[&modern, &classic]).map_err(err)?;

        let auto_restore = CheckMenuItem::new(
            "Auto-restore English words",
            true,
            settings.auto_restore,
            None,
        );

        let settings_item = MenuItem::new("Settings…", true, None);

        let report = MenuItem::new("Report an issue…", true, None);
        let help_menu = Submenu::new("Help", true);
        help_menu
            .append_items(&[
                &MenuItem::new(
                    "Telex:  aa â · ee ê · oo ô · ow ơ · uw ư · aw ă · dd đ",
                    false,
                    None,
                ),
                &MenuItem::new(
                    "Telex tones:  s sắc · f huyền · r hỏi · x ngã · j nặng · z xoá",
                    false,
                    None,
                ),
                &MenuItem::new(
                    "VNI:  6 â ê ô · 7 ơ ư · 8 ă · 9 đ · 1–5 tones · 0 xoá",
                    false,
                    None,
                ),
                &MenuItem::new(
                    "Esc:  restore the word exactly as you typed it",
                    false,
                    None,
                ),
                &PredefinedMenuItem::separator(),
                &report,
            ])
            .map_err(err)?;

        let update = MenuItem::new("Check for updates…", true, None);
        let quit = MenuItem::new("Quit PhaciusKey", true, None);

        let menu = Menu::new();
        let sep = PredefinedMenuItem::separator;
        menu.append_items(&[
            &header,
            &status,
            &sep(),
            &toggle,
            &sep(),
            &method_menu,
            &tone_menu,
            &auto_restore,
            &sep(),
            &settings_item,
            &help_menu,
            &sep(),
            &update,
            &sep(),
            &quit,
        ])
        .map_err(err)?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(status_icon(settings.enabled).ok_or("failed to render the tray icon")?)
            .with_tooltip("PhaciusKey — Vietnamese input")
            .build()
            .map_err(|e| e.to_string())?;

        Ok(Self {
            tray,
            status,
            toggle,
            method_menu,
            tone_menu,
            telex,
            vni,
            modern,
            classic,
            auto_restore,
            settings: settings_item,
            update,
            report,
            quit,
            available: RefCell::new(None),
        })
    }

    pub fn set_update_available(&self, version: &str) {
        *self.available.borrow_mut() = Some(version.to_string());
        self.update.set_text(format!("⬇ Update to v{version} now"));
        self.update.set_enabled(true);
    }

    pub fn available_version(&self) -> Option<String> {
        self.available.borrow().clone()
    }

    pub fn set_update_checking(&self) {
        self.update.set_text("Checking for updates…");
        self.update.set_enabled(false);
    }

    pub fn set_update_installing(&self, version: &str) {
        self.update.set_text(format!("Installing v{version}…"));
        self.update.set_enabled(false);
    }

    pub fn set_update_idle(&self) {
        self.update.set_text("Check for updates…");
        self.update.set_enabled(true);
    }

    pub fn refresh(&self, settings: &Settings, current_app: Option<&str>) {
        let effective = settings.vietnamese_on(current_app);

        self.toggle.set_checked(settings.enabled);

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
        self.status.set_text(&status);
        let _ = self
            .tray
            .set_tooltip(Some(format!("PhaciusKey — {status}")));

        let _ = self
            .toggle
            .set_accelerator(parse_shortcut(&settings.toggle_shortcut).and_then(accelerator_for));

        self.method_menu.set_text(method_title(settings));
        self.tone_menu.set_text(tone_title(settings));
        self.telex.set_checked(settings.method == Method::Telex);
        self.vni.set_checked(settings.method == Method::Vni);
        self.modern
            .set_checked(settings.placement == Placement::Modern);
        self.classic
            .set_checked(settings.placement == Placement::Classic);
        self.auto_restore.set_checked(settings.auto_restore);

        let _ = self.tray.set_icon(status_icon(effective));
    }
}

fn method_title(settings: &Settings) -> String {
    format!(
        "Input method  ·  {}",
        match settings.method {
            Method::Telex => "Telex",
            Method::Vni => "VNI",
        }
    )
}

fn tone_title(settings: &Settings) -> String {
    format!(
        "Tone placement  ·  {}",
        match settings.placement {
            Placement::Modern => "Modern",
            Placement::Classic => "Classic",
        }
    )
}

#[rustfmt::skip]
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

fn status_icon(enabled: bool) -> Option<Icon> {
    let rgba = render_rgba(36, enabled);
    Icon::from_rgba(rgba, 36, 36).ok()
}

type Rgb = (f32, f32, f32);

pub(crate) fn render_rgba(size: usize, enabled: bool) -> Vec<u8> {
    const SS: usize = 4;
    let big = size * SS;
    let n = big * big;
    let big_f = big as f32;

    let color: Rgb = if enabled {
        (52.0, 199.0, 89.0)
    } else {
        (255.0, 59.0, 48.0)
    };

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
        let rgba = render_rgba(*size as usize, true);
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
