//! Lightweight update check and outbound links.
//!
//! We shell out to `curl` (present on macOS and modern Windows) so the app pulls
//! in no TLS/networking crates. The check only *notifies*; it never replaces the
//! app in place — see the note in `main` about why silent self-update needs a
//! stable Developer ID signature to avoid re-granting Accessibility.

/// This build's version, from Cargo.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

const REPO: &str = "PhaciusPhat/phacius_vnkey";

/// URL of the latest release (what "Download update" / "Check for updates" open).
pub fn releases_url() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

/// URL to file a new bug report, pre-filled with the app version.
pub fn new_issue_url() -> String {
    format!(
        "https://github.com/{REPO}/issues/new?title=%5Bbug%5D%20&body={}",
        urlencode(&format!(
            "\n\n---\nphacius_vnkey {CURRENT} · macOS\n(describe the problem, and what you typed)"
        ))
    )
}

/// Query GitHub for the newest release tag. Returns the version (no leading `v`)
/// if it is newer than this build, else `None`. Blocking; run off the main
/// thread.
///
/// Uses the releases *list* rather than `/releases/latest`, because our releases
/// are marked pre-release and `/releases/latest` only returns full releases.
pub fn check_for_newer() -> Option<String> {
    let output = std::process::Command::new("curl")
        .args([
            "-sL",
            "--max-time",
            "8",
            "-H",
            "User-Agent: phacius_vnkey",
            "-H",
            "Accept: application/vnd.github+json",
            &format!("https://api.github.com/repos/{REPO}/releases?per_page=10"),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let releases: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;

    // Highest version across all (non-draft) releases.
    let newest = releases
        .as_array()?
        .iter()
        .filter(|r| !r.get("draft").and_then(|d| d.as_bool()).unwrap_or(false))
        .filter_map(|r| r.get("tag_name")?.as_str())
        .map(|tag| tag.trim_start_matches('v').to_string())
        .max_by_key(|v| parse(v))?;

    if is_newer(&newest, CURRENT) {
        Some(newest)
    } else {
        None
    }
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
