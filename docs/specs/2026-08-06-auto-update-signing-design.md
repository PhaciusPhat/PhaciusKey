# Auto-update: stable signing identity + resilient checks

**Date:** 2026-08-06 · **Status:** implemented in 0.0.11
**Reference:** [m_capture](https://github.com/tuyen-nguyen-mesoneer/m_capture)
(CONTRIBUTING.md → Releasing, `Sources/Updater.swift`, `.github/workflows/release.yml`)

## Problem

The auto-update shipped in 0.0.10 downloads, swaps and relaunches correctly,
but has two gaps:

1. **Every release is ad-hoc signed** (`codesign --sign -`), so each build
   carries a fresh signing identity. macOS keys the Accessibility (TCC) grant
   to the certificate, so every auto-update silently revokes the grant —
   Vietnamese typing goes dead until the user re-grants and relaunches. This
   defeats the point of silent updates.
2. **The update check runs once, at launch.** PhaciusKey is a menu-bar agent
   that runs for weeks, so a launch-only check means "once per login", not
   "daily". Worse, a login-item launch races Wi-Fi/VPN coming up; a user whose
   check always loses that race never auto-updates at all.

## Design

Adopt m_capture's release model wholesale, adapted to this repo:

### Stable shared signing identity

- One self-signed code-signing certificate, **`phaciuskey-release`**
  (CN=phaciuskey-release, 10-year validity). Self-signed is enough: TCC only
  needs the identity to be *stable*, not Apple-trusted. Gatekeeper is already
  handled by the installer stripping `com.apple.quarantine`.
- `scripts/package-app.sh` signs with that identity when present, pins the
  canonical cert by SHA-1 (`RELEASE_CERT_SHA`) and aborts on a same-named but
  different cert. `PHACIUSKEY_ALLOW_ADHOC=1` keeps personal from-source builds
  working.
- `.github/workflows/release.yml` imports the cert from two secrets
  (`RELEASE_CERT_P12_BASE64`, `RELEASE_CERT_PASSWORD`) into a throwaway
  keychain, refuses to release when the secret is missing, and deletes the
  keychain afterwards.
- The cert/key/p12 live outside git (`.signing/` is gitignored); the .p12 +
  password are the release credential.

### Resilient update checks

- `update::check_for_newer()` now distinguishes "up to date" (`Ok(None)`)
  from "check failed" (`Err`), instead of collapsing both into `None`.
- `main::spawn_update_check` loops for the process lifetime: check at launch,
  then daily; a failed check retries after 15 minutes (network race at login,
  GitHub per-IP rate limiting).
- `spawn_update_install` gains an `AtomicBool` guard so a re-announced update
  can't start a second concurrent download/swap.

Not ported from m_capture: the pending-version bookkeeping and busy-guard —
PhaciusKey relaunches immediately after a swap (an IME restart is instant and
loses no user state), so there is no window where a stale "pending" build sits
on disk.

### First update after this change

0.0.10 → 0.0.11 swaps an ad-hoc-signed bundle for a `phaciuskey-release`-signed
one, so the grant is lost **one last time**; the post-update dialog already
explains the re-grant. Updates between stable-signed releases (0.0.11 → …)
keep the grant.

## Alternatives considered

- **Developer ID + notarization** — the "proper" fix, but needs a paid Apple
  Developer account; the self-signed shared identity achieves grant stability
  today and Developer ID can replace it later by updating `RELEASE_CERT_SHA`.
- **Sparkle-style appcast/feed** — more moving parts than the GitHub releases
  feed already in use; no benefit at this scale.
