//! Lightweight update check and outbound links.
//!
//! We shell out to `curl` (present on macOS and modern Windows) so the app pulls
//! in no TLS/networking crates. When a newer release is found, `main` hands it to
//! `installer` to download and swap in place. Whether the Accessibility grant
//! survives that swap is a *signing* property, not an updater one: releases must
//! share the `phaciuskey-release` identity (see CONTRIBUTING.md → Releasing).

/// This build's version, from Cargo.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

const REPO: &str = "PhaciusPhat/phacius_vnkey";

/// URL of the latest release (what "Download update" / "Check for updates" open).
pub fn releases_url() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

/// Direct URL of a release's macOS disk image.
///
/// Built from the tag rather than `releases/latest`, which only ever resolves to
/// a non-prerelease and would 302 to the releases list.
pub fn dmg_url(version: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/v{version}/PhaciusKey-{version}.dmg")
}

/// URL to file a new bug report, pre-filled with the app version.
pub fn new_issue_url() -> String {
    format!(
        "https://github.com/{REPO}/issues/new?title=%5Bbug%5D%20&body={}",
        urlencode(&format!(
            "\n\n---\nPhaciusKey {CURRENT} · macOS\n(describe the problem, and what you typed)"
        ))
    )
}

/// Query GitHub for the newest release tag. `Ok(Some(version))` (no leading `v`)
/// when a release newer than this build exists, `Ok(None)` when up to date, and
/// `Err` when the check itself failed — the caller retries failures much sooner
/// than the daily cadence, because the launch check routinely races Wi-Fi/VPN
/// coming up at login. Blocking; run off the main thread.
///
/// Uses the releases *list* rather than `/releases/latest`, because our releases
/// are marked pre-release and `/releases/latest` only returns full releases.
pub fn check_for_newer() -> Result<Option<String>, String> {
    let output = std::process::Command::new("curl")
        .args([
            "-sL",
            "--max-time",
            "8",
            "-H",
            "User-Agent: PhaciusKey",
            "-H",
            "Accept: application/vnd.github+json",
            &format!("https://api.github.com/repos/{REPO}/releases?per_page=10"),
        ])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !output.status.success() {
        return Err(format!("curl exited with {}", output.status));
    }
    let releases: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("unexpected response from GitHub: {e}"))?;

    // Highest version across all (non-draft) releases. An empty or non-array
    // body (e.g. a rate-limit error object) counts as a failed check, not as
    // "up to date".
    let newest = releases
        .as_array()
        .ok_or("unexpected response from GitHub: not a release list")?
        .iter()
        .filter(|r| !r.get("draft").and_then(|d| d.as_bool()).unwrap_or(false))
        .filter_map(|r| r.get("tag_name")?.as_str())
        .map(|tag| tag.trim_start_matches('v').to_string())
        .max_by_key(|v| parse(v));

    Ok(newest.filter(|v| is_newer(v, CURRENT)))
}

/// Open a URL in the user's default browser.
pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = std::process::Command::new("xdg-open");

    let _ = cmd.arg(url).spawn();
}

/// Compare dotted numeric versions (e.g. `0.0.10` > `0.0.9`).
fn is_newer(candidate: &str, current: &str) -> bool {
    parse(candidate) > parse(current)
}

fn parse(v: &str) -> (u32, u32, u32) {
    let mut it = v.split('.').map(|p| p.trim().parse::<u32>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering() {
        assert!(is_newer("0.0.4", "0.0.3"));
        assert!(is_newer("0.1.0", "0.0.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.0.3", "0.0.3"));
        assert!(!is_newer("0.0.2", "0.0.3"));
    }
}
