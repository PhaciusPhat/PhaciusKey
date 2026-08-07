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
/// Reads the public `releases.atom` feed rather than the REST API: the
/// unauthenticated API allows 60 requests/hour **per IP**, and users behind a
/// shared egress (Cloudflare WARP, corporate NAT — common in Vietnam) found it
/// permanently exhausted ("not a release list"). The feed is uncapped, and —
/// unlike `/releases/latest` — includes prereleases, which our releases are.
pub fn check_for_newer() -> Result<Option<String>, String> {
    let output = std::process::Command::new("curl")
        .args([
            "-sL",
            "--max-time",
            "8",
            "-H",
            "User-Agent: PhaciusKey",
            &format!("https://github.com/{REPO}/releases.atom"),
        ])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !output.status.success() {
        return Err(format!("curl exited with {}", output.status));
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let newest = newest_version_in_atom(&body)
        .ok_or("unexpected response from GitHub: no release tags in the feed")?;

    Ok(Some(newest).filter(|v| is_newer(v, CURRENT)))
}

/// Highest `vX.Y.Z` across the feed's `…/releases/tag/v…` references. `None`
/// when the body carries no tags at all (an error page, an empty feed) — a
/// failed check, never "up to date". Draft releases are unpublished and never
/// appear in the feed, so no draft filter is needed.
fn newest_version_in_atom(body: &str) -> Option<String> {
    const MARKER: &str = "/releases/tag/v";
    body.match_indices(MARKER)
        .map(|(i, _)| {
            body[i + MARKER.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<String>()
        })
        .filter(|v| !v.is_empty())
        .max_by_key(|v| parse(v))
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
    fn finds_the_newest_version_in_an_atom_feed() {
        let feed = r#"<?xml version="1.0" encoding="UTF-8"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
          <entry>
            <id>tag:github.com,2008:Repository/1/v0.0.9</id>
            <link rel="alternate" href="https://github.com/x/y/releases/tag/v0.0.9"/>
          </entry>
          <entry>
            <id>tag:github.com,2008:Repository/1/v0.0.20</id>
            <link rel="alternate" href="https://github.com/x/y/releases/tag/v0.0.20"/>
          </entry>
        </feed>"#;
        assert_eq!(newest_version_in_atom(feed), Some("0.0.20".to_string()));
    }

    #[test]
    fn rejects_bodies_without_release_tags() {
        // The exact failure that motivated the atom switch: a rate-limit JSON
        // object from the REST API. Any tagless body must read as a failed
        // check, never as "up to date".
        assert_eq!(newest_version_in_atom(r#"{"message":"API rate limit exceeded"}"#), None);
        assert_eq!(newest_version_in_atom(""), None);
        assert_eq!(newest_version_in_atom("<feed></feed>"), None);
    }

    #[test]
    fn version_ordering() {
        assert!(is_newer("0.0.4", "0.0.3"));
        assert!(is_newer("0.1.0", "0.0.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.0.3", "0.0.3"));
        assert!(!is_newer("0.0.2", "0.0.3"));
    }
}
