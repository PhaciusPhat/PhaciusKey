//! In-place self-update: download the new release, swap the app bundle, relaunch.
//!
//! # The Accessibility caveat
//!
//! macOS binds the Accessibility (TCC) grant to the app's **code signature**.
//! Replacing the bundle keeps the grant only if the new bundle carries the *same*
//! stable signing identity. `scripts/package-app.sh` ad-hoc signs
//! (`codesign --sign -`), which mints a fresh ad-hoc identity per build — so
//! after an update macOS treats the app as a different program and asks for
//! Accessibility again. That is a property of the signing setup, not of this
//! code: with a Developer ID certificate plus notarization, this same flow
//! becomes genuinely permission-preserving with no changes here.
//!
//! [`install`] therefore reports whether the grant survived, and the app tells
//! the user the truth either way rather than silently stopping working.
//!
//! # Safety of the swap
//!
//! The download is verified with `codesign --verify` before anything is touched
//! (this catches a truncated or corrupt DMG even for ad-hoc signatures). The old
//! bundle is moved aside rather than deleted, and restored if the copy fails, so
//! a failed update leaves a working app. Nothing runs unless the process is
//! actually inside a `.app` — a `cargo run` build never self-updates.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What an update did, for the message shown to the user afterwards.
pub struct Outcome {
    /// Version that was installed.
    pub version: String,
    /// Whether Accessibility permission survived the swap. False with ad-hoc
    /// signing, so the user has to re-grant it — see the module docs.
    pub permission_kept: bool,
}

/// The `.app` bundle this process is running from, or `None` when it is not
/// inside one (`cargo run`, a bare binary). Self-update is skipped when `None`.
pub fn app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // …/PhaciusKey.app/Contents/MacOS/vnkey → …/PhaciusKey.app
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension()?.eq_ignore_ascii_case("app")).then(|| bundle.to_path_buf())
}

/// Download release `version` and replace the running bundle with it.
///
/// Blocking and slow (network + disk); run on a background thread.
#[cfg(target_os = "macos")]
pub fn install(version: &str) -> Result<Outcome, String> {
    let target = app_bundle().ok_or("not running from a .app bundle; skipping self-update")?;

    let work = std::env::temp_dir().join(format!("phaciuskey-update-{version}"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("cannot create {}: {e}", work.display()))?;

    let dmg = work.join(format!("PhaciusKey-{version}.dmg"));
    let url = crate::update::dmg_url(version);
    run(
        "curl",
        &[
            "-fsSL",
            "--max-time",
            "180",
            "-o",
            &dmg.to_string_lossy(),
            &url,
        ],
    )
    .map_err(|e| format!("download failed ({url}): {e}"))?;

    let mount = work.join("mnt");
    std::fs::create_dir_all(&mount).map_err(|e| format!("cannot create mountpoint: {e}"))?;
    run(
        "hdiutil",
        &[
            "attach",
            "-nobrowse",
            "-readonly",
            "-quiet",
            "-mountpoint",
            &mount.to_string_lossy(),
            &dmg.to_string_lossy(),
        ],
    )
    .map_err(|e| format!("could not mount {}: {e}", dmg.display()))?;

    // From here on, always unmount before returning.
    let staged = work.join("PhaciusKey.app");
    let result = stage_from_mount(&mount, &staged);
    let _ = run("hdiutil", &["detach", "-quiet", &mount.to_string_lossy()]);
    result?;

    swap_in_place(&staged, &target)?;
    let _ = std::fs::remove_dir_all(&work);

    Ok(Outcome {
        version: version.to_string(),
        // Re-read the live TCC state rather than assuming: if the user ever moves
        // to a stable Developer ID signature this starts reporting true on its own.
        permission_kept: crate::platform::permission_granted(),
    })
}

#[cfg(not(target_os = "macos"))]
pub fn install(_version: &str) -> Result<Outcome, String> {
    Err("in-place self-update is implemented for macOS only".into())
}

/// Copy the app out of the mounted DMG and verify it before it goes anywhere near
/// the installed bundle.
#[cfg(target_os = "macos")]
fn stage_from_mount(mount: &Path, staged: &Path) -> Result<(), String> {
    let src = mount.join("PhaciusKey.app");
    if !src.is_dir() {
        return Err(format!("{} not found in the disk image", src.display()));
    }

    // Verifies the bundle hashes; catches a truncated or tampered download even
    // though the signature itself is ad-hoc.
    run(
        "codesign",
        &["--verify", "--deep", "--strict", &src.to_string_lossy()],
    )
    .map_err(|e| format!("downloaded app failed signature verification: {e}"))?;

    // `ditto` (not fs::copy) preserves the bundle's symlinks, extended
    // attributes and signature.
    run(
        "ditto",
        &[&src.to_string_lossy(), &staged.to_string_lossy()],
    )
    .map_err(|e| format!("could not stage the new app: {e}"))?;

    // curl-downloaded files are quarantined; left in place, Gatekeeper would
    // block the relaunch of this unnotarized build.
    let _ = run(
        "xattr",
        &["-dr", "com.apple.quarantine", &staged.to_string_lossy()],
    );

    Ok(())
}

/// Replace `target` with `staged`, keeping the old bundle until the copy lands.
#[cfg(target_os = "macos")]
fn swap_in_place(staged: &Path, target: &Path) -> Result<(), String> {
    let backup = target.with_file_name(format!(
        "{}.old",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&backup);

    std::fs::rename(target, &backup)
        .map_err(|e| format!("cannot move the current app aside (need write access?): {e}"))?;

    // Cross-volume, so copy rather than rename.
    if let Err(e) = run(
        "ditto",
        &[&staged.to_string_lossy(), &target.to_string_lossy()],
    ) {
        // Put the working app back before giving up.
        let _ = std::fs::remove_dir_all(target);
        let _ = std::fs::rename(&backup, target);
        return Err(format!("install failed, previous version restored: {e}"));
    }

    let _ = std::fs::remove_dir_all(&backup);

    // Strip quarantine again at the installed path. It was already cleared on the
    // staged copy, but `ditto` propagates extended attributes, and a quarantine
    // flag surviving here is exactly what makes macOS refuse to open the app
    // ("PhaciusKey is damaged" / unidentified developer) on the relaunch below.
    let _ = run(
        "xattr",
        &["-dr", "com.apple.quarantine", &target.to_string_lossy()],
    );

    Ok(())
}

/// Relaunch the bundle once this process has exited, then exit.
///
/// `open -n` cannot start the app while the old instance is still alive, so the
/// helper waits for this PID to disappear first.
pub fn relaunch_and_exit(app: &Path) -> ! {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; open -n '{app}'",
            pid = std::process::id(),
            app = app.to_string_lossy().replace('\'', "'\\''"),
        );
        let _ = Command::new("sh").arg("-c").arg(script).spawn();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;

    std::process::exit(0)
}

/// Show a modal "you were updated" dialog.
///
/// Spawned detached: `osascript` blocks until the user dismisses it, and this is
/// called from the event loop.
pub fn announce_update(from: &str, to: &str, needs_permission: bool) {
    let mut body = format!(
        "PhaciusKey has been updated from {from} to {to} and restarted automatically."
    );
    if needs_permission {
        // Be explicit: with ad-hoc signing the grant cannot survive, and silently
        // dead Vietnamese typing is far worse than saying so.
        body.push_str(
            "\n\nmacOS needs you to allow Accessibility once more, because each build \
             is signed with a different ad-hoc identity. Open System Settings → \
             Privacy & Security → Accessibility and enable PhaciusKey — typing stays \
             off until then.",
        );
    }
    show_dialog(&body);
}

/// Tell the user an automatic update failed. They can still update by hand.
pub fn announce_failure(version: &str, error: &str) {
    show_dialog(&format!(
        "PhaciusKey could not install version {version} automatically.\n\n{error}\n\n\
         The current version keeps working; you can update manually from the menu."
    ));
}

fn show_dialog(body: &str) {
    #[cfg(target_os = "macos")]
    {
        let escaped = body.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "display dialog \"{escaped}\" with title \"PhaciusKey\" buttons {{\"OK\"}} \
             default button \"OK\" with icon note"
        );
        let _ = Command::new("osascript").arg("-e").arg(script).spawn();
    }
    #[cfg(not(target_os = "macos"))]
    eprintln!("[vnkey] {body}");
}

/// Run a command, mapping a non-zero exit into an `Err` carrying its stderr.
fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("{program} exited with {}", output.status)
    } else {
        stderr
    })
}
