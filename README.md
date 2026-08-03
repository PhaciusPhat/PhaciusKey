# phacius_vnkey

Vietnamese input method (IME). Type Vietnamese with Telex or VNI using any
keyboard — no special hardware required. **Written entirely in Rust**, one shared
engine driving thin per-OS keyboard hooks.

- **Telex** and **VNI** input methods
- Smart spell-check — only applies diacritics when the result is a valid Vietnamese syllable
- Auto-restores raw keystrokes for English and other non-Vietnamese words
- Modern and Classic tone placement (`hòa` vs `hoà`)
- Menu-bar / system-tray control (toggle, method, tone placement, auto-restore)
- Cross-platform by design: **macOS today**, **Windows** scaffold in progress

### ⬇️ [Download the latest release](https://github.com/PhaciusPhat/phacius_vnkey/releases/latest)

Grab `PhaciusKey-<version>.dmg`, drag it to Applications, done. No Rust, no toolchain.

**Requires:** macOS 13 Ventura or later · Apple Silicon (M-series) Mac.

---

## Install (macOS users)

1. Open the [**latest release**](https://github.com/PhaciusPhat/phacius_vnkey/releases/latest) and download **`PhaciusKey-<version>.dmg`** under **Assets**.
2. Double-click the downloaded `.dmg`, then drag **PhaciusKey** onto the **Applications** shortcut.
3. Open **PhaciusKey** from Applications. The **VN** icon appears in your menu bar.
4. On first launch macOS asks for **Accessibility** permission — grant it in
   System Settings → Privacy & Security → Accessibility, then **relaunch the app**
   (the keyboard hook is installed once at startup).

> **First launch blocked?** The app is ad-hoc signed, not notarized, so macOS
> may say it's from an "unidentified developer." Right-click (or Control-click)
> **PhaciusKey** in Applications → **Open** → **Open** again to allow it.
> Alternatively: `xattr -dr com.apple.quarantine /Applications/PhaciusKey.app`

---

## Architecture

One portable brain (Rust), thin per-OS bodies — all Rust.

```
┌─────────────────────────────────────────────────────────────┐
│  Shell  (apps/vnkey — cross-platform Rust)                    │
│  ┌────────────┐  ┌───────────────┐  ┌──────────────────────┐  │
│  │ tray menu  │  │ TOML settings │  │ platform keyboard    │  │
│  │ (tray-icon)│  │ (serde/toml)  │  │ hook  (trait)        │  │
│  └────────────┘  └───────────────┘  └──────────┬───────────┘  │
│                                        macOS ▸ CGEventTap      │
│                                        Windows ▸ WH_KEYBOARD_LL│
└────────────────────────────────────────────────┼─────────────┘
                                                  │ EditActions
┌─────────────────────────────────────────────────▼────────────┐
│  Core engine  (crates/vnkey-core)  —  ZERO OS dependencies    │
│   • Telex / VNI   • syllable validator   • tone placement     │
│   • composition buffer   • auto-restore   • NFC output        │
└───────────────────────────────────────────────────────────────┘
```

Only the keyboard hook and text injection differ per OS; they live behind one
trait (`apps/vnkey/src/platform/`). Everything else — engine, tray, settings —
is shared. Adding an OS means adding one `platform/<os>.rs`.

---

## Build from source (developers)

### Requirements

| Tool | Version |
|------|---------|
| Rust | 1.70+ (via [rustup](https://rustup.rs) or [asdf](https://asdf-vm.com)) |
| macOS | 13 Ventura or later (for the macOS build) |

### 1. Clone and build

```bash
git clone https://github.com/PhaciusPhat/phacius_vnkey.git
cd phacius_vnkey
cargo build -p vnkey
```

### 2. Run

```bash
cargo run -p vnkey
```

The **VN** icon appears in your menu bar. Grant **Accessibility** permission when
prompted (System Settings → Privacy & Security → Accessibility), then relaunch.

### 3. Package a distributable `.dmg` (maintainers)

```bash
bash scripts/package-app.sh
```

Builds the release binary, assembles `PhaciusKey.app`, ad-hoc signs it, and
writes `dist/PhaciusKey-<version>.dmg`. To ship notarized, replace the
`codesign --sign -` line with your Developer ID and run `xcrun notarytool`.

---

## Usage

| Action | How |
|--------|-----|
| Toggle Vietnamese on/off | Click the VN icon → **Vietnamese typing** |
| Switch input method | Click the VN icon → **Telex** / **VNI** |
| Tone placement | Click the VN icon → **Modern** / **Classic** |
| Auto-restore English | Click the VN icon → **Auto-restore English** |
| Quit | Click the VN icon → **Quit PhaciusKey** |

Settings persist to `~/Library/Application Support/vnkey/config.toml` on macOS
(the OS config dir elsewhere).

### Automatic updates

New releases are installed automatically: the app checks GitHub at startup, and
if a newer version exists it downloads the `.dmg`, verifies it with `codesign`,
replaces its own bundle, relaunches, and shows a dialog saying what happened.
Set `auto_update = false` in `config.toml` to be notified only.

> **Accessibility must be granted again after each update.** macOS ties the
> Accessibility permission to the app's code signature, and releases are ad-hoc
> signed (`codesign --sign -`), so every build has a different identity and the
> grant cannot carry over. The post-update dialog says so. Signing with a
> Developer ID certificate is what makes updates fully seamless — no code change
> is needed for that, `installer::install` already reports the live permission
> state.

### Telex cheat-sheet

| Keys | Result | Keys | Result |
|------|--------|------|--------|
| `aa` | â | `s` | sắc ´ |
| `aw` | ă | `f` | huyền ` |
| `ee` | ê | `r` | hỏi |
| `oo` | ô | `x` | ngã ~ |
| `ow` | ơ | `j` | nặng · |
| `uw` | ư | `z` | remove tone |
| `dd` | đ | | |

### VNI cheat-sheet

| Key | Effect | Key | Effect |
|-----|--------|-----|--------|
| `1` | sắc | `6` | circumflex (â ê ô) |
| `2` | huyền | `7` | horn (ơ ư) |
| `3` | hỏi | `8` | breve (ă) |
| `4` | ngã | `9` | đ |
| `5` | nặng | `0` | remove tone |

---

## Development

```bash
cargo test --all                              # engine + shell tests
cargo clippy --all-targets -- -D warnings     # lint
cargo run -p vnkey                            # run the macOS app
```

CI runs `cargo test`, `cargo clippy`, and a Windows `cargo build` on every push
via `.github/workflows/ci.yml`.

### Windows status

`apps/vnkey/src/platform/windows.rs` is a compiling scaffold (`WH_KEYBOARD_LL` +
`SendInput`) but is **untested on real hardware**. It needs verification on a
Windows machine before a Windows release — see the platform module's header.

---

## Project layout

```
phacius_vnkey/
├── crates/
│   └── vnkey-core/      # Pure Rust engine — methods, validator, tone placement
├── apps/
│   └── vnkey/           # Cross-platform Rust shell (the only OS-specific code)
│       ├── src/
│       │   ├── main.rs       # tao event loop, menu-bar accessory app
│       │   ├── config.rs     # TOML settings
│       │   ├── state.rs      # shared engine + settings
│       │   ├── tray.rs       # tray-icon / muda menu
│       │   └── platform/     # per-OS keyboard hook + injection
│       │       ├── mod.rs         # KeyboardHook trait
│       │       ├── macos.rs       # CGEventTap + CGEvent injection
│       │       └── windows.rs     # WH_KEYBOARD_LL + SendInput (scaffold)
│       └── Info.plist   # macOS bundle metadata
├── docs/specs/          # Design documents
└── scripts/
    └── package-app.sh   # Builds the .app + .dmg
```

Design reference: [tuyenvm/OpenKey](https://github.com/tuyenvm/OpenKey).

---

## License

MIT © 2026 Phacius
