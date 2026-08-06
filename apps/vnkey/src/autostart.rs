//! Launch-at-login registration.
//!
//! macOS: a LaunchAgent plist in `~/Library/LaunchAgents` pointing at the
//! bundle's executable. Written/removed to match the setting on every launch,
//! which also self-heals a stale path after the user moves the app. Only a
//! real `.app` bundle is registered — a `cargo run` binary lives in `target/`
//! and would be a broken (and confusing) login item.
//!
//! Errors are logged and swallowed: failing to register autostart must never
//! take the input method down with it.

#[cfg(target_os = "macos")]
const LABEL: &str = "com.phacius.vnkey";

/// Make the on-disk login-item registration match `enabled`.
#[cfg(target_os = "macos")]
pub fn apply(enabled: bool) {
    let Some(dir) = dirs::home_dir().map(|h| h.join("Library/LaunchAgents")) else {
        eprintln!("[vnkey] cannot resolve the home directory; skipping login-item setup");
        return;
    };
    let plist = dir.join(format!("{LABEL}.plist"));

    // Registering needs a bundle to point at; unregistering never does.
    let exe = crate::installer::app_bundle().map(|b| b.join("Contents/MacOS/vnkey"));
    let (Some(exe), true) = (exe, enabled) else {
        if plist.exists() {
            if let Err(e) = std::fs::remove_file(&plist) {
                eprintln!("[vnkey] could not remove {}: {e}", plist.display());
            }
        }
        return;
    };

    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{exe}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
</dict>
</plist>
"#,
        exe = xml_escape(&exe.to_string_lossy()),
    );

    // Skip the write when nothing changed — launchd watches this directory.
    if std::fs::read_to_string(&plist).ok().as_deref() == Some(&content) {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[vnkey] could not create {}: {e}", dir.display());
        return;
    }
    if let Err(e) = std::fs::write(&plist, content) {
        eprintln!("[vnkey] could not write {}: {e}", plist.display());
    }
}

#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// No registration mechanism wired up on this OS yet (Windows would use the
/// `Run` registry key once the Windows shell is verified on hardware).
#[cfg(not(target_os = "macos"))]
pub fn apply(_enabled: bool) {}
