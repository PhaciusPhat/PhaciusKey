# PhaciusKey — Design Document

**Date:** 2026-06-25
**Status:** Draft for review
**Author:** phat.le@mesoneer.io

---

## 1. Overview

`PhaciusKey` is a Vietnamese input method (IME) for macOS. It lets users type Vietnamese
with diacritics and tone marks using standard keyboard conventions (Telex, VNI). The
codebase is deliberately structured so the language logic is **OS-independent** and can be
reused by future Windows and Linux shells with no changes.

**Design principles:** clean separation of concerns, single-purpose units behind well-defined
interfaces, exhaustive test coverage of the language logic, and a fail-safe shell (a bug never
swallows or corrupts the user's keystrokes — worst case, typing behaves normally).

## 2. Goals & non-goals

### In scope (v1)
- Input methods: **Telex** and **VNI**.
- Smart behaviors:
  - **Valid-syllable spell check** — only apply a transform if the result is a legal Vietnamese syllable.
  - **Auto-restore for non-Vietnamese** — keep raw keystrokes when a word can't form a valid syllable (e.g. English).
  - **Modern tone placement** (default) with a **Classic** toggle (`hoà` vs `hòa`).
  - **Per-app enable/disable** + a **global hotkey** to toggle Vietnamese typing on/off.
- macOS menu-bar app + preferences window, styled after mesoneer.io.
- Unicode **NFC** output.

### Out of scope (v1 — explicitly deferred)
- VIQR and Simple-Telex input methods (architecture leaves room; not built yet).
- Legacy encodings (TCVN3, VNI-Windows, VISCII).
- Windows / Linux shells (the **core is built to enable them**, but no shell is shipped in v1).
- Macros / text expansion, dictionary-based autocomplete, cloud sync.

## 3. Architecture

One portable brain (Rust), thin per-OS bodies (Swift today; Windows/Linux later).

```
┌─────────────────────────────────────────────────────────┐
│  macOS Shell (Swift)                                      │
│  ┌──────────────┐  ┌────────────┐  ┌──────────────────┐  │
│  │ CGEventTap   │  │ Menu-bar   │  │ Preferences      │  │
│  │ key listener │  │ status app │  │ window (SwiftUI) │  │
│  └──────┬───────┘  └────────────┘  └──────────────────┘  │
│         │ keystrokes in / edit-actions out                │
│  ┌──────▼──────────────────────────────────────────────┐ │
│  │  FFI bridge (C ABI)  —  PhaciusKey_engine.h      │ │
│  └──────┬──────────────────────────────────────────────┘ │
└─────────┼─────────────────────────────────────────────────┘
          │
┌─────────▼─────────────────────────────────────────────────┐
│  Core engine (Rust)  —  ZERO OS dependencies, pure logic   │
│   • input methods (Telex, VNI)   • syllable validator      │
│   • composition buffer / state    • tone placement rules   │
│   • auto-restore logic            • outputs EditActions     │
└────────────────────────────────────────────────────────────┘
```

**The contract:** the core never knows what OS it runs on. It receives one keystroke plus the
current configuration/state and returns a list of **`EditAction`s** (e.g. `{backspace: 2,
insert: "ấ"}`). The shell is the only component that knows about `CGEventTap`, `NSStatusItem`,
or SwiftUI. A new OS = a new shell over the **same** core.

## 4. Repository layout

```
PhaciusKey/
├── README.md
├── crates/
│   ├── vnkey-core/         # pure engine: methods, validator, buffer, tones
│   │   ├── src/lib.rs
│   │   └── tests/          # exhaustive table-driven unit tests
│   └── vnkey-ffi/          # C-ABI wrapper; builds static/dylib + header
│       ├── src/lib.rs
│       └── cbindgen.toml   # auto-generates PhaciusKey_engine.h
├── apps/
│   └── macos/              # Swift app — the ONLY OS-specific code
│       ├── Sources/
│       │   ├── EventTap/   # CGEventTap + action executor
│       │   ├── MenuBar/    # NSStatusItem
│       │   ├── Preferences/# SwiftUI, mesoneer-styled
│       │   └── Bridge/     # thin Swift wrapper over the C header
│       └── Package.swift
├── docs/specs/             # design docs
└── scripts/                # build-engine.sh (cargo build → copy lib + header)
```

## 5. Core engine design (`vnkey-core`, Rust)

| Unit | Responsibility | Depends on |
|------|----------------|------------|
| `Keystroke` / `EditAction` | Input/output value types crossing the boundary | — |
| `InputMethod` (trait) | Map raw keys → transform intents. Impls: `Telex`, `Vni` | types |
| `CompositionBuffer` | Hold raw keys since last word boundary; recompute target word; diff vs on-screen text → minimal `EditAction`s | types |
| `SyllableValidator` | Model Vietnamese phonotactics (onset / nucleus / coda + tone); answer "is this a legal syllable?" | — |
| `TonePlacement` | Decide mark position: `Modern` (default) or `Classic` | validator |
| `Engine` | Orchestrate: key → method → validate → place tone → buffer diff → actions; commit/reset on word boundary | all above |

**Key flows:**
- **Composition:** each key updates the buffer, the engine recomputes the target Vietnamese word
  and emits the *minimal* `{backspace: n, insert: "…"}` diff against what's currently displayed.
- **Valid-syllable spell check:** a transform is applied only if it yields a legal syllable;
  otherwise the key is passed through literally.
- **Auto-restore:** if the buffer cannot form a valid syllable (e.g. `"test"`), the raw
  keystrokes are restored instead of forcing diacritics.
- **Word boundaries:** space, punctuation, navigation keys, and a shell-supplied
  focus-change/mouse-click signal all commit and reset the buffer.
- **Output:** always Unicode **NFC**.

The engine is **pure and deterministic** — no I/O, no globals, no time — which makes it
exhaustively unit-testable and identical across operating systems.

## 6. macOS shell design (`apps/macos`, Swift)

- **EventTap** — installs a `CGEventTap` on `keyDown`. For each event it calls the engine; if
  actions return, it **swallows the original event** and synthesizes them; otherwise the key
  passes through untouched.
- **Action executor** — turns `EditAction`s into synthesized `CGEvent`s (backspace key events +
  `keyboardSetUnicodeString` inserts).
- **Shell-owned state** (not the engine's concern): active input method, on/off, per-app enabled
  map, global toggle hotkey. Passed into each engine call; persisted via `UserDefaults`.
- **Per-app tracking** — `NSWorkspace` frontmost-app notifications → look up the bundle ID's
  on/off state.
- **Permissions** — requests **Accessibility** permission on first launch (required for event
  taps) with a clear onboarding explainer.
- **Buffer reset** — mouse click / focus change feeds a reset signal to the engine so composition
  never bleeds across fields.

## 7. UI design (chosen by author, mesoneer style)

**Visual language (from mesoneer.io):** corporate-minimal, primary **blue** (`#1657E0`), dark
navy ink (`#0A1B3D`), clean white surfaces with light-gray (`#F4F7FB`) section fills, bold
sans-serif headings, soft-rounded corners (~10–14px), generous whitespace, flat (no gradients).

### 7.1 Menu-bar dropdown (the everyday surface — kept tiny)
```
┌────────────────────────────┐
│  ● Vietnamese typing   [ON] │   ← toggle (matches global hotkey)
│  ───────────────────────────│
│  Method      Telex  ⌄        │   ← Telex / VNI quick switch
│  ───────────────────────────│
│  Preferences…               │
│  Quit PhaciusKey         │
└────────────────────────────┘
```
The menu-bar icon shows current state at a glance (e.g. **VN** when on, dimmed when off).

### 7.2 Preferences window — **left-sidebar layout** (chosen)
Chosen over top-tabs because a sidebar scales cleanly as future sections (VIQR, macros, updates)
are added, and reads as more "settings-app native."

```
┌──────────────────────────────────────────────────────────┐
│ ● ● ●   PhaciusKey                                      │
├────────────┬─────────────────────────────────────────────┤
│ ⌨︎ General  │  General                                     │
│ ⌘ Hotkey   │  Typing behavior and defaults.               │
│ ▤ Per-app  │                                              │
│ ⓘ About    │  Vietnamese typing            ( ●——) ON      │
│            │  Input method            [ Telex | VNI ]      │
│            │  Tone placement          [ Modern | Classic ] │
│            │   hoà vs hòa                                  │
│            │  Auto-restore English         ( ●——) ON      │
│            │  Start at login               (——● ) OFF     │
└────────────┴─────────────────────────────────────────────┘
```
- **General** — master on/off, input method, tone placement, auto-restore, start-at-login.
- **Hotkey** — recorder control for the global toggle (default `⌃⌥V`).
- **Per-app** — list of apps with individual on/off; "default for new apps" switch.
- **About** — version, links, Accessibility-permission status + re-grant button.

Built in **SwiftUI**, segmented controls for binary/enum choices, toggles in primary blue.

## 8. Cross-OS strategy (why this scales)

"Intercept the global keystroke stream and rewrite it" is the same model on every platform, so
only the shell changes:

| Concern | macOS (v1) | Windows (future) | Linux (future) |
|---------|-----------|------------------|----------------|
| Key capture | `CGEventTap` | `WH_KEYBOARD_LL` hook | IBus / Fcitx engine |
| Emit text | `keyboardSetUnicodeString` | `SendInput` | IBus commit |
| UI | SwiftUI | WinUI / WPF | GTK / Qt |
| **Engine** | **vnkey-core (identical)** | **vnkey-core (identical)** | **vnkey-core (identical)** |

The FFI is a stable C ABI, so any language with C interop can host the core.

## 9. Error handling & safety

- The FFI wrapper **catches Rust panics** at the boundary and returns a safe "pass-through" result;
  panics never cross into Swift.
- If the engine errors or is disabled, the shell **lets the keystroke through unmodified**. The
  failure mode is "typing behaves normally," never a dropped/garbled keystroke or a crash.
- All buffer state is bounded and reset on word boundaries; no unbounded growth.

## 10. Testing strategy

- **Engine (Rust):** table-driven unit tests mapping `(method, keystrokes) → expected text`,
  covering Telex, VNI, every tone/diacritic, tone-placement modes, auto-restore, and invalid
  inputs. Property tests for invariants (idempotent commit, NFC output, no panic on any byte
  sequence). This is where correctness lives — fast, deterministic, no UI.
- **FFI (vnkey-ffi):** round-trip tests through the C ABI; verify panic isolation.
- **Bridge (Swift):** unit tests that the Swift wrapper maps C structs ↔ Swift types correctly.
- **Integration (manual checklist for v1):** type across TextEdit, Safari, Notes, a terminal,
  and an Electron app; verify backspace-replace, per-app toggle, hotkey, and permission flow.
- **CI:** `cargo test` + `cargo clippy` + `swift build`/`swift test` on every push.

## 11. Build & tooling

- `scripts/build-engine.sh` — `cargo build --release -p vnkey-ffi`, run `cbindgen` to emit
  `PhaciusKey_engine.h`, copy the lib + header into the macOS app's expected location.
- Rust workspace via root `Cargo.toml`; macOS app via `Package.swift` (SwiftPM) linking the
  static lib.
- Lint/format: `rustfmt` + `clippy`, `swift-format`.

## 12. Tier & subtasks

> Recorded locally per project convention (no Jira writes).

**Recommended tier: Tier 2 (Standard feature project).** Multi-component, new codebase, but
well-bounded scope with clear interfaces and no cross-team dependencies.

**Subtask breakdown:**
1. **Repo scaffold** — Cargo workspace, `apps/macos` SwiftPM skeleton, `build-engine.sh`, CI, README.
2. **Core types & engine skeleton** — `Keystroke`, `EditAction`, `Engine` orchestration, buffer diffing.
3. **Telex input method** + unit test corpus.
4. **VNI input method** + unit test corpus.
5. **Syllable validator** (phonotactics model) + tests.
6. **Tone placement** (Modern/Classic) + tests.
7. **Auto-restore** logic + tests.
8. **FFI layer** — `vnkey-ffi`, cbindgen header, panic isolation, round-trip tests.
9. **Swift bridge** — wrapper over the C header + tests.
10. **EventTap + action executor** — capture, swallow, synthesize; Accessibility permission flow.
11. **Menu-bar app** — status item, dropdown, icon state.
12. **Preferences window** — sidebar layout, General/Hotkey/Per-app/About; persistence.
13. **Per-app + global hotkey** wiring.
14. **Integration pass** — multi-app manual checklist; polish.

## 13. Open questions / future

- Default global hotkey (`⌃⌥V` proposed) — confirm no common conflict.
- Codesigning / notarization & distribution channel (DMG, Homebrew cask) — out of v1 build, plan before release.
- VIQR / Simple-Telex and legacy encodings remain straightforward additions behind the existing trait.
