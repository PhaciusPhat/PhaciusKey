# PhaciusKey — Market Gap Analysis & Feature Proposal

**Date:** 2026-08-10
**Status:** Proposal for review
**Constraints agreed up front:** strictly on-device (no network, no bundled ML model);
parity before differentiation; macOS-first, Windows later.

---

## 0. Research caveat — read before trusting the matrix

**EVKey and GoTiengViet could not be verified.** This network's Cloudflare Zero Trust
gateway intercepts `evkey.vn` (TLS cert resolves to `no-matching-host-in-cert.teams.cloudflare.com`),
`gotiengviet.com`, and `web.archive.org`; `vietkey.net` returned 403. The session's web-search
quota was also exhausted, so Vietnamese-language forum and app-store complaint mining
did not happen.

EVKey is the most-used macOS Vietnamese IME and therefore the most important competitor.
Every claim about it below is **unverified** and must be re-checked from an unrestricted
network before any roadmap decision rests on it.

Verified from primary sources: UniKey (unikey.org), OpenKey (GitHub README + issue
tracker), VietKey (vi.wikipedia.org), Windows built-in IME (Microsoft Learn).

---

## 1. Where PhaciusKey actually stands

Corrected against the code, not the README:

| Claimed in README | Reality |
|---|---|
| "Per-app on/off memory (EVKey-style)" | **Not implemented.** `per_app_mode`/`app_modes` survive only as retired keys drained into a flat `disabled_apps` exclusion list (`config.rs:42-43,114`) and never written back (`config.rs:633-634`). |
| "Menu-bar control (toggle, method, tone placement, auto-restore)" | Panel has toggle/method/placement; **no auto-restore control**. |
| "Click the VN icon → Help" | No Help item exists; the cheatsheet lives in Settings → About. |

Genuine strengths worth defending — these are not table stakes, competitors mostly lack them:

- **Per-app slow typing and autocomplete fix.** OpenKey's issue tracker shows broken typing
  in Telegram, Terminal and Electron apps is a live, unsolved complaint. PhaciusKey already
  ships targeted workarounds for exactly that class.
- **Backspace resumes the previous word** (16-word history) — not observed in any competitor.
- **Toggle shortcut recorded by pressing it**, including modifier-only chords with poisoning.
- **Signed auto-update with cert-pinning** so the Accessibility grant survives updates.
- **270 tests over the language engine**, including a corpus and an on-screen simulator.

---

## 2. Verified gaps vs. competitors

Ordered by whether they block someone switching from UniKey/OpenKey.

### 2.1 Legacy charsets — the one real moat we don't have
UniKey (Unicode, TCVN3/ABC, VIQR, VNI, VPS, VISCII, BK HCM1/2, NCR), OpenKey (Unicode,
TCVN3, VNI-Windows, CP1258) and VietKey all ship broad legacy charset support.
PhaciusKey is **Unicode NFC only**, and the 2026-06-25 design doc lists legacy encodings
as an explicit non-goal.

**Recommendation: don't build live charset typing. Build charset conversion as a tool.**
The remaining demand is concentrated in Photoshop/CAD/old databases — a shrinking,
Windows-heavy audience that overlaps poorly with a macOS-first product. A
"convert selection" command captures most of the residual value at a fraction of the cost,
and does not contaminate the hot path or the engine.

### 2.2 Per-app on/off memory — parity gap *and* a broken promise
UniKey has per-app Vietnamese on/off; OpenKey auto-remembers *encoding* per app.
PhaciusKey advertises this and does not have it. **Highest-priority parity item**: it is
already promised to users, and the per-app plumbing (`disabled_apps`, `slow_apps`,
`autocomplete_fix_apps`, focused-app detection) mostly exists.

### 2.3 Convert-selection tools
OpenKey ships a dedicated *Chuyển mã*; VietKey ships diacritic removal. PhaciusKey has
neither. **Remove-diacritics on selection** is the genuinely evergreen one (filenames, URLs,
search queries, legacy systems) and is trivial given `base_vowel()` already exists.

### 2.4 Smaller verified gaps
- **"Simple Telex"** third mode (OpenKey) — low effort, modest value.
- **User-definable key remapping** (UniKey custom input-method definitions) — currently
  hardcoded match arms (`telex.rs:248`, `vni.rs:23`).
- **VIQR input** (UniKey, VietKey) — low value in 2026. Recommend skipping.
- **Vietnamese UI localization.** Every competitor is Vietnamese-first; PhaciusKey's UI is
  hardcoded English while its marketing site is Vietnamese. This is a cheap credibility win.

### 2.5 Ecosystem whitespace — nobody has these
- **Settings backup/export/profiles.** Not confirmed present in *any* surveyed tool.
  PhaciusKey already exports macros (`ui/ipc.rs:206`); generalizing is small.
- **Typing statistics.** Also absent everywhere — but low value and mildly privacy-smelly
  even when local. Recommend declining.

---

## 3. New feature proposals

All filtered to run fully on-device. Where an idea needs local *data* (not a model), the size
cost is stated so it can be judged honestly.

### P1. English-word veto for auto-restore — *highest value per unit of effort*
**Problem.** The validator is purely phonotactic, so an English word that happens to fit
Vietnamese syllable shape still gets diacriticized. The codebase admits this: the test is
literally named `known_ambiguous_english_words_still_convert`.

**Proposal.** Add a compact English wordlist (top ~10k words, ~60-100 KB, perfectly hashed
or FST-compressed) consulted *only* at word commit as a veto on conversion.

**Why it fits.** Purely local data, no model. Off the per-keystroke hot path — it runs at
word boundary, where there is far more slack. Directly attacks the single most common
complaint about every Vietnamese IME: mangling English while code-switching.

**Effort:** Low-medium. **Risk:** Low — a veto can only reduce false conversions.

### P2. Per-app rules engine — consolidation that is also a feature
**Problem.** Per-app behaviour has fragmented into three parallel string lists matched by
**display name**, so two apps sharing a name collide and a renamed app silently drops out.

**Proposal.** One per-app rules table keyed on **bundle ID**, carrying: Vietnamese on/off
(delivering §2.2), input method, tone placement, slow typing, autocomplete fix, snippets on/off.

**Why it fits.** Same primitive Karabiner uses (`frontmost_application_if`). Resolves once per
app-switch and caches, never per keystroke. Closes the parity gap and the naming fragility in
one change.

**Effort:** Medium (needs a config migration). **Risk:** Medium — migration must be lossless.

### P3. Local user-vocabulary learning (Mozc `UserHistoryPredictor` model)
**Proposal.** Learn (bare form → committed accented form) pairs the user actually types,
ranked by an LRU + max-merge frequency rule (Rime's approach: take the max of old and new
frequency, so one typo doesn't poison ranking but repetition rises fast). Bigram links so
"xin" primes "chào". Stored as an inspectable text file. "Forget this" and "clear history".

**Why it fits especially well here.** Mozc explicitly refuses to learn from privacy-sensitive
input. **PhaciusKey already detects Secure Input** (`IsSecureEventInputEnabled`, polled every
2s) — that is exactly the gate needed, already half-built. No shipped corpus; the file grows
only with the user's own typing (tens of KB to a few MB, LRU-bounded).

**Effort:** Medium. **Risk:** Medium — needs a visible, trustworthy "forget" story.

### P4. Tone-cycle key — candidate selection without a candidate window
**Proposal.** A key that cycles the just-typed syllable through its legal diacritic
interpretations: `hoa` → `hòa` → `hoà` → `hoạ` → … back to raw.

**Why it fits.** PhaciusKey already has Esc → restore-raw and a full syllable validator to
enumerate legal forms. It follows SKK's philosophy — when disambiguation is expensive, give
the user a cheap explicit signal instead of guessing. No dictionary, no model, no popup UI.
Not observed in any Vietnamese IME.

**Effort:** Low-medium. **Risk:** Low — purely additive, one new key.

### P5. Snippet upgrade (Espanso-style)
Macros today are literal whole-word string→string. Add: cursor placement marker, date/time
expansion, and **case propagation** (`ALH` → `ALTHOUGH`). Low effort, cleanly separable,
expansion fires only at trigger completion.

### P6. Bare-ASCII diacritic restoration — the flagship, and the one with a real cost
**Proposal.** Type or paste `toi muon di choi`, hit a key, get `tôi muốn đi chơi`. Enumerate
each bare syllable's accented candidates from a static table, then Viterbi-decode the sentence
with an n-gram frequency table. The pre-neural, fully-local approach; reported ~90-98% word
accuracy depending on n-gram order and corpus.

**The honest cost.** The syllable table is small (Vietnamese has only a few thousand
syllables). The n-gram table is the problem: word-level trigrams run to **tens of MB**, against
a current binary of 2.3 MB. A syllable-level unigram+bigram table (~1-5 MB compressed) is the
realistic compromise, trading accuracy for size.

**This is the one proposal that strains the "no bundled model" constraint** — it is data, not a
model, but it is *big* data by this project's standards. Flagged for an explicit decision.

**Effort:** High. **Risk:** Medium-high (binary size, and it only runs on explicit invocation,
never per keystroke).

---

## 4. Recommended sequencing

**Phase 1 — truth and parity (low effort, unblocks switchers)**
1. Fix the three stale README claims, or implement what they promise.
2. P2 per-app rules engine keyed on bundle ID → delivers per-app on/off memory (§2.2).
3. Settings export/import (§2.5) — generalize the existing macro export.
4. Remove-diacritics on selection (§2.3).
5. Vietnamese UI localization (§2.4).

**Phase 2 — differentiate (medium effort, high value)**
6. P1 English-word veto.
7. P4 tone-cycle key.
8. P5 snippet upgrade.
9. P3 local vocabulary learning.

**Phase 3 — flagship, only if the size cost is accepted**
10. P6 diacritic restoration.

**Recommend declining:** live legacy-charset typing (§2.1 — build the conversion tool
instead), VIQR (§2.4), typing statistics (§2.5).

---

## 5. Cross-cutting risk: the macOS/Windows split widens

Per-app exclusions, slow typing and autocomplete fix are already **silently inert on Windows**
— `set_current_app` is never called there, so `current_app` is always `None` and
`excluded_for(None)` is always `false`. P2 makes per-app behaviour more central, deepening a
split where a Windows user sees settings that do nothing.

**Mitigation:** either implement foreground-window detection on Windows alongside P2, or have
the settings UI visibly disable per-app controls on platforms that cannot honour them.
Silently-dead settings are worse than absent ones.

---

## 6. Open questions

1. **EVKey verification.** Re-run the competitor scan from an unrestricted network before
   committing to Phase 1. If EVKey already has P1/P3/P4, differentiation must move.
2. **P6 binary size.** Is 1-5 MB of n-gram data acceptable, or is the small-binary property
   worth more than the feature?
3. **Legacy charsets.** Confirm the conversion-tool-not-typing-mode call — it is the single
   largest verified parity gap and the recommendation is to deliberately not close it fully.
