# Settings and menu redesign: custom panel, tabbed window, exclusion list

**Date:** 2026-08-09 · **Status:** designed, not yet implemented

## Problem

Seven issues, one of which is a hard bug and the rest a UI rework.

1. **The toggle shortcut can never be changed.** It always stays `ctrl+shift+v`.
2. The shortcut label renders as an undifferentiated monospace blob (`⌃⇧V`).
3. There is no bound on how many keys a shortcut may hold.
4. The settings window is one long scroll of six stacked sections, and wears
   the stock OS titlebar.
5. The tray icon's menu and the settings window do not look like the same
   application.
6. The menu carries current-app information the user does not want there.
7. "Remember on/off per app" confuses people; a plain exclusion list does not.

Plus: the published home page describes an app that no longer exists.

### Root cause of (1)

`settings.html` builds the capture message with a duplicate object key:

```js
send({ cmd: "shortcut_capture", code: e.code,
       ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, cmd: e.metaKey });
```

`cmd` is both the command discriminator and the name chosen for the ⌘
modifier. A JavaScript object literal keeps the *last* duplicate, so the
serialised message is `{"cmd":false,"code":"KeyV",…}`. In `apply_ipc`,
`v["cmd"].as_str()` is therefore `None`, the `match` falls through to
`_ => {}`, and nothing is written. The page then repaints the label from
unchanged state, which reads as "the shortcut reverted".

The defect that let a typo become a silent no-op is the hand-rolled
`match v["cmd"].as_str()` with a catch-all arm. That is what the fix targets,
not just the field name.

## Decisions

| Question | Decision |
|---|---|
| What counts toward "2–3 keys" | One or two modifiers plus exactly one letter/digit/space. Modifier-only combinations are not accepted. |
| Settings window chrome | Fully frameless; own header, own close button, drag to move. |
| The tray icon's menu | Replaced by a panel we draw ourselves, so it can share the settings theme. |
| Panel content | Secure-input warning · Vietnamese toggle · method · tone · update row · Settings… · Quit. |
| Shortcut inside an excluded app | Nothing happens. Exclusion is absolute. |
| Appearance | The dark lacquer theme in both system appearances. |
| Windows | Written and kept lint-clean, including the toggle shortcut. Verified on macOS only. |
| Settings tabs | Typing · Macros · Apps · About, with the nav as a left sidebar. |

## Design

### Shortcut

**IPC.** `apply_ipc`'s `match v["cmd"].as_str()` is replaced by a
`#[serde(tag = "cmd")]` enum deserialised with `serde_json::from_str`. A
message that fails to deserialise is reported with `eprintln!` instead of
being dropped. The ⌘ field is renamed `meta` in both directions.

**Rules.** `parse_shortcut` accepts a combination only when it holds exactly
one letter, digit or space and either one or two modifiers.
`shortcut_from_event` applies the same ceiling, so the recorder refuses a
third modifier at capture time and can say so, rather than storing a value
the keyboard hook will not match.

A stored shortcut with more than two modifiers (only reachable by hand-editing
`config.toml`) stops parsing and is shown as invalid — the path an
unparseable value already takes.

**Display.** `shortcut_display(&str) -> String` is replaced by
`shortcut_parts(&str) -> Vec<String>`, returning `["⌃","⇧","V"]`, with the
raw string as a single element when it does not parse. Both surfaces render
one keycap chip per part. While recording, held modifiers appear as chips
immediately and the pending key shows as a dashed placeholder.

**Windows.** `platform/windows.rs` currently never tests the shortcut;
`shortcut_modifier_down()` only bails out of Vietnamese processing. It gains
an `is_toggle_shortcut(vk)` that mirrors `platform/macos.rs`, reading modifier
state from `GetAsyncKeyState` and consuming the keystroke when it matches.

### Per-app model

`per_app_mode: bool` and `app_modes: BTreeMap<String, bool>` are removed from
`Settings`. `disabled_apps` keeps its name and its TOML key, so existing configs load
unchanged; only the vocabulary around it changes. `disabled_for` is renamed
`excluded_for` to match the wording the UI now uses — **"Apps that never use
Vietnamese typing"**.

```rust
pub fn vietnamese_on(&self, app: Option<&str>) -> bool {
    !self.excluded_for(app) && self.enabled
}
```

**Migration.** A `#[serde(rename = "app_modes", skip_serializing)]` field
carries legacy entries through `load()`; those set to `false` are folded into
`disabled_apps`, then the field is cleared. `per_app_mode` needs no handling —
serde ignores unknown keys. Both vanish from `config.toml` on the next save.
The migration is one-way, which is appropriate for a setting being removed
because it confused users.

`state::toggle_vietnamese` loses its per-app branch and only ever flips
`enabled`. In an excluded app the shortcut therefore has no visible effect,
and the tray icon shows the off state.

**Self-pid guard.** The macOS event tap resolves the target pid of each key
event into an application name and records it in `seen_apps`. Once the
settings window accepts typing, PhaciusKey files *itself* into the list the
user is curating. The tap skips events whose target pid is our own process.

### Two surfaces, one theme

`settings.html` is 643 lines of CSS, markup and script in a single
`include_str!`. A tabbed frameless window plus a panel would roughly double
that in one file, so it splits:

```
src/ui/
  mod.rs               document composition · IPC enum · state payload
  panel.rs             tray-anchored panel window
  settings.rs          settings window
  assets/theme.css     tokens + switch / segment / keycap / button primitives
  assets/panel.html    assets/panel.js
  assets/settings.html assets/settings.js
```

Each document is composed at startup from theme, markup and script. A single
shared `theme.css` is what actually makes the two backgrounds match; it keeps
the existing lacquer palette, unchanged across system appearances.

One state payload serves both surfaces. The panel ignores the fields it does
not use, which costs a little redundant JSON per push and removes the risk of
the two payloads drifting apart.

### Panel

Built with `with_decorations(false)`, `with_transparent(true)`,
`with_resizable(false)`, always on top, and `with_accept_first_mouse(true)` so
the first click operates a control instead of merely focusing the window.

**Placement.** `TrayIconEvent::Click` carries the icon's screen `rect`. The
geometry lands in a pure function:

```rust
fn panel_origin(icon: Rect, panel: PhysicalSize<u32>, work_area: Rect)
    -> PhysicalPosition<i32>
```

It centres the panel horizontally on the icon, places it below, flips it above
when there is no room underneath (Windows taskbars sit at the bottom), and
clamps horizontally to the work area. Being pure, it is unit-testable without
a GUI.

**Dismissal.** `WindowEvent::Focused(false)` hides the panel; Esc does the
same from the page. This creates one interaction to guard: clicking the tray
icon while the panel is open delivers focus-loss *before* the click, so the
panel would hide and immediately reopen. A timestamp check ignores a tray
click within 250 ms of a focus-driven hide.

**Height.** The page reports its content height over IPC and the window
resizes to match, because the secure-input warning and the update row appear
conditionally and a fixed height would leave dead space.

**Content.**

```
⚠ Secure input on — typing paused        (only when it applies)
PhaciusKey                       v0.0.23
Vietnamese typing         ⌃⇧V     (━●)
────────────────────────────────────────
Input method        [ Telex ][  VNI  ]
Tone placement      [ Modern ][Classic]
────────────────────────────────────────
⬇ Update to v0.0.24 now                  (or: Check for updates…)
────────────────────────────────────────
Settings…                           Quit
```

**Fallback.** With the native menu gone, a webview that fails to build would
leave a menu-bar-only application with no way to quit. If `Panel::new()`
fails, the tray attaches a minimal native menu holding Settings… and Quit.

### Settings window

Frameless and transparent for rounded corners and a shadow; **fixed at
720×560 and not resizable**. Fixing the size removes all resize-edge handling
and addresses the original complaint directly: the header and nav never move,
and only the content pane scrolls.

```
┌────────────────────────────────────────────────┐
│ ⌁  PhaciusKey                        v0.0.23   │  drag to move
├──────────────┬─────────────────────────────────┤
│  ▸ Typing    │  Vietnamese typing        (━●)  │
│    Macros    │  Toggle shortcut    [⌃][⇧][V]   │
│    Apps      │  Input method  [Telex][ VNI ]   │  only this
│    About     │  Tone placement [Modern][Class] │  pane scrolls
│              │  ── Telex ────────────────────  │
│              │  Standalone "w" → ư       (━●)  │
└──────────────┴─────────────────────────────────┘
```

Dragging is `mousedown` on the header (excluding controls) → IPC →
`Window::drag_window()`. `TaoWindow` overrides `canBecomeKeyWindow`
unconditionally, so a borderless window still takes keyboard focus and the
macro text fields keep working.

The close control sits at the trailing edge of the titlebar on both platforms.
It is drawn from the same glass-and-jade vocabulary as every other control here
rather than imitating the system traffic lights, which a borderless window does
not have.

**Tabs.**

- **Typing** — master switch, toggle shortcut, input method, tone placement,
  auto-restore, capitalise sentences, and the Telex group. The Telex group
  renders only when Telex is selected; today `#telex-section` exists but
  nothing ever toggles it, so VNI users see four irrelevant switches.
- **Macros** — enable, list, add, import/export.
- **Apps** — the exclusion list, slow typing, autocomplete fix.
- **About** — version, update status and check, start at login, automatic
  updates, the Telex/VNI cheatsheet, open config file, report an issue.

### Tray

`tray.rs` loses the whole menu — roughly its top half — leaving the icon,
the tooltip and click events. `handle_menu_event` in `main.rs` mostly
disappears with it, as does `accelerator_for` and its `Shortcut`-to-`Code`
table, which existed only to put `⌃⇧V` beside a menu item. The cheatsheet and
"Report an issue" move to About.

Update status moves out of `Tray`, where it lives as mutated menu-item text,
into shared state as an enum (`Idle`, `Checking`, `Available(v)`,
`Installing(v)`, `Failed(reason)`), so the panel and About render the same
value from one source.

### Home page

`docs/_config.yml` pins `version: 0.0.18` against a shipping 0.0.23. Beyond
the number, `docs/_includes/content.html` omits macros, sentence
capitalisation, the Telex shortcut options, the configurable toggle shortcut,
excluded apps, automatic updates and start-at-login, and its "Điều khiển từ
thanh menu" feature card describes the menu being replaced. The feature grid,
setup steps and FAQ are rewritten against what ships.

`release.yml` verifies `Info.plist` against `Cargo.toml` but never looks at
`docs/_config.yml`, which is how the drift went unnoticed. The site version
joins that check.

## Testing

Unit-testable, and covered:

- `parse_shortcut` — modifier floor and ceiling, single key, rejects
  modifier-only and 3-modifier combinations.
- `shortcut_from_event` — refuses a third modifier; everything it emits parses
  back.
- `shortcut_parts` — glyph order, Space, unparseable fallback.
- IPC — every command deserialises from the exact payload the page sends,
  including `shortcut_capture` with `meta`. This is the regression test for
  the original bug.
- Migration — legacy `app_modes` false entries become exclusions; unknown
  `per_app_mode` is ignored; neither is written back.
- `vietnamese_on` — exclusion beats `enabled`, case-insensitively.
- `panel_origin` — below, flipped above, clamped at both screen edges.

Then `cargo test --workspace` and
`cargo clippy --all-targets --all-features -- -D warnings`.

The macOS application is run by hand to check the panel, chrome, tabs and the
recorder. **Windows is not verified**: it is written, kept lint-clean, and
built by CI, but the toggle shortcut there needs testing on real hardware.

## Sequencing

1. Shortcut fix, key-count rules, keycap parts, exclusion list and migration.
   Pure logic, fully covered by tests, shippable alone.
2. Settings window: frameless chrome, left nav, tabs, asset split, shared
   theme.
3. Panel replacing the native menu; `tray.rs` shrinks; update status moves to
   shared state.
4. Windows toggle shortcut, home page, release version check.

## Out of scope

- Light-appearance palette. The lacquer theme is used in both appearances.
- Resizable settings window.
- Restoring per-app memory in any form.
- Verifying Windows behaviour on real hardware.
