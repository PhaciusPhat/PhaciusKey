# Update alerts, modifier-only shortcuts, and an honest panel switch — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the toggle shortcut be two or three modifiers alone, replace the four
`osascript` update dialogs with a themed window, and stop the tray panel describing its
machine-wide switch as per-application.

**Architecture:** Three independent parts over one Rust + webview desktop app. Part A
changes a parsed value type, adds a pure state machine, and feeds it from both keyboard
hooks. Part B adds a third webview surface beside the existing two. Part C moves one
string into the payload so it can be tested.

**Tech Stack:** Rust 2021, `tao` 0.30 (windowing), `wry` 0.56 (webview), `serde_json`,
CoreGraphics event tap on macOS, `SetWindowsHookEx` on Windows.

**Spec:** `docs/specs/2026-08-09-update-alerts-and-modifier-shortcuts-design.md`

**Branch:** `feat/update-alerts-shortcuts`

## Global Constraints

- **No allocation on the keystroke path.** `Engine::process`, `Engine::backspace`, and
  everything the hook callbacks reach run on every keystroke. macOS disables an event tap
  whose callback overruns; Windows removes a hook past `LowLevelHooksTimeout`. No `clone`,
  no `String`, no `Vec` on that path.
- **Comments state constraints only.** Never comment to explain a bug fixed, to argue a
  change is correct, or to say what the code used to do. A comment states something the
  reader cannot derive from the code — an ABI rule, a platform quirk, a safety
  precondition — and stops. See `.claude/skills/commenting-only-constraints/SKILL.md`.
- **Follow `.claude/skills/rust-best-practices/`.** Invoke it before writing code.
- **`unwrap`/`expect` are denied outside `cfg(test)`** by the workspace `[lints]` table.
- **Every task ends green.** All three, because `.github/workflows/ci.yml` runs all three:
  ```sh
  cargo fmt --check
  cargo test --workspace
  cargo clippy --all-targets --all-features -- -D warnings
  ```
- **Shortcut key counts:** 1–2 modifiers plus a key, or 2–3 modifiers alone. Never a lone
  modifier, never four modifiers.
- **Stored shortcut format is unchanged:** a lowercase `+`-joined string such as
  `ctrl+shift+v` or `ctrl+shift`. No config migration.

---

## File Structure

**Part A — shortcuts**
- `apps/vnkey/src/config.rs` — `Shortcut.key` becomes `Option<char>`; `ChordWatch` and its
  modifier bitmask constants are added here, beside the parsing they belong to.
- `apps/vnkey/src/state.rs` — `AtomicU32` cache of the parsed shortcut.
- `apps/vnkey/src/platform/macos.rs` — `FlagsChanged` in the tap mask and its branch.
- `apps/vnkey/src/platform/windows.rs` — modifier-only path in the hook.
- `apps/vnkey/src/ui/ipc.rs` — `ShortcutCapture.code` becomes `Option<String>`.
- `apps/vnkey/src/ui/assets/settings.js` — recorder commits on release.

**Part B — alerts**
- `apps/vnkey/src/update.rs` — `Notice`, `Action`, four constructors.
- `apps/vnkey/src/ui/alert.rs` — the window (new).
- `apps/vnkey/src/ui/assets/alert.{html,css,js}` — the page (new).
- `apps/vnkey/src/ui/mod.rs` — `Surface::Alert`, module wiring.
- `apps/vnkey/src/ui/ipc.rs` — `open_accessibility`, `open_releases`.
- `apps/vnkey/src/platform/mod.rs` + `macos.rs` — `open_accessibility_settings`.
- `apps/vnkey/src/installer.rs` — four functions deleted.
- `apps/vnkey/src/main.rs` — alert ownership, `Resize` routed by surface, `--show-alert`.

**Part C — panel**
- `apps/vnkey/src/ui/payload.rs` — `excluded_summary`.
- `apps/vnkey/src/ui/assets/panel.{html,js}` — subtitle and warning row.

---

### Task 1: Shortcut accepts modifiers alone

**Files:**
- Modify: `apps/vnkey/src/config.rs:190-245` (`Shortcut`, `MODIFIERS`, `parse_shortcut`)
- Modify: `apps/vnkey/src/config.rs:302-357` (`shortcut_from_event`, `shortcut_parts`)
- Modify: `apps/vnkey/src/config.rs:416-432` (two existing tests)
- Modify: `apps/vnkey/src/platform/macos.rs:264-281`, `apps/vnkey/src/platform/windows.rs:180-194` (call sites)

**Interfaces:**
- Produces: `Shortcut { ctrl: bool, shift: bool, alt: bool, cmd: bool, key: Option<char> }`;
  `parse_shortcut(&str) -> Option<Shortcut>`;
  `shortcut_from_event(ctrl: bool, alt: bool, shift: bool, cmd: bool, code: Option<&str>) -> Option<String>`;
  `shortcut_parts(&str) -> Vec<String>` unchanged in signature.

- [ ] **Step 1: Invert the two tests that pin the old rule**

In `config.rs`, `rejects_garbage_and_modifierless_keys` currently asserts
`parse_shortcut("ctrl+shift")` is `None`. Remove that line from it, and replace
`a_combination_holds_two_or_three_keys` with both shapes:

```rust
    #[test]
    fn rejects_garbage_and_modifierless_keys() {
        assert_eq!(parse_shortcut(""), None);
        assert_eq!(parse_shortcut("v"), None);
        assert_eq!(parse_shortcut("ctrl+vv"), None);
        assert_eq!(parse_shortcut("ctrl+ß"), None);
        assert_eq!(parse_shortcut("hyper+v"), None);
    }

    #[test]
    fn a_combination_holds_two_or_three_keys() {
        assert!(parse_shortcut("ctrl+v").is_some());
        assert!(parse_shortcut("ctrl+shift+v").is_some());
        assert!(parse_shortcut("ctrl+alt+shift+v").is_none());
        assert!(parse_shortcut("ctrl+alt+shift+cmd+v").is_none());
    }

    #[test]
    fn modifiers_alone_need_two_of_them() {
        assert!(parse_shortcut("ctrl+shift").is_some());
        assert!(parse_shortcut("ctrl+alt+shift").is_some());
        assert_eq!(parse_shortcut("shift"), None);
        assert_eq!(parse_shortcut("ctrl"), None);
        assert_eq!(parse_shortcut("ctrl+alt+shift+cmd"), None);
    }

    #[test]
    fn modifiers_alone_render_as_keycaps() {
        assert_eq!(shortcut_parts("ctrl+shift"), ["⌃", "⇧"]);
    }

    #[test]
    fn a_recorded_release_becomes_a_modifier_only_shortcut() {
        assert_eq!(
            shortcut_from_event(true, false, true, false, None).as_deref(),
            Some("ctrl+shift")
        );
        assert_eq!(shortcut_from_event(true, false, false, false, None), None);
        assert_eq!(shortcut_from_event(true, true, true, true, None), None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p vnkey config::tests 2>&1 | tail -30`
Expected: FAIL — `modifiers_alone_need_two_of_them` and the two new ones. The
`shortcut_from_event` test fails to compile, since `code` is not yet `Option`.

- [ ] **Step 3: Change the field and the validity rule**

In `config.rs`, replace the `Shortcut` struct, the `MODIFIERS` constant, and the tail of
`parse_shortcut`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub cmd: bool,
    pub key: Option<char>,
}

/// A key needs one or two modifiers to escape ordinary typing. Modifiers alone need two,
/// because a single held modifier occurs constantly while typing.
const MODIFIERS_WITH_KEY: std::ops::RangeInclusive<usize> = 1..=2;
const MODIFIERS_ALONE: std::ops::RangeInclusive<usize> = 2..=3;

impl Shortcut {
    fn modifier_count(&self) -> usize {
        [self.ctrl, self.shift, self.alt, self.cmd]
            .into_iter()
            .filter(|held| *held)
            .count()
    }
}

fn modifiers_fit(count: usize, key: Option<char>) -> bool {
    match key {
        Some(_) => MODIFIERS_WITH_KEY.contains(&count),
        None => MODIFIERS_ALONE.contains(&count),
    }
}
```

In `parse_shortcut`, initialise `key: None`, assign `sc.key = Some(' ')` and
`sc.key = Some(c)` in the two key arms, and replace the final line:

```rust
    modifiers_fit(sc.modifier_count(), sc.key).then_some(sc)
```

- [ ] **Step 4: Make `shortcut_from_event` take an optional code**

```rust
pub fn shortcut_from_event(
    ctrl: bool,
    alt: bool,
    shift: bool,
    cmd: bool,
    code: Option<&str>,
) -> Option<String> {
    let key = match code {
        Some(code) => Some(match code.as_bytes() {
            [b'K', b'e', b'y', c] if c.is_ascii_uppercase() => c.to_ascii_lowercase() as char,
            [b'D', b'i', b'g', b'i', b't', d] if d.is_ascii_digit() => *d as char,
            _ if code == "Space" => ' ',
            _ => return None,
        }),
        None => None,
    };

    let held = [ctrl, alt, shift, cmd].into_iter().filter(|h| *h).count();
    if !modifiers_fit(held, key) {
        return None;
    }

    let mut parts: Vec<&str> = Vec::new();
    for (held, name) in [(ctrl, "ctrl"), (alt, "alt"), (shift, "shift"), (cmd, "cmd")] {
        if held {
            parts.push(name);
        }
    }
    let key = key.map(|key| if key == ' ' { "space".to_string() } else { key.to_string() });
    if let Some(key) = &key {
        parts.push(key);
    }
    Some(parts.join("+"))
}
```

- [ ] **Step 5: Make `shortcut_parts` tolerate a missing key**

Replace the `parts.push(match sc.key { … })` tail with:

```rust
    if let Some(key) = sc.key {
        parts.push(match key {
            ' ' => "Space".to_string(),
            key => key.to_uppercase().to_string(),
        });
    }
    parts
```

- [ ] **Step 6: Fix the two hook call sites so the workspace compiles**

`platform/macos.rs`, in `is_toggle_shortcut`, replace `let Some(want) = keycode_for(sc.key)`:

```rust
    let Some(want) = sc.key.and_then(keycode_for) else {
        return false;
    };
```

`platform/windows.rs`, in `is_toggle_shortcut`, replace the `windows_vk` line:

```rust
    if sc.key.and_then(config::windows_vk) != Some(vk) {
        return false;
    }
```

Both now return `false` for a modifier-only shortcut, which Tasks 4 and 5 handle.

- [ ] **Step 7: Fix the remaining test fixtures**

The tests at `config.rs:379-432` construct `Shortcut { … key: 'v' }` literals and call
`shortcut_from_event(..., "KeyV")`. Wrap each key in `Some(...)`, and each `code` argument
in `Some(...)`. In `a_recorded_key_press_becomes_a_canonical_shortcut` and the roundtrip
property test, pass `Some(code)`.

- [ ] **Step 8: Run the full suite**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: PASS.

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: no warnings.

- [ ] **Step 9: Commit**

```bash
git add apps/vnkey/src/config.rs apps/vnkey/src/platform/macos.rs apps/vnkey/src/platform/windows.rs
git commit -m "feat(config): let a shortcut be two or three modifiers alone"
```

---

### Task 2: The `ChordWatch` firing rule

**Files:**
- Modify: `apps/vnkey/src/config.rs` (append after `shortcut_parts`)

**Interfaces:**
- Consumes: nothing.
- Produces: `pub const MOD_CTRL: u8 = 1; MOD_SHIFT: u8 = 2; MOD_ALT: u8 = 4; MOD_CMD: u8 = 8;`
  `Shortcut::modifier_mask(&self) -> u8`;
  `ChordWatch::default()`, `ChordWatch::modifiers(&mut self, held: u8, target: u8) -> bool`,
  `ChordWatch::interrupted(&mut self)`.

**Note on the spec.** The spec sketches this with a single `armed` flag. That sketch
re-arms when a third modifier is pressed and then released — `⌃↓ ⇧↓ ⌥↓ ⌥↑ ⇧↑ ⌃↑` would
fire, which the spec's own test list forbids. A second flag is required, cleared only when
every modifier is up.

- [ ] **Step 1: Write the failing tests**

```rust
    fn watch_sequence(target: u8, steps: &[u8]) -> usize {
        let mut watch = ChordWatch::default();
        steps.iter().filter(|held| watch.modifiers(**held, target)).count()
    }

    const CS: u8 = MOD_CTRL | MOD_SHIFT;

    #[test]
    fn a_clean_hold_and_release_fires_once() {
        assert_eq!(watch_sequence(CS, &[MOD_CTRL, CS, MOD_CTRL, 0]), 1);
    }

    #[test]
    fn either_modifier_may_be_released_first() {
        assert_eq!(watch_sequence(CS, &[MOD_SHIFT, CS, MOD_SHIFT, 0]), 1);
    }

    #[test]
    fn a_third_modifier_spoils_the_gesture_even_once_released() {
        assert_eq!(
            watch_sequence(CS, &[MOD_CTRL, CS, CS | MOD_ALT, CS, MOD_CTRL, 0]),
            0
        );
    }

    #[test]
    fn a_key_pressed_between_spoils_the_gesture() {
        let mut watch = ChordWatch::default();
        assert!(!watch.modifiers(MOD_CTRL, CS));
        assert!(!watch.modifiers(CS, CS));
        watch.interrupted();
        assert!(!watch.modifiers(MOD_CTRL, CS));
        assert!(!watch.modifiers(0, CS));
    }

    #[test]
    fn a_partial_hold_never_fires() {
        assert_eq!(watch_sequence(CS, &[MOD_CTRL, 0]), 0);
    }

    #[test]
    fn the_gesture_can_be_repeated() {
        let mut watch = ChordWatch::default();
        for _ in 0..2 {
            assert!(!watch.modifiers(MOD_CTRL, CS));
            assert!(!watch.modifiers(CS, CS));
            assert!(watch.modifiers(0, CS));
        }
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p vnkey config::tests 2>&1 | tail -20`
Expected: FAIL — `ChordWatch` and the `MOD_*` constants do not exist.

- [ ] **Step 3: Implement**

Append to `config.rs`:

```rust
pub const MOD_CTRL: u8 = 1;
pub const MOD_SHIFT: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_CMD: u8 = 8;

impl Shortcut {
    pub fn modifier_mask(&self) -> u8 {
        (if self.ctrl { MOD_CTRL } else { 0 })
            | (if self.shift { MOD_SHIFT } else { 0 })
            | (if self.alt { MOD_ALT } else { 0 })
            | (if self.cmd { MOD_CMD } else { 0 })
    }
}

/// Tracks a modifier-only shortcut across events, which cannot fire on press: the
/// combination is a prefix of every `⌃⇧X` in every application.
///
/// `poisoned` outlives `armed` deliberately. Releasing a third modifier returns the held
/// set to the target, and without it the gesture would re-arm mid-flight.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChordWatch {
    armed: bool,
    poisoned: bool,
}

impl ChordWatch {
    /// Returns true exactly once per clean gesture, on the release that empties the set.
    pub fn modifiers(&mut self, held: u8, target: u8) -> bool {
        if held == 0 {
            let fired = self.armed;
            self.armed = false;
            self.poisoned = false;
            return fired;
        }
        if held & !target != 0 {
            self.armed = false;
            self.poisoned = true;
            return false;
        }
        if held == target && !self.poisoned {
            self.armed = true;
        }
        false
    }

    pub fn interrupted(&mut self) {
        self.armed = false;
        self.poisoned = true;
    }
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p vnkey config::tests 2>&1 | tail -20`
Expected: PASS.

Run: `cargo clippy --all-targets --all-features -- -D warnings`

- [ ] **Step 5: Commit**

```bash
git add apps/vnkey/src/config.rs
git commit -m "feat(config): add the modifier-only firing rule"
```

---

### Task 3: Cache the parsed shortcut off the keystroke path

**Files:**
- Modify: `apps/vnkey/src/state.rs:1-55` (statics), `:40-48` (`init`), `:110-121` (`update`)
- Modify: `apps/vnkey/src/platform/macos.rs:264-281`
- Modify: `apps/vnkey/src/platform/windows.rs:180-194`

**Interfaces:**
- Consumes: `Shortcut`, `parse_shortcut` from Task 1.
- Produces: `state::toggle_shortcut() -> Option<Shortcut>`, allocation-free.

- [ ] **Step 1: Write the failing test**

The packing is pure, so it is tested where it lives. Add a test module to the bottom of
`state.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_valid_shortcut_survives_the_round_trip() {
        for text in [
            "ctrl+v", "ctrl+shift+v", "cmd+space", "alt+z", "cmd+2",
            "ctrl+shift", "ctrl+alt+shift", "alt+cmd",
        ] {
            let parsed = crate::config::parse_shortcut(text);
            assert!(parsed.is_some(), "{text} should parse");
            assert_eq!(unpack(pack(parsed)), parsed, "{text} should round-trip");
        }
    }

    #[test]
    fn an_unset_shortcut_packs_to_zero() {
        assert_eq!(pack(None), 0);
        assert_eq!(unpack(0), None);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p vnkey state:: 2>&1 | tail -20`
Expected: FAIL — `pack` and `unpack` do not exist.

- [ ] **Step 3: Implement the cache**

Add to the top of `state.rs`, beside `RECORDING_SHORTCUT`:

```rust
use std::sync::atomic::AtomicU32;

use crate::config::Shortcut;

/// The parsed toggle shortcut, read by the keyboard hooks on every event. A packed atomic
/// rather than the `Settings` behind the mutex: reading it must not lock, clone or parse.
///
/// Layout: bits 0–3 the modifiers, bit 4 set when a key is present, bits 8+ that key.
/// Zero means no usable shortcut, which is unambiguous because every valid shortcut holds
/// at least one modifier.
static TOGGLE: AtomicU32 = AtomicU32::new(0);

fn pack(shortcut: Option<Shortcut>) -> u32 {
    let Some(sc) = shortcut else { return 0 };
    let mut bits = u32::from(sc.modifier_mask()) & 0xF;
    if let Some(key) = sc.key {
        bits |= 1 << 4;
        bits |= (key as u32) << 8;
    }
    bits
}

fn unpack(bits: u32) -> Option<Shortcut> {
    if bits == 0 {
        return None;
    }
    Some(Shortcut {
        ctrl: bits & u32::from(crate::config::MOD_CTRL) != 0,
        shift: bits & u32::from(crate::config::MOD_SHIFT) != 0,
        alt: bits & u32::from(crate::config::MOD_ALT) != 0,
        cmd: bits & u32::from(crate::config::MOD_CMD) != 0,
        key: (bits & (1 << 4) != 0).then(|| char::from_u32(bits >> 8).unwrap_or('\0')),
    })
}

fn cache_toggle(settings: &Settings) {
    TOGGLE.store(
        pack(crate::config::parse_shortcut(&settings.toggle_shortcut)),
        Ordering::Relaxed,
    );
}

pub fn toggle_shortcut() -> Option<Shortcut> {
    unpack(TOGGLE.load(Ordering::Relaxed))
}
```

- [ ] **Step 4: Write the cache whenever settings change**

In `init`, before `SHELL.set`, add `cache_toggle(&settings);`.
In `update`, inside the `with` closure after `f(&mut s.settings)`, add
`cache_toggle(&s.settings);`.

`toggle_vietnamese` only flips `enabled` and needs no cache write.

- [ ] **Step 5: Read the cache from both hooks**

`platform/macos.rs`, `is_toggle_shortcut` — replace the settings lookup:

```rust
    let Some(sc) = state::toggle_shortcut() else {
        return false;
    };
```

`platform/windows.rs`, `is_toggle_shortcut` — the same replacement. Remove the now-unused
`config::parse_shortcut` import if clippy flags it.

- [ ] **Step 6: Run and verify**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: PASS.

Run: `cargo clippy --all-targets --all-features -- -D warnings`

- [ ] **Step 7: Commit**

```bash
git add apps/vnkey/src/state.rs apps/vnkey/src/platform/macos.rs apps/vnkey/src/platform/windows.rs
git commit -m "perf(state): read the toggle shortcut without cloning settings"
```

---

### Task 4: macOS fires a modifier-only shortcut

**Files:**
- Modify: `apps/vnkey/src/platform/macos.rs:33-38` (event-type constants), `:118` (mask),
  `:145-192` (callback dispatch), `:264-281` (`is_toggle_shortcut`)

**Interfaces:**
- Consumes: `state::toggle_shortcut`, `ChordWatch`, `MOD_*` from Tasks 2 and 3.
- Produces: nothing for later tasks.

- [ ] **Step 1: Add the event type and put it in the mask**

Beside the other `ET_` constants:

```rust
const ET_FLAGS_CHANGED: u32 = 12;
```

At line 118:

```rust
    let mask: u64 = (1 << ET_KEY_DOWN)
        | (1 << ET_FLAGS_CHANGED)
        | (1 << ET_LEFT_MOUSE_DOWN)
        | (1 << ET_RIGHT_MOUSE_DOWN);
```

- [ ] **Step 2: Add the watch and the mask helper**

Near the top of the file, beside `TAP_PORT`:

```rust
thread_local! {
    static CHORD: Cell<config::ChordWatch> = const { Cell::new(config::ChordWatch::new()) };
}

fn held_mask(flags: CGEventFlags) -> u8 {
    (if flags.contains(CGEventFlags::CGEventFlagControl) { config::MOD_CTRL } else { 0 })
        | (if flags.contains(CGEventFlags::CGEventFlagShift) { config::MOD_SHIFT } else { 0 })
        | (if flags.contains(CGEventFlags::CGEventFlagAlternate) { config::MOD_ALT } else { 0 })
        | (if flags.contains(CGEventFlags::CGEventFlagCommand) { config::MOD_CMD } else { 0 })
}
```

`Cell` needs a `const` constructor, so add to `config.rs` beside `ChordWatch`:

```rust
impl ChordWatch {
    pub const fn new() -> Self {
        Self { armed: false, poisoned: false }
    }
}
```

- [ ] **Step 3: Handle the flags event in the dispatch**

In `tap_callback`'s `match etype`, add an arm before `ET_KEY_DOWN => {}`:

```rust
        ET_FLAGS_CHANGED => {
            // Never swallowed: another application would be left believing a modifier is
            // still held.
            let cg = ManuallyDrop::new(CGEvent::from_ptr(event as *mut _));
            if modifier_only_toggle_fired(held_mask(cg.get_flags())) {
                state::toggle_vietnamese();
            }
            return event;
        }
```

And in the `ET_LEFT_MOUSE_DOWN | ET_RIGHT_MOUSE_DOWN` arm, before `state::reset()`:

```rust
            CHORD.with(|chord| {
                let mut watch = chord.get();
                watch.interrupted();
                chord.set(watch);
            });
```

- [ ] **Step 4: Add the helper, and interrupt on key-down**

```rust
fn modifier_only_toggle_fired(held: u8) -> bool {
    if state::shortcut_recording() {
        return false;
    }
    let Some(sc) = state::toggle_shortcut() else {
        return false;
    };
    if sc.key.is_some() {
        return false;
    }
    CHORD.with(|chord| {
        let mut watch = chord.get();
        let fired = watch.modifiers(held, sc.modifier_mask());
        chord.set(watch);
        fired
    })
}
```

In the key-down path, immediately after the `SYNTHETIC_MARKER` early return, add:

```rust
    CHORD.with(|chord| {
        let mut watch = chord.get();
        watch.interrupted();
        chord.set(watch);
    });
```

- [ ] **Step 5: Build and verify**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: PASS.

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
git add apps/vnkey/src/config.rs apps/vnkey/src/platform/macos.rs
git commit -m "feat(macos): fire the toggle on a clean modifier release"
```

---

### Task 5: Windows fires a modifier-only shortcut

**Files:**
- Modify: `apps/vnkey/src/platform/windows.rs:60-110` (hook body), `:175-204` (helpers)

**Interfaces:**
- Consumes: the same as Task 4.

**Note.** `GetAsyncKeyState` may not yet reflect the key the hook is currently reporting,
so the current event's own bit is forced rather than read.

- [ ] **Step 1: Add the watch and the mask helper**

```rust
static CHORD: Mutex<config::ChordWatch> = Mutex::new(config::ChordWatch::new());

/// The watch is only consulted for a modifier-only shortcut, so the common case — a
/// shortcut with a key — never takes this lock. The hook runs on every keystroke and is
/// watchdogged by `LowLevelHooksTimeout`.
fn modifier_only_target() -> Option<u8> {
    if state::shortcut_recording() {
        return None;
    }
    let sc = state::toggle_shortcut()?;
    sc.key.is_none().then(|| sc.modifier_mask())
}

fn chord_interrupted() {
    if modifier_only_target().is_none() {
        return;
    }
    if let Ok(mut watch) = CHORD.lock() {
        watch.interrupted();
    }
}

/// The modifier bit a virtual-key code carries, if it is a modifier at all.
fn modifier_bit(vk: u16) -> Option<u8> {
    Some(match vk {
        v if v == VK_CONTROL.0 || v == VK_LCONTROL.0 || v == VK_RCONTROL.0 => config::MOD_CTRL,
        v if v == VK_SHIFT.0 || v == VK_LSHIFT.0 || v == VK_RSHIFT.0 => config::MOD_SHIFT,
        v if v == VK_MENU.0 || v == VK_LMENU.0 || v == VK_RMENU.0 => config::MOD_ALT,
        v if v == VK_LWIN.0 || v == VK_RWIN.0 => config::MOD_CMD,
        _ => return None,
    })
}

fn held_mask(vk: u16, is_key_up: bool) -> u8 {
    let mut mask = (if down(VK_CONTROL) { config::MOD_CTRL } else { 0 })
        | (if down(VK_SHIFT) { config::MOD_SHIFT } else { 0 })
        | (if down(VK_MENU) { config::MOD_ALT } else { 0 })
        | (if down(VK_LWIN) || down(VK_RWIN) { config::MOD_CMD } else { 0 });
    if let Some(bit) = modifier_bit(vk) {
        if is_key_up {
            mask &= !bit;
        } else {
            mask |= bit;
        }
    }
    mask
}

fn modifier_only_toggle_fired(vk: u16, is_key_up: bool) -> bool {
    if modifier_bit(vk).is_none() {
        return false;
    }
    let Some(target) = modifier_only_target() else {
        return false;
    };
    let Ok(mut watch) = CHORD.lock() else {
        return false;
    };
    watch.modifiers(held_mask(vk, is_key_up), target)
}
```

Add `VK_LCONTROL, VK_RCONTROL, VK_LSHIFT, VK_RSHIFT, VK_LMENU, VK_RMENU` to the `windows`
crate imports beside the existing `VK_*` names, and `std::sync::Mutex`.

- [ ] **Step 2: Feed it from the hook**

In `low_level_keyboard_proc`, immediately after `let vk = kb.vkCode as u16;`:

```rust
    if modifier_only_toggle_fired(vk, is_key_up) {
        state::toggle_vietnamese();
    }
    if !is_key_up && modifier_bit(vk).is_none() {
        chord_interrupted();
    }
```

Modifier keys are never swallowed: the code falls through to the existing paths, and
`is_toggle_shortcut` returns `false` for a modifier-only shortcut after Task 1.

- [ ] **Step 3: Verify the Windows target lints clean**

Run: `cargo clippy --target x86_64-pc-windows-msvc --all-targets --all-features -- -D warnings`

If that target is not installed, run `rustup target add x86_64-pc-windows-msvc` first. If
it still cannot build for lack of the MSVC toolchain, note that in the commit and rely on
CI, which lints the Windows target.

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: PASS on the host target.

- [ ] **Step 4: Commit**

```bash
git add apps/vnkey/src/platform/windows.rs
git commit -m "feat(win): fire the toggle on a clean modifier release"
```

---

### Task 6: The recorder captures modifiers alone

**Files:**
- Modify: `apps/vnkey/src/ui/ipc.rs:68-77` (`ShortcutCapture`), `:133-143` (handler), `:312-341` (test)
- Modify: `apps/vnkey/src/ui/assets/settings.js:61-163`

**Interfaces:**
- Consumes: `shortcut_from_event(.., code: Option<&str>)` from Task 1.

- [ ] **Step 1: Write the failing test**

In `ipc.rs` tests, extend `every_command_the_pages_send_is_understood` with a
modifier-only capture, and add:

```rust
    #[test]
    fn a_capture_may_arrive_without_a_key() {
        assert_eq!(
            parse(
                r#"{"cmd":"shortcut_capture","code":null,"ctrl":true,
                    "alt":false,"shift":true,"meta":false}"#
            ),
            Cmd::ShortcutCapture {
                code: None,
                ctrl: true,
                alt: false,
                shift: true,
                meta: false,
            }
        );
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p vnkey ipc::tests 2>&1 | tail -20`
Expected: FAIL — `code` is `String`, so `null` does not deserialise.

- [ ] **Step 3: Make the field optional**

In `Cmd::ShortcutCapture`, change `code: String` to `code: Option<String>`. In the handler:

```rust
        Cmd::ShortcutCapture {
            code,
            ctrl,
            alt,
            shift,
            meta,
        } => {
            if let Some(shortcut) = shortcut_from_event(ctrl, alt, shift, meta, code.as_deref()) {
                state::update(move |s| s.toggle_shortcut = shortcut);
            }
        }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p vnkey ipc::tests 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Teach the page to commit on release**

In `settings.js`, replace the hint constants and add peak tracking:

```js
  var ASK = "Hold modifiers, then press a key — or just let go.";
  var TOO_MANY = "That’s too many — three keys at most.";
  var NEED_TWO = "Use two modifiers, or add a key.";
  var peak = 0;
```

In `startRecording`, add `peak = 0;`. In the modifier branch of the keydown handler,
record the peak:

```js
    if (["Control", "Alt", "Shift", "Meta"].indexOf(e.key) !== -1) {
      var mods = heldMods(e);
      if (mods.length >= peak) { peak = mods.length; peakMods = mods; }
      renderCaps(shortcut, mods, true);
      setHint(mods.length > 3 ? TOO_MANY : ASK);
      return;
    }
```

Declare `var peakMods = [];` beside `peak`, and reset it in `startRecording`.

Add a `usedKey` flag set to `true` just before the existing `send({cmd:"shortcut_capture"…})`,
reset in `startRecording`, and change that send to pass `code: e.code`.

Replace the keyup handler:

```js
  document.addEventListener("keyup", function (e) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();

    var stillHeld = e.ctrlKey || e.altKey || e.shiftKey || e.metaKey;
    if (stillHeld || usedKey) return;

    if (peak < 2) return setHint(NEED_TWO);
    if (peak > 3) return setHint(TOO_MANY);

    send({
      cmd: "shortcut_capture",
      code: null,
      ctrl: peakMods.indexOf("⌃") !== -1,
      alt: peakMods.indexOf("⌥") !== -1,
      shift: peakMods.indexOf("⇧") !== -1,
      meta: peakMods.indexOf("⌘") !== -1,
    });
    stopRecording();
  }, true);
```

- [ ] **Step 6: Verify**

Run: `cargo test --workspace 2>&1 | tail -20` and
`cargo clippy --all-targets --all-features -- -D warnings`

- [ ] **Step 7: Commit**

```bash
git add apps/vnkey/src/ui/ipc.rs apps/vnkey/src/ui/assets/settings.js
git commit -m "feat(settings): record a shortcut made only of modifiers"
```

---

### Task 7: The alert's content model

**Files:**
- Modify: `apps/vnkey/src/update.rs` (append before `#[cfg(test)]`)
- Modify: `apps/vnkey/src/platform/mod.rs`, `apps/vnkey/src/platform/macos.rs`

**Interfaces:**
- Produces: `update::Action` with `label()` and `cmd()`; `update::Notice`;
  `update::notice_updated(from: &str, to: &str, needs_permission: bool) -> Notice`;
  `notice_install_failed(version: &str, error: &str) -> Notice`;
  `notice_up_to_date() -> Notice`; `notice_check_failed(error: &str) -> Notice`;
  `platform::open_accessibility_settings()`.

- [ ] **Step 1: Write the failing tests**

Append to `update.rs` tests:

```rust
    #[test]
    fn a_completed_update_offers_accessibility_only_when_it_was_lost() {
        let kept = notice_updated("0.0.24", "0.0.25", false);
        assert!(kept.body.contains("0.0.24"));
        assert!(kept.body.contains("0.0.25"));
        assert_eq!(kept.action, None);
        assert_eq!(kept.warn, None);

        let lost = notice_updated("0.0.24", "0.0.25", true);
        assert_eq!(lost.action, Some(Action::Accessibility));
        assert!(lost.warn.is_some());
    }

    #[test]
    fn a_failed_install_offers_a_manual_download() {
        let notice = notice_install_failed("0.0.25", "curl exited with 22");
        assert!(notice.body.contains("0.0.25"));
        assert!(notice.body.contains("curl exited with 22"));
        assert_eq!(notice.action, Some(Action::Releases));
    }

    #[test]
    fn being_up_to_date_needs_no_action() {
        let notice = notice_up_to_date();
        assert!(notice.body.contains(CURRENT));
        assert_eq!(notice.action, None);
    }

    #[test]
    fn a_failed_check_offers_a_retry() {
        let notice = notice_check_failed("no network");
        assert!(notice.body.contains("no network"));
        assert_eq!(notice.action, Some(Action::Retry));
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p vnkey update::tests 2>&1 | tail -20`
Expected: FAIL — nothing named `Notice` exists.

- [ ] **Step 3: Implement**

```rust
/// The one button an alert offers besides Done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accessibility,
    Releases,
    Retry,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Accessibility => "Open Accessibility",
            Action::Releases => "Download manually",
            Action::Retry => "Try again",
        }
    }

    /// The interface command the button sends, matching `ui::ipc::Cmd`.
    pub fn cmd(self) -> &'static str {
        match self {
            Action::Accessibility => "open_accessibility",
            Action::Releases => "open_releases",
            Action::Retry => "check_updates",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// Rendered in the amber box the panel uses for the secure-input warning.
    pub warn: Option<String>,
    pub action: Option<Action>,
}

pub fn notice_updated(from: &str, to: &str, needs_permission: bool) -> Notice {
    Notice {
        title: format!("Updated to {to}"),
        body: format!("PhaciusKey has been updated from {from} to {to} and restarted."),
        warn: needs_permission.then(|| {
            "macOS needs you to allow Accessibility once more, because this update changed \
             the app's code-signing identity. Typing stays off until then."
                .to_string()
        }),
        action: needs_permission.then_some(Action::Accessibility),
    }
}

pub fn notice_install_failed(version: &str, error: &str) -> Notice {
    Notice {
        title: "Update failed".to_string(),
        body: format!(
            "PhaciusKey could not install version {version} automatically.\n\n{error}\n\n\
             The current version keeps working."
        ),
        warn: None,
        action: Some(Action::Releases),
    }
}

pub fn notice_up_to_date() -> Notice {
    Notice {
        title: "Up to date".to_string(),
        body: format!("PhaciusKey {CURRENT} is the newest version."),
        warn: None,
        action: None,
    }
}

pub fn notice_check_failed(error: &str) -> Notice {
    Notice {
        title: "Couldn't check for updates".to_string(),
        body: format!("PhaciusKey could not reach GitHub.\n\n{error}"),
        warn: None,
        action: Some(Action::Retry),
    }
}
```

- [ ] **Step 4: Add the accessibility opener**

In `platform/macos.rs`:

```rust
pub(super) fn open_accessibility_settings() {
    crate::update::open_url(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    );
}
```

In `platform/mod.rs`:

```rust
pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    macos::open_accessibility_settings();
}
```

- [ ] **Step 5: Run and commit**

Run: `cargo test --workspace 2>&1 | tail -20` — Expected: PASS.
Run: `cargo clippy --all-targets --all-features -- -D warnings`

```bash
git add apps/vnkey/src/update.rs apps/vnkey/src/platform/mod.rs apps/vnkey/src/platform/macos.rs
git commit -m "feat(update): describe each update outcome as data"
```

---

### Task 8: The alert window

**Files:**
- Create: `apps/vnkey/src/ui/alert.rs`, `apps/vnkey/src/ui/assets/alert.html`,
  `apps/vnkey/src/ui/assets/alert.css`, `apps/vnkey/src/ui/assets/alert.js`
- Modify: `apps/vnkey/src/ui/mod.rs`, `apps/vnkey/src/ui/ipc.rs`

**Interfaces:**
- Consumes: `update::Notice` and `Action` from Task 7; `super::document` from `ui/mod.rs`.
- Produces: `ui::Alert::new(target, proxy) -> Result<Alert, String>`;
  `Alert::window_id()`, `Alert::show(&Notice, target)`, `Alert::hide()`,
  `Alert::set_content_height(f64, target)`; `Surface::Alert`;
  `WindowAction::OpenAccessibility` is not added — the commands are handled inside `ipc.rs`.

- [ ] **Step 1: Write the failing tests**

In `ui/mod.rs` tests:

```rust
    #[test]
    fn an_alert_page_carries_the_theme_and_both_parts() {
        let page = document(
            include_str!("assets/alert.css"),
            include_str!("assets/alert.html"),
            include_str!("assets/alert.js"),
        );
        assert!(page.contains("--lacquer"), "theme is missing");
        assert!(page.contains("id=\"title\""), "title is missing");
        assert!(page.contains("id=\"action\""), "action button is missing");
    }
```

In `alert.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 100.0, y: 0.0, width: 1000.0, height: 800.0 };

    #[test]
    fn it_centres_on_the_work_area() {
        let origin = centre_origin(PhysicalSize::new(400, 200), SCREEN);
        assert_eq!(origin.x, 400);
        assert_eq!(origin.y, 300);
    }

    #[test]
    fn it_centres_on_a_work_area_that_does_not_start_at_zero() {
        let screen = Rect { x: -1920.0, y: -1080.0, width: 1920.0, height: 1080.0 };
        let origin = centre_origin(PhysicalSize::new(400, 200), screen);
        assert_eq!(origin.x, -1160);
        assert_eq!(origin.y, -640);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p vnkey 2>&1 | tail -20`
Expected: FAIL — the asset files and `alert` module do not exist.

- [ ] **Step 3: Write the page**

`assets/alert.html`:

```html
<div class="surface" id="surface">
  <header class="head">
    <b id="title"></b>
  </header>
  <p class="body" id="body"></p>
  <p class="warn" id="warn" hidden></p>
  <div class="foot">
    <button class="btn accent" id="action" type="button" hidden></button>
    <button class="btn" id="done" type="button">Done</button>
  </div>
</div>
```

`assets/alert.css`:

```css
/* The update alert. Same theme as the panel; wider, and centred on the screen rather
   than hung off the menu bar. */
html, body { height: auto; }
.surface { position: static; inset: auto; padding: 14px 16px 16px; }

.head { padding: 0 0 6px; }
.head b { color: var(--ink); font-size: 14px; font-weight: 600; }

.body { margin: 0; font-size: 12.5px; white-space: pre-wrap; }

.warn { margin: 10px 0 0; font-size: 11.5px; padding: 8px 10px; }
.warn[hidden] { display: none; }

.foot { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
.foot .btn[hidden] { display: none; }
```

`assets/alert.js`:

```js
(function () {
  "use strict";

  var send = function (o) {
    if (window.ipc) window.ipc.postMessage(JSON.stringify(o));
  };
  var $ = function (id) { return document.getElementById(id); };
  var close = function () { send({ cmd: "close_window" }); };

  $("done").addEventListener("click", close);

  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape") return close();
    if (e.key === "Enter" && !$("action").hidden) $("action").click();
  });

  var reported = 0;
  function reportHeight() {
    var height = Math.ceil($("surface").getBoundingClientRect().height);
    if (!height || height === reported) return;
    reported = height;
    send({ cmd: "panel_height", height: height });
  }
  if (window.ResizeObserver) new ResizeObserver(reportHeight).observe($("surface"));

  window.__setNotice = function (n) {
    $("title").textContent = n.title;
    $("body").textContent = n.body;
    $("warn").hidden = !n.warn;
    $("warn").textContent = n.warn || "";

    var action = $("action");
    action.hidden = !n.action;
    action.textContent = n.action ? n.action.label : "";
    action.onclick = n.action
      ? function () { send({ cmd: n.action.cmd }); close(); }
      : null;

    reportHeight();
  };
}());
```

- [ ] **Step 4: Write the window**

`apps/vnkey/src/ui/alert.rs`:

```rust
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

const CSS: &str = include_str!("assets/alert.css");
const BODY: &str = include_str!("assets/alert.html");
const SCRIPT: &str = include_str!("assets/alert.js");

const WIDTH: f64 = 380.0;
const INITIAL_HEIGHT: f64 = 160.0;

fn centre_origin(size: PhysicalSize<u32>, work_area: Rect) -> PhysicalPosition<i32> {
    let x = work_area.x + (work_area.width - f64::from(size.width)) / 2.0;
    let y = work_area.y + (work_area.height - f64::from(size.height)) / 2.0;
    PhysicalPosition::new(x.round() as i32, y.round() as i32)
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

        Ok(Self { window, webview, height: Cell::new(INITIAL_HEIGHT) })
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
        self.window.set_outer_position(centre_origin(size, work_area));
    }
}
```

`Rect` must become visible to this module: in `panel.rs` it is already `pub struct Rect`,
and `ui/mod.rs` declares `mod panel;` — change to `pub(super) mod panel;` is not needed
because both are children of `ui`. Add `mod alert;` and `pub use alert::Alert;` to
`ui/mod.rs`, and add `Alert` to the `Surface` enum.

- [ ] **Step 5: Add the two commands**

In `ipc.rs`, add to `Cmd`:

```rust
    OpenAccessibility,
    OpenReleases,
```

and to `apply_ipc`:

```rust
        Cmd::OpenAccessibility => crate::platform::open_accessibility_settings(),
        Cmd::OpenReleases => update::open_url(&update::releases_url()),
```

Extend the `every_command_the_pages_send_is_understood` list with
`r#"{"cmd":"open_accessibility"}"#` and `r#"{"cmd":"open_releases"}"#`.

- [ ] **Step 6: Keep `main.rs` compiling**

Adding `Surface::Alert` makes the two `match surface` blocks in `main.rs` non-exhaustive.
Task 9 gives them behaviour; this task only has to keep the tree green. In the
`WindowAction::Close` match add:

```rust
                        Surface::Alert => {}
```

and change the `WindowAction::Resize(height)` arm to name the surface:

```rust
                    Some(WindowAction::Resize(height)) => match surface {
                        Surface::Alert => {}
                        Surface::Panel | Surface::Settings => {
                            if let Some(panel) = &panel {
                                panel.set_content_height(f64::from(height), target);
                            }
                        }
                    },
```

- [ ] **Step 7: Run and commit**

Run: `cargo test --workspace 2>&1 | tail -20` — Expected: PASS.
Run: `cargo clippy --all-targets --all-features -- -D warnings`

```bash
git add apps/vnkey/src/ui/
git commit -m "feat(ui): draw update alerts in the application's own theme"
```

---

### Task 9: Alerts replace the AppleScript dialogs

**Files:**
- Modify: `apps/vnkey/src/main.rs:29-36` (events), `:38-78` (argument parsing),
  `:98-210` (event loop), `:389-400` (`announce_completed_update`)
- Modify: `apps/vnkey/src/installer.rs:147-184` (delete four functions)

**Interfaces:**
- Consumes: `ui::Alert`, `update::Notice` and its constructors.

- [ ] **Step 1: Delete the AppleScript path**

Remove `announce_update`, `announce`, `announce_failure` and `show_dialog` from
`installer.rs`. Keep `run`, `install`, `stage_from_mount`, `swap_in_place`,
`relaunch_and_exit`, `app_bundle`, `Outcome`.

- [ ] **Step 2: Own an alert in the event loop**

In `run`, beside `let mut panel: Option<Panel> = None;`:

```rust
    let mut alert: Option<Alert> = None;
```

Add a helper below `push_state`:

```rust
fn show_alert(
    alert: &mut Option<Alert>,
    notice: &crate::update::Notice,
    target: &tao::event_loop::EventLoopWindowTarget<UserEvent>,
    proxy: &EventLoopProxy<UserEvent>,
) {
    if alert.is_none() {
        match Alert::new(target, proxy.clone()) {
            Ok(win) => *alert = Some(win),
            Err(e) => {
                eprintln!("[vnkey] failed to create the alert window: {e}");
                return;
            }
        }
    }
    if let Some(win) = alert {
        win.show(notice, target);
    }
}
```

- [ ] **Step 3: Point the four callers at it**

`UserEvent::UpdateFailed`:

```rust
                let notice = update::notice_install_failed(&version, &reason);
                show_alert(&mut alert, &notice, target, &proxy);
```

`UpdateCheckDone(Ok(None))`: `update::notice_up_to_date()`.
`UpdateCheckDone(Err(e))`: `update::notice_check_failed(&e)`.

`announce_completed_update` becomes a function returning the notice rather than showing it:

```rust
fn completed_update_notice() -> Option<update::Notice> {
    let previous = state::settings().last_seen_version;
    let notice = previous.as_deref().filter(|prev| *prev != update::CURRENT).map(|prev| {
        update::notice_updated(prev, update::CURRENT, !platform::permission_granted())
    });
    if previous.as_deref() != Some(update::CURRENT) {
        state::update(|s| s.last_seen_version = Some(update::CURRENT.to_string()));
    }
    notice
}
```

and at `StartCause::Init`, replacing the `announce_completed_update();` call:

```rust
                if let Some(notice) = completed_update_notice() {
                    show_alert(&mut alert, &notice, target, &proxy);
                }
```

- [ ] **Step 4: Route close and resize by surface**

In the `Ipc` arm, extend `WindowAction::Close`:

```rust
                        Surface::Alert => {
                            if let Some(win) = &alert {
                                win.hide();
                            }
                        }
```

and `WindowAction::Resize(height)`:

```rust
                    Some(WindowAction::Resize(height)) => match surface {
                        Surface::Alert => {
                            if let Some(win) = &alert {
                                win.set_content_height(f64::from(height), target);
                            }
                        }
                        Surface::Panel | Surface::Settings => {
                            if let Some(panel) = &panel {
                                panel.set_content_height(f64::from(height), target);
                            }
                        }
                    },
```

- [ ] **Step 5: Add `--show-alert`**

In `main`, after the `--export-iconset` block:

```rust
    let mut forced_alert = None;
    if args_start.as_deref() == Some("--show-alert") {
        let Some(kind) = args.next() else {
            eprintln!("usage: vnkey --show-alert <updated-needs-permission|install-failed|up-to-date|check-failed>");
            std::process::exit(2);
        };
        forced_alert = Some(match kind.as_str() {
            "updated-needs-permission" => update::notice_updated("0.0.24", update::CURRENT, true),
            "install-failed" => update::notice_install_failed(update::CURRENT, "a sample failure"),
            "up-to-date" => update::notice_up_to_date(),
            "check-failed" => update::notice_check_failed("a sample failure"),
            other => {
                eprintln!("[vnkey] unknown alert kind: {other}");
                std::process::exit(2);
            }
        });
    }
```

Restructure the leading argument read so both flags share it: replace
`if args.next().as_deref() == Some("--export-iconset")` with
`let args_start = args.next();` followed by
`if args_start.as_deref() == Some("--export-iconset") { … }`.

Thread `forced_alert` into `run` as a parameter, and at `StartCause::Init` prefer it over
`completed_update_notice()`.

- [ ] **Step 6: Verify each alert by hand**

```sh
cargo run -p vnkey -- --show-alert up-to-date
cargo run -p vnkey -- --show-alert updated-needs-permission
cargo run -p vnkey -- --show-alert install-failed
cargo run -p vnkey -- --show-alert check-failed
```

For each: it appears centred, in front of whatever is frontmost; Done closes it; the
action button fires on the first click without a focusing click first.

If any alert appears *behind* the frontmost application, apply the fallback recorded in
the spec — an `objc_msgSend` shim calling `[NSApp activateIgnoringOtherApps:YES]` in
`macos.rs`, called from `Alert::show`.

- [ ] **Step 7: Run and commit**

Run: `cargo test --workspace 2>&1 | tail -20` and
`cargo clippy --all-targets --all-features -- -D warnings`

```bash
git add apps/vnkey/src/main.rs apps/vnkey/src/installer.rs
git commit -m "feat(update): replace the AppleScript dialogs with themed alerts"
```

---

### Task 10: The panel stops describing its switch as per-application

**Files:**
- Modify: `apps/vnkey/src/ui/payload.rs`
- Modify: `apps/vnkey/src/ui/assets/panel.html:9-13`, `apps/vnkey/src/ui/assets/panel.js:53-79`

**Interfaces:**
- Produces: payload field `excluded_summary: String`.

- [ ] **Step 1: Write the failing test**

In `payload.rs` tests:

```rust
    #[test]
    fn the_summary_counts_and_pluralises() {
        assert_eq!(excluded_summary(0), "Everywhere");
        assert_eq!(excluded_summary(1), "Everywhere except 1 app");
        assert_eq!(excluded_summary(3), "Everywhere except 3 apps");
    }

    #[test]
    fn the_payload_never_scopes_the_switch_to_one_application() {
        assert_eq!(payload()["excluded_summary"], "Everywhere");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p vnkey payload::tests 2>&1 | tail -20`
Expected: FAIL — `excluded_summary` does not exist.

- [ ] **Step 3: Implement**

```rust
/// The switch is machine-wide, so its caption describes scope and never names the
/// application in front, which would read as the switch belonging to that application.
fn excluded_summary(count: usize) -> String {
    match count {
        0 => "Everywhere".to_string(),
        1 => "Everywhere except 1 app".to_string(),
        n => format!("Everywhere except {n} apps"),
    }
}
```

and in `state_json`, beside `"excluded_apps"`:

```rust
        "excluded_summary": excluded_summary(s.disabled_apps.len()),
```

- [ ] **Step 4: Update the page**

`panel.html`, replacing the Vietnamese row and adding the warning:

```html
  <div class="row">
    <div class="lbl"><b>Vietnamese typing</b><small id="where"></small></div>
    <span class="caps" id="shortcut"></span>
    <label class="sw"><input type="checkbox" id="enabled"><i></i></label>
  </div>
  <p class="warn" id="excluded" hidden></p>
```

`panel.js`, replacing the `$("where")` assignment:

```js
    $("where").textContent = s.excluded_summary;
    $("excluded").hidden = !s.excluded_here;
    $("excluded").textContent = "⚠ " + (s.current_app || "This app") + " is one of them";
```

- [ ] **Step 5: Verify by hand**

Run `cargo run -p vnkey`, open the panel from the tray icon, and check three states: no
exclusions (`Everywhere`); one or more excluded while standing outside them
(`Everywhere except N apps`, no warning row); standing inside an excluded application
(warning row present). Flip the switch inside an excluded application — the switch moves,
the warning stays, typing stays off there.

- [ ] **Step 6: Run and commit**

Run: `cargo test --workspace 2>&1 | tail -20` and
`cargo clippy --all-targets --all-features -- -D warnings`

```bash
git add apps/vnkey/src/ui/payload.rs apps/vnkey/src/ui/assets/panel.html apps/vnkey/src/ui/assets/panel.js
git commit -m "fix(panel): stop the master switch reading as a per-app switch"
```

---

### Task 11: Release 0.0.25

**Files:**
- Modify: `apps/vnkey/Cargo.toml:4`
- Modify: `docs/specs/2026-08-09-update-alerts-and-modifier-shortcuts-design.md:3` (status)

- [ ] **Step 1: Run the hand tests the spec calls for**

From the spec's Testing section, on macOS:

- `⌃⇧` recorded by holding and releasing; `⌃⇧V` by holding and pressing.
- `⌃⇧` toggles typing; `⌃⇧C` in a browser opens dev tools and does *not* toggle;
  `⌃⇧` with `⌥` added and released does *not* toggle.
- All four alerts via `--show-alert`, each in front of a fullscreen application.

Record the outcome. If any fails, stop and fix before releasing.

- [ ] **Step 2: Bump the version — in BOTH files**

`.github/workflows/release.yml` refuses to publish unless the tag matches
`apps/vnkey/Info.plist`'s `CFBundleShortVersionString` **and** `apps/vnkey/Cargo.toml`'s
`version`. Bumping only one fails the release after the tag is already pushed.

- `apps/vnkey/Cargo.toml`: `version = "0.0.24"` → `"0.0.25"`
- `apps/vnkey/Info.plist`: `CFBundleShortVersionString` `0.0.24` → `0.0.25`
  (leave `CFBundleVersion` at `18`; it has not moved since 0.0.18)

Then `cargo check -p vnkey` so `Cargo.lock` updates. Verify before tagging:

```sh
grep '^version' apps/vnkey/Cargo.toml
/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' apps/vnkey/Info.plist
```

- [ ] **Step 3: Mark the spec implemented**

Change the status line to
`**Date:** 2026-08-09 · **Status:** implemented in 0.0.25.` and note any hand test that
was not run.

- [ ] **Step 4: Verify green**

Run: `cargo test --workspace 2>&1 | tail -20`
Run: `cargo clippy --all-targets --all-features -- -D warnings`
Both must pass before the tag.

- [ ] **Step 5: Commit, merge, tag, push**

```bash
git add apps/vnkey/Cargo.toml Cargo.lock docs/specs/
git commit -m "chore(release): bump version to 0.0.25"
git switch main
git merge --no-ff feat/update-alerts-shortcuts -m "Merge update alerts, modifier-only shortcuts, and the panel switch fix"
git tag v0.0.25
git push origin main --tags
```

---

## Self-Review

**Spec coverage.** Part A data model → Task 1. Firing rule → Task 2. macOS → Task 4.
Windows → Task 5. Hot path → Task 3. Recorder → Task 6. Part B surface, content, wiring,
sizing, deletions → Tasks 7–9. `--show-alert` → Task 9 Step 5. Activation fallback →
Task 9 Step 6. Part C → Task 10. Release → Task 11.

**Deviation from the spec, deliberate.** The spec's `ChordWatch` sketch uses one flag and
would re-arm after a third modifier is released, contradicting its own test list. Task 2
adds `poisoned`. The spec should be amended to match after implementation.

**Not covered, and why.** The spec's `centre_origin` is specified as centring on "the
monitor under the pointer"; `Alert::place` uses `primary_monitor()` instead, because an
alert that fires unprompted has no pointer gesture to take a monitor from. The pure
function takes a work area either way, so switching later costs one line.

**Type consistency.** `Shortcut.key: Option<char>` is used consistently in Tasks 1, 3, 4,
5. `ChordWatch::modifiers(held: u8, target: u8) -> bool` and `interrupted()` match across
Tasks 2, 4, 5. `Notice { title, body, warn, action }` matches across Tasks 7, 8, 9.
`Action::cmd()` returns the same strings the `Cmd` enum accepts in Task 8 Step 5.
