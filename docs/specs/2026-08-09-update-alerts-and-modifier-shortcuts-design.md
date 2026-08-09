# Themed update alerts, modifier-only shortcuts, and an honest panel switch

**Date:** 2026-08-09 · **Status:** designed, not yet implemented.

## Problem

Three unrelated complaints, all about surfaces the 0.0.24 redesign did not
reach. Neither of the last two is the defect it was reported as: one names a
real gap by the wrong measurement, the other describes behaviour that is
already correct. Both are recorded as reported and then as found, so the next
reader does not go hunting for a bug that was never there.

1. **The update popups do not look like the application.** Four moments in the
   update lifecycle go through `osascript display dialog`: a system modal with
   the Script Editor icon, system chrome, and a single OK button. The rest of
   the app — the tray panel and the settings window — is drawn by us from a
   shared theme. On Windows the same four moments call `eprintln!`, which in a
   `windows_subsystem = "windows"` build goes nowhere at all, so Windows users
   are never told an update installed or failed.

2. **The toggle shortcut cannot be two modifiers.** A tester wants `⌃⇧`, the
   combination UniKey and OpenKey use. `parse_shortcut` requires a
   letter, digit or space, so `"ctrl+shift"` is rejected — pinned by a test at
   `config.rs:420`.

3. **The panel's Vietnamese switch looks like it only covers the current
   application.** It does not, but the line printed directly beneath it names
   whatever application you are standing in, which is the one position that
   implies scope.

Point 2 reverses a decision recorded in
`2026-08-09-settings-ui-redesign-design.md`, which stated: *"One or two
modifiers plus exactly one letter/digit/space. Modifier-only combinations are
not accepted."* That decision stands corrected. The min-2 / max-3 key rule it
established is unchanged; what changes is that two or three modifiers on their
own now satisfy it.

### What is not the problem

**The shortcut does not demand three keys.** `MODIFIERS` is `1..=2`
(`config.rs:201`), `parse_shortcut("ctrl+v")` is pinned as valid
(`config.rs:428`), the recorder hint already reads *"Hold one or two
modifiers"*, and both hooks match modifier-for-modifier. Two-key combinations
such as `⌃V` and `⌥Z` work today. The actual gap is modifier-only, which is a
different shape, not a different count.

**The panel switch is not per-application.** `vietnamese_on(app)` is
`!excluded_for(app) && self.enabled` (`config.rs:149`); the switch writes
`enabled`, one machine-wide flag, and `toggle_vietnamese` (`state.rs:123`)
flips exactly that. Per-application behaviour lives entirely in the exclusion
list, which is what the reporter expected and what already exists. Only the
panel's own copy says otherwise, and only the panel's: the settings window
already labels the same switch *"The master switch, everywhere"*
(`settings.html:21`).

### A hot-path defect this work would otherwise worsen

`state::settings()` (`state.rs:106`) clones the whole `Settings` — including the
`macros` `BTreeMap` and three `Vec<String>`s — and `is_toggle_shortcut` calls it
on **every key-down**, then string-parses `toggle_shortcut`. CLAUDE.md forbids
exactly this: *"Do not clone or allocate on that path."*

It is pre-existing, but modifier-only matching puts `FlagsChanged` on the same
path — one clone and one parse for every ⇧ pressed to type a capital letter. It
is fixed here because this work depends on that path being cheap.

## Decisions

| Question | Decision |
|---|---|
| Which update moments interrupt | The same four as today. What changes is how they look, and that they gain actions. |
| What draws them | A third webview surface sharing `theme.css`, alongside `Panel` and `SettingsWindow`. |
| Alert buttons | Each alert carries **Done**, plus one action button where an action resolves it. |
| Modifier-only shortcuts | Allowed, at two or three modifiers. A lone modifier is not. |
| When a modifier-only shortcut fires | On clean release: nothing else pressed, no third modifier joined. |
| How it is recorded | The same gesture that fires it — hold, then let go. |
| Parsed shortcut on the hot path | Cached in an `AtomicU32`, written on settings change. |
| Testing the alerts | A hidden `--show-alert <kind>` flag, following `--export-iconset`. |
| The panel's switch subtitle | Describes global scope only. It never names the current application. |
| Standing in an excluded application | Said in a warning row, not in the switch's subtitle. |
| Where that subtitle is built | `payload.rs`, so its counting and pluralisation get a unit test. |

---

## Part A — Modifier-only toggle shortcuts

### Data model

`Shortcut.key` becomes `Option<char>`, replacing the `'\0'` sentinel that
currently means "unset". Validity takes two shapes:

| Shape | Modifiers | Total keys |
|---|---|---|
| `Some(key)` | 1–2 | 2–3 |
| `None` | 2–3 | 2–3 |

`"ctrl+shift"` and `"ctrl+alt+shift"` parse. `"shift"` does not — a lone
modifier would fire constantly during ordinary typing. `"ctrl+alt+shift+cmd"`
still does not. The stored form stays a plain `+`-joined string, so existing
config files are untouched and no migration is needed.

`shortcut_parts("ctrl+shift")` returns `["⌃", "⇧"]`, which the existing keycap
renderer draws without change.

### The firing rule

A modifier-only shortcut is a prefix of every `⌃⇧X` in every application, so it
cannot fire when the modifiers go down. It fires when they are released, and
only if the gesture was clean.

This lives in `config::ChordWatch`, a pure state machine holding one `bool`, so
the rule is tested without a keyboard. The arms are evaluated in the order
written:

```
modifiers(held, target):
  held == target   → arm,      no fire
  held is empty    → fire if armed, then disarm
  held ⊄ target    → disarm            (a third modifier joined)
  held ⊂ target    → unchanged         (mid-press, or mid-release)

interrupted():     → disarm            (any other key, or a mouse click)
```

Because the empty case is tested first, the final arm only ever sees a
non-empty proper subset.

The subset case is what makes releasing one modifier before the other work:
`⌃⇧` → release ⇧ → `{⌃}` is a subset, still armed → release ⌃ → empty, fires.

Both hooks own a `ChordWatch` and feed it. The branching exists once.

### macOS

`ET_FLAGS_CHANGED = 12` joins the tap mask (`macos.rs:118`) and gains a branch
in the `match etype` dispatch (`macos.rs:151`). Flags events are **always**
passed through unchanged — swallowing one would leave every other application
believing a modifier is still held.

`ET_KEY_DOWN` and the two mouse arms call `interrupted()`.

### Windows

The low-level hook already receives `VK_CONTROL` and `VK_SHIFT` down and up, so
`ChordWatch` is fed from the existing key-up branch (`windows.rs:88`) and the
key-down path. Modifier keys are never swallowed, for the same reason.

### The hot path

A `static TOGGLE: AtomicU32` in `state` holds the parsed shortcut packed into
four modifier bits plus the key, written whenever settings change and read
lock-free in the hook. This removes a `Settings` clone and a string parse from
every keystroke. Pack and unpack round-trip in tests.

### The recorder

`Cmd::ShortcutCapture.code` becomes `Option<String>`.

The page tracks the peak modifier set held during the gesture, because on the
keyup that empties the set the event's own `ctrlKey`/`shiftKey` flags have
already gone false. On that keyup, if no usable key was pressed and the peak
held two or three modifiers, it captures modifiers-only. If a usable key was
pressed, behaviour is exactly as today.

The hint becomes *"Hold modifiers, then press a key — or just let go."* A lone
modifier released on its own answers *"Use two modifiers, or add a key."*

---

## Part B — Themed update alerts

### The surface

`ui/alert.rs` with `assets/alert.{html,css,js}`, built on the same recipe as
`Panel`: undecorated, transparent, always-on-top, drawing its own `.surface` so
it inherits the lacquer background, jade accents and radius from `theme.css`.
`Surface` gains an `Alert` variant, so `close_window` routes to `alert.hide()`
the way it already routes for the other two surfaces.

One window instance, created lazily and hidden rather than destroyed. A second
alert arriving while one is up replaces its content; these are rare enough that
a queue would be machinery for nothing.

### Content is data

`update::Notice { title, body, action: Option<Action> }`, next to the
`Status::detail` prose that already lives there, with four constructors:

| Caller | Title | Action button |
|---|---|---|
| `announce_completed_update` | Updated to `<version>` | Open Accessibility — only when the grant was lost |
| `UserEvent::UpdateFailed` | Update failed | Download manually |
| `UpdateCheckDone(Ok(None))` | Up to date | — |
| `UpdateCheckDone(Err)` | Couldn't check for updates | Try again |

Every alert carries **Done**. The accessibility variant renders its second
paragraph in the existing `.warn` box — the amber treatment the panel already
uses for the secure-input notice, which is the right register for *typing is
off until you act*.

### Wiring

Two new commands, `open_accessibility` and `open_releases`, sitting beside
`report_issue` in `ipc.rs`, which already opens URLs this way. **Try again**
reuses the existing `check_updates` command and closes the alert; whatever that
check returns then opens a fresh alert through the same four constructors, so a
repeated failure is a repeated alert rather than a window that mutates in place.
Escape and Done both close; Enter fires the accent button.

`platform::open_accessibility_settings()` opens
`x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility`.
The action is constructed only when the grant was lost, which cannot happen on
Windows.

### Sizing and placement

The body ranges from one line to a paragraph plus a warning box, so the page
measures itself and reports height through the existing `panel_height`
mechanism. `WindowAction::Resize` currently lands on the panel unconditionally
(`main.rs:183`); it is routed by `Surface` instead. A pure
`centre_origin(size, work_area)` centres the window on the monitor under the
pointer, testable the way `panel_origin` already is.

### What is deleted

`installer::announce`, `announce_update`, `announce_failure` and `show_dialog`,
and with them the `osascript` shellout and the `eprintln!` fallback. Windows
gets visible update alerts for the first time.

### The activation risk, and how it is handled

The app runs as an `Accessory` on macOS (`main.rs:72`) — no Dock icon, never the
active application. Two of the four alerts appear unprompted, one of them
seconds after `relaunch_and_exit` restarted the process. Whether such a window
reliably comes to the front is the least certain part of this design.

It is handled in three layers, of which the first two mean focus is never
load-bearing:

1. **Visibility.** `with_always_on_top(true)` sets the floating window level,
   and window levels order globally rather than per-application. `Panel`
   already relies on this and appears over other applications today.

2. **Operability.** `with_accept_first_mouse(true)`, for the reason already
   recorded at `panel.rs:114`: without it the click that raises the window is
   spent focusing it and the control under the pointer never fires. Every
   button therefore works on the first click, whether or not the window is key.

3. **Keyboard.** Escape and Enter are a convenience. If an unfocused alert does
   not receive them, nothing is blocked, because every action has a button.
   This is the difference from `display dialog`, where OK is the only way out.

### Making it testable

The post-restart alert is otherwise nearly impossible to exercise — it would
take cutting a real release and letting the app self-update. A hidden flag,
following the pattern `main.rs:39–50` already establishes for
`--export-iconset` and `--settings-window`:

```
vnkey --show-alert updated-needs-permission
vnkey --show-alert install-failed
vnkey --show-alert up-to-date
vnkey --show-alert check-failed
```

Each triggers its alert at `StartCause::Init`, which is where the real
post-restart alert fires, so the launch-time path is exercised for real on both
platforms. An unrecognised kind exits with status 2 and a usage line, as
`--export-iconset` already does.

### If layer 1 proves insufficient

The fallback is explicit activation. No new dependency is required:
`macos.rs` already does raw `extern "C"` FFI against CoreGraphics and
ApplicationServices, so an `objc_msgSend` shim calling
`[NSApp activateIgnoringOtherApps:YES]` fits the existing style, isolated to one
function called at show time.

---

## Part C — The panel's Vietnamese switch

The switch is machine-wide and stays machine-wide. What changes is the line
underneath it, at `panel.js:63`, which today resolves to `"On in Safari"` or
`"Off in Safari"` and so reads as a Safari switch.

The subtitle now only ever describes global scope:

| Excluded applications | Subtitle |
|---|---|
| none | `Everywhere` |
| one | `Everywhere except 1 app` |
| more | `Everywhere except <n> apps` |

Standing inside an excluded application is worth saying, but not there. It
moves to a warning row using the `.warn` box the panel already carries for the
secure-input notice, reading `<app> is one of them`, hidden otherwise:

```
⚠ Secure input on — typing paused        (existing, unchanged)

PhaciusKey                                                   v0.0.25
Vietnamese typing                                     ⌃ ⇧ V   ●───
Everywhere except 3 apps
⚠ Safari is one of them
```

The subtitle string is built in `payload.rs` as `excluded_summary`, not in the
page. Presentation logic in the payload is a slight anomaly — the payload
otherwise carries state — and it is accepted here to buy a unit test on the
counting and pluralisation, since the repository has no JavaScript test
harness. The page assigns the field and does no arithmetic.

`excluded_here` and `current_app` are already in the payload
(`payload.rs:48–52`) and carry the warning row. The panel stops reading
`vietnamese_here`, which stays in the payload for the settings window — one
payload serves every surface, and a surface ignoring a field it does not use is
the existing arrangement.

`panel.html` gains one element for the warning row. The panel measures its own
height, so the row appearing and disappearing needs no sizing work.

---

## Testing

Unit tests, run by `cargo test --workspace`:

- `parse_shortcut` accepts `ctrl+shift` and `ctrl+alt+shift`; rejects `shift`,
  `ctrl`, and four modifiers. The existing assertion that `"ctrl+shift"` is
  `None` is inverted, and the change is recorded here rather than in a comment.
- `shortcut_parts("ctrl+shift")` is `["⌃", "⇧"]`.
- `shortcut_from_event` with `code: None` yields `"ctrl+shift"`, and refuses a
  single modifier.
- `ChordWatch` against sequences: `⌃↓ ⇧↓ ⇧↑ ⌃↑` fires; `⌃↓ ⇧↓ C↓ ⇧↑ ⌃↑` does
  not; `⌃↓ ⇧↓ ⌥↓ ⌥↑ ⇧↑ ⌃↑` does not; `⌃↓ ⌃↑` does not.
- The `AtomicU32` shortcut cache round-trips every valid shape.
- The four `Notice` constructors: their content, and that the accessibility
  action appears only when the grant was lost.
- `centre_origin` on a work area that does not start at the origin, matching
  the existing `panel_origin` coverage.
- The alert page assembles with the theme and its own parts.
- `excluded_summary` for none, one and several excluded applications.
- `open_accessibility` and `open_releases` added to the existing *every command
  the pages send is understood* test.

By hand, on macOS:

- All four alerts via `--show-alert`, confirming each appears in front of a
  focused full-screen application and that its buttons work on the first click.
- `⌃⇧` toggles typing; `⌃⇧C` in a browser opens dev tools and does not toggle;
  `⌃⇧` held with `⌥` added does not toggle.
- Recording `⌃⇧` by holding and releasing, and `⌃⇧V` by holding and pressing.
- The panel row in all three states: no exclusions, exclusions set while
  standing outside them, and standing inside one. The subtitle text itself is
  unit-tested; what this checks is the layout and the warning row.
- Flipping the switch while standing in an excluded application: the switch
  moves, the warning row stays, and typing stays off there. This is the case
  the old copy made incoherent.

Windows remains unverified on real hardware, as with 0.0.24, but is written and
kept lint-clean.

## Out of scope

- Queuing or stacking multiple simultaneous alerts.
- Changing when update checks happen, or the daily/15-minute cadence.
- Notification-centre integration.
- Any modifier-only shortcut for anything other than the toggle.
- What the toggle does inside an excluded application. A modifier-only
  shortcut reaches `state::toggle_vietnamese()` by the same route a keyed one
  does, so that behaviour is whatever it is today, unchanged. Part C changes
  how that state is described, never what it is.
- Making the panel switch per-application. It is machine-wide by design, and
  the exclusion list is the per-application mechanism.
