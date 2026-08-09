pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

const REPO: &str = "PhaciusPhat/PhaciusKey";

pub fn releases_url() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

pub fn dmg_url(version: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/v{version}/PhaciusKey-{version}.dmg")
}

pub fn new_issue_url() -> String {
    format!(
        "https://github.com/{REPO}/issues/new?title=%5Bbug%5D%20&body={}",
        urlencode(&format!(
            "\n\n---\nPhaciusKey {CURRENT} · macOS\n(describe the problem, and what you typed)"
        ))
    )
}

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
        assert_eq!(
            newest_version_in_atom(r#"{"message":"API rate limit exceeded"}"#),
            None
        );
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
