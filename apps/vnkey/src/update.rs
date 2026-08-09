pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

const REPO: &str = "PhaciusPhat/PhaciusKey";

/// Where the update cycle has got to. Held here rather than in the tray, so
/// every surface reports the same thing without one of them having to ask
/// another.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Idle,
    Checking,
    Available(String),
    Installing(String),
    Failed(String),
}

impl Status {
    /// A stable name for the page to switch on, separate from the prose.
    pub fn state(&self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Checking => "checking",
            Status::Available(_) => "available",
            Status::Installing(_) => "installing",
            Status::Failed(_) => "failed",
        }
    }

    /// The version the status is about, when it is about one.
    pub fn version(&self) -> Option<&str> {
        match self {
            Status::Available(v) | Status::Installing(v) => Some(v),
            Status::Idle | Status::Checking | Status::Failed(_) => None,
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Status::Idle => format!("{CURRENT} — up to date"),
            Status::Checking => format!("{CURRENT} — checking…"),
            Status::Available(v) => format!("{CURRENT} — version {v} is ready"),
            Status::Installing(v) => format!("{CURRENT} — installing {v}…"),
            Status::Failed(reason) => format!("{CURRENT} — update failed: {reason}"),
        }
    }
}

static STATUS: std::sync::Mutex<Status> = std::sync::Mutex::new(Status::Idle);

pub fn status() -> Status {
    STATUS.lock().map(|s| s.clone()).unwrap_or_default()
}

/// A version waiting to be installed. `Installing` is deliberately not one:
/// that install is already under way.
pub fn available_version() -> Option<String> {
    match status() {
        Status::Available(v) => Some(v),
        Status::Idle | Status::Checking | Status::Installing(_) | Status::Failed(_) => None,
    }
}

pub fn set_status(next: Status) {
    if let Ok(mut status) = STATUS.lock() {
        *status = next;
    }
}

pub fn releases_url() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

#[cfg(target_os = "macos")]
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

/// The one button an alert offers besides Done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accessibility,
    Releases,
    Retry,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Accessibility => "Open Accessibility",
            Action::Releases => "Download manually",
            Action::Retry => "Try again",
        }
    }

    /// The interface command the button sends, matching `ui::ipc::Cmd`.
    pub fn cmd(self) -> &'static str {
        match self {
            Action::Accessibility => "open_accessibility",
            Action::Releases => "open_releases",
            Action::Retry => "check_updates",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// Rendered in the amber box the panel uses for the secure-input warning.
    pub warn: Option<String>,
    pub action: Option<Action>,
}

#[allow(dead_code)]
pub fn notice_updated(from: &str, to: &str, needs_permission: bool) -> Notice {
    Notice {
        title: format!("Updated to {to}"),
        body: format!("PhaciusKey has been updated from {from} to {to} and restarted."),
        warn: needs_permission.then(|| {
            "macOS needs you to allow Accessibility once more, because this update changed \
             the app's code-signing identity. Typing stays off until then."
                .to_string()
        }),
        action: needs_permission.then_some(Action::Accessibility),
    }
}

#[allow(dead_code)]
pub fn notice_install_failed(version: &str, error: &str) -> Notice {
    Notice {
        title: "Update failed".to_string(),
        body: format!(
            "PhaciusKey could not install version {version} automatically.\n\n{error}\n\n\
             The current version keeps working."
        ),
        warn: None,
        action: Some(Action::Releases),
    }
}

#[allow(dead_code)]
pub fn notice_up_to_date() -> Notice {
    Notice {
        title: "Up to date".to_string(),
        body: format!("PhaciusKey {CURRENT} is the newest version."),
        warn: None,
        action: None,
    }
}

#[allow(dead_code)]
pub fn notice_check_failed(error: &str) -> Notice {
    Notice {
        title: "Couldn't check for updates".to_string(),
        body: format!("PhaciusKey could not reach GitHub.\n\n{error}"),
        warn: None,
        action: Some(Action::Retry),
    }
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

    #[test]
    fn a_completed_update_offers_accessibility_only_when_it_was_lost() {
        let kept = notice_updated("0.0.24", "0.0.25", false);
        assert!(kept.body.contains("0.0.24"));
        assert!(kept.body.contains("0.0.25"));
        assert_eq!(kept.action, None);
        assert_eq!(kept.warn, None);

        let lost = notice_updated("0.0.24", "0.0.25", true);
        assert_eq!(lost.action, Some(Action::Accessibility));
        assert!(lost.warn.is_some());
    }

    #[test]
    fn a_failed_install_offers_a_manual_download() {
        let notice = notice_install_failed("0.0.25", "curl exited with 22");
        assert!(notice.body.contains("0.0.25"));
        assert!(notice.body.contains("curl exited with 22"));
        assert_eq!(notice.action, Some(Action::Releases));
    }

    #[test]
    fn being_up_to_date_needs_no_action() {
        let notice = notice_up_to_date();
        assert!(notice.body.contains(CURRENT));
        assert_eq!(notice.action, None);
    }

    #[test]
    fn a_failed_check_offers_a_retry() {
        let notice = notice_check_failed("no network");
        assert!(notice.body.contains("no network"));
        assert_eq!(notice.action, Some(Action::Retry));
    }
}
