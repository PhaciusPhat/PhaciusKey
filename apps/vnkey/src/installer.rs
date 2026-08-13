use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

pub struct Outcome {
    pub version: String,
    pub permission_kept: bool,
}

pub fn app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension()?.eq_ignore_ascii_case("app")).then(|| bundle.to_path_buf())
}

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

    let staged = work.join("PhaciusKey.app");
    let result = stage_from_mount(&mount, &staged);
    let _ = run("hdiutil", &["detach", "-quiet", &mount.to_string_lossy()]);
    result?;

    swap_in_place(&staged, &target)?;
    let _ = std::fs::remove_dir_all(&work);

    Ok(Outcome {
        version: version.to_string(),
        permission_kept: crate::platform::permission_granted(),
    })
}

#[cfg(not(target_os = "macos"))]
pub fn install(_version: &str) -> Result<Outcome, String> {
    Err("in-place self-update is implemented for macOS only".into())
}

#[cfg(target_os = "macos")]
fn stage_from_mount(mount: &Path, staged: &Path) -> Result<(), String> {
    let src = mount.join("PhaciusKey.app");
    if !src.is_dir() {
        return Err(format!("{} not found in the disk image", src.display()));
    }

    run(
        "codesign",
        &["--verify", "--deep", "--strict", &src.to_string_lossy()],
    )
    .map_err(|e| format!("downloaded app failed signature verification: {e}"))?;

    run(
        "ditto",
        &[&src.to_string_lossy(), &staged.to_string_lossy()],
    )
    .map_err(|e| format!("could not stage the new app: {e}"))?;

    let _ = run(
        "xattr",
        &["-dr", "com.apple.quarantine", &staged.to_string_lossy()],
    );

    Ok(())
}

#[cfg(target_os = "macos")]
fn swap_in_place(staged: &Path, target: &Path) -> Result<(), String> {
    let backup = target.with_file_name(format!(
        "{}.old",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&backup);

    std::fs::rename(target, &backup)
        .map_err(|e| format!("cannot move the current app aside (need write access?): {e}"))?;

    if let Err(e) = run(
        "ditto",
        &[&staged.to_string_lossy(), &target.to_string_lossy()],
    ) {
        let _ = std::fs::remove_dir_all(target);
        let _ = std::fs::rename(&backup, target);
        return Err(format!("install failed, previous version restored: {e}"));
    }

    let _ = std::fs::remove_dir_all(&backup);

    let _ = run(
        "xattr",
        &["-dr", "com.apple.quarantine", &target.to_string_lossy()],
    );

    Ok(())
}

pub fn relaunch_and_exit(app: &Path) -> ! {
    #[cfg(target_os = "macos")]
    {
        // `-g` leaves the relaunched app unactivated. Without it the app takes
        // the keyboard from whatever the user is typing in, because it is a
        // Dock app rather than the agent it once was.
        let script = format!(
            "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; open -g -n '{app}'",
            pid = std::process::id(),
            app = app.to_string_lossy().replace('\'', "'\\''"),
        );
        let _ = Command::new("sh").arg("-c").arg(script).spawn();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;

    std::process::exit(0)
}

#[cfg(target_os = "macos")]
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
