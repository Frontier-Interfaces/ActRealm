//! Bounded local-only result excerpts. Never part of SQLite, export, Cloud or spool.
use crate::sanitize::sanitize_result_text;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

const TTL: u64 = 24 * 60 * 60 * 1000;
const MAX_RESULTS: usize = 128;
const MAX_SOURCE: usize = 32 * 1024;
const MAX_PROMPT_CHARS: usize = 2_000;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultArtifact {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub can_reveal: bool,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    identity: (u64, u64),
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResult {
    pub session_id: String,
    pub observed_at: u64,
    pub source: String,
    pub summary: String,
    pub truncated: bool,
    pub artifacts: Vec<ResultArtifact>,
}

/// The user's own prompt for the current turn, sanitized and bounded. Like
/// [`SessionResult`], it lives only in Runtime memory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPrompt {
    pub text: String,
    /// The Provider prompt was longer than the bound, or lines that looked
    /// like credentials were removed.
    pub truncated: bool,
    pub observed_at: u64,
}

#[derive(Default)]
pub(crate) struct OutcomeRegistry {
    entries: HashMap<String, SessionResult>,
    started_at: HashMap<String, u64>,
    prompts: HashMap<String, SessionPrompt>,
}

impl OutcomeRegistry {
    pub fn clear(&mut self, session: &str, at: u64) {
        let start = self.started_at.entry(session.to_owned()).or_default();
        *start = (*start).max(at);
        if self
            .entries
            .get(session)
            .is_some_and(|r| r.observed_at <= at)
        {
            self.entries.remove(session);
        }
        self.prune(at);
    }

    pub fn observe(
        &mut self,
        session: &str,
        text: &str,
        cwd: Option<&Path>,
        at: u64,
        source: &str,
    ) {
        if self
            .started_at
            .get(session)
            .is_some_and(|start| at <= *start)
            || self
                .entries
                .get(session)
                .is_some_and(|r| r.observed_at >= at)
        {
            return;
        }
        self.prune(at);
        let raw: String = text.chars().take(MAX_SOURCE).collect();
        let (plain, paths) = extract_links(&raw);
        let mut fenced = false;
        let mut lines = Vec::new();
        for line in plain.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            let line = line
                .trim()
                .trim_start_matches(['#', '-', '*', '>', ' '])
                .replace("**", "")
                .replace('`', "");
            if let Ok(safe) = sanitize_result_text(&line) {
                if !safe.is_empty() {
                    lines.push(safe);
                }
            }
            if lines.len() >= 5 {
                break;
            }
        }
        let joined = lines.join("\n");
        let summary: String = joined.chars().take(600).collect();
        let artifacts = paths
            .into_iter()
            .filter_map(|p| resolve_artifact(&p, cwd))
            .take(5)
            .collect::<Vec<_>>();
        if summary.is_empty() && artifacts.is_empty() {
            return;
        }
        self.entries.insert(
            session.to_owned(),
            SessionResult {
                session_id: session.to_owned(),
                observed_at: at,
                source: source.to_owned(),
                truncated: joined.chars().count() > 600 || raw.len() < text.len(),
                summary,
                artifacts,
            },
        );
    }

    pub fn get(&mut self, session: &str, start: u64, now: u64) -> Option<SessionResult> {
        self.prune(now);
        let mut result = self
            .entries
            .get(session)
            .filter(|r| r.observed_at >= start)?
            .clone();
        for a in &mut result.artifacts {
            a.can_reveal = valid_artifact(&a.path, Some(a.identity));
        }
        Some(result)
    }

    /// Replaces the prompt of the session's previous turn. A turn whose
    /// Provider did not expose prompt text (or whose text was entirely
    /// redacted) leaves no prompt behind.
    pub fn observe_prompt(&mut self, session: &str, text: Option<&str>, at: u64) {
        self.prune(at);
        if self
            .prompts
            .get(session)
            .is_some_and(|prompt| prompt.observed_at > at)
        {
            return;
        }
        match text.and_then(sanitize_prompt) {
            Some((text, truncated)) => {
                self.prompts.insert(
                    session.to_owned(),
                    SessionPrompt {
                        text,
                        truncated,
                        observed_at: at,
                    },
                );
            }
            None => {
                self.prompts.remove(session);
            }
        }
    }

    pub fn get_prompt(&mut self, session: &str, start: u64, now: u64) -> Option<SessionPrompt> {
        self.prune(now);
        self.prompts
            .get(session)
            .filter(|prompt| prompt.observed_at >= start)
            .cloned()
    }

    pub fn reveal_path(
        &mut self,
        session: &str,
        id: &str,
        start: u64,
        now: u64,
    ) -> Option<PathBuf> {
        self.get(session, start, now)?
            .artifacts
            .into_iter()
            .find(|a| a.id == id && a.can_reveal)
            .map(|a| a.path)
    }

    fn prune(&mut self, now: u64) {
        self.entries
            .retain(|_, r| now.saturating_sub(r.observed_at) <= TTL);
        self.prompts
            .retain(|_, p| now.saturating_sub(p.observed_at) <= TTL);
        while self.prompts.len() >= MAX_RESULTS {
            let Some(key) = self
                .prompts
                .iter()
                .min_by_key(|(_, p)| p.observed_at)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            self.prompts.remove(&key);
        }
        self.started_at
            .retain(|_, at| now.saturating_sub(*at) <= TTL);
        while self.entries.len() >= MAX_RESULTS {
            let Some(key) = self
                .entries
                .iter()
                .min_by_key(|(_, r)| r.observed_at)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            self.entries.remove(&key);
        }
        if self.started_at.len() > MAX_RESULTS * 4 {
            self.started_at.clear();
        }
    }
}

/// Applies the Runtime result sanitizer line by line: lines carrying
/// credentials are dropped, paths/URLs/hosts/emails are replaced by
/// placeholders, and the result is bounded to [`MAX_PROMPT_CHARS`].
fn sanitize_prompt(text: &str) -> Option<(String, bool)> {
    let raw: String = text.chars().take(MAX_SOURCE).collect();
    let mut truncated = raw.len() < text.len();
    if raw.to_ascii_lowercase().contains("private key-----") {
        return None;
    }
    let mut lines = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match sanitize_result_text(line) {
            Ok(safe) if !safe.is_empty() => lines.push(safe),
            Ok(_) => {}
            Err(_) => truncated = true,
        }
    }
    let joined = lines.join("\n");
    if joined.chars().count() > MAX_PROMPT_CHARS {
        truncated = true;
    }
    let bounded: String = joined.chars().take(MAX_PROMPT_CHARS).collect();
    (!bounded.is_empty()).then_some((bounded, truncated))
}

fn extract_links(raw: &str) -> (String, Vec<String>) {
    let mut rest = raw;
    let mut out = String::new();
    let mut paths = Vec::new();
    while let Some(begin) = rest.find('[') {
        out.push_str(&rest[..begin]);
        let next = &rest[begin + 1..];
        let Some(middle) = next.find("](") else {
            out.push_str(&rest[begin..]);
            return (out, paths);
        };
        let after = &next[middle + 2..];
        let Some(end) = after.find(')') else {
            out.push_str(&rest[begin..]);
            return (out, paths);
        };
        let label = &next[..middle];
        let target = after[..end].trim().trim_matches(['<', '>']);
        out.push_str(label);
        if paths.len() < 12
            && target.len() < 2048
            && !target.contains("://")
            && !target.contains('?')
            && !target.contains('#')
        {
            paths.push(target.to_owned());
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    (out, paths)
}

fn resolve_artifact(raw: &str, cwd: Option<&Path>) -> Option<ResultArtifact> {
    let raw = raw
        .rsplit_once(':')
        .filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .map(|(p, _)| p)
        .unwrap_or(raw);
    let path = PathBuf::from(raw.replace("%20", " "));
    let path = if path.is_absolute() {
        path
    } else {
        cwd?.join(path)
    };
    if !valid_artifact(&path, None) {
        return None;
    }
    let meta = fs::symlink_metadata(&path).ok()?;
    let name = path.file_name()?.to_str()?.to_owned();
    let kind = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(ResultArtifact {
        id: Uuid::now_v7().to_string(),
        name,
        kind,
        can_reveal: true,
        path,
        identity: (meta.dev(), meta.ino()),
    })
}

fn valid_artifact(path: &Path, identity: Option<(u64, u64)>) -> bool {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return false;
    };
    if !path.is_absolute()
        || !(path.starts_with(&home)
            || path.starts_with("/tmp")
            || path.starts_with("/private/tmp")
            || path.starts_with(std::env::temp_dir()))
    {
        return false;
    }
    if path.components().any(|c| match c {
        Component::ParentDir => true,
        Component::Normal(v) => v
            .to_str()
            .is_none_or(|s| s.starts_with('.') || ["Library", "etc", "proc", "dev"].contains(&s)),
        _ => false,
    }) {
        return false;
    }
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ![
        "html", "htm", "md", "txt", "pdf", "png", "jpg", "jpeg", "webp", "csv", "json", "swift",
        "rs", "ts", "tsx", "js", "jsx", "css", "docx", "xlsx", "pptx", "zip",
    ]
    .contains(&extension.as_str())
    {
        return false;
    }
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.uid() != unsafe { libc::geteuid() }
    {
        return false;
    }
    // Resolve aliases before trusting a file reference. /tmp itself is an OS alias.
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    if !(canonical.starts_with(&home)
        || canonical.starts_with("/private/tmp")
        || canonical.starts_with(std::env::temp_dir().canonicalize().unwrap_or_default()))
    {
        return false;
    }
    if canonical.components().any(|c| matches!(c,Component::Normal(v) if v.to_str().is_none_or(|s|s.starts_with('.') || s == "Library"))) { return false; }
    identity.is_none_or(|i| i == (meta.dev(), meta.ino()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excerpts_keep_provider_results_but_drop_secrets_and_older_turns() {
        let mut r = OutcomeRegistry::default();
        r.clear("s", 100);
        r.observe(
            "s",
            "计算结果：15,129\npassword=secret-value\n已复核",
            None,
            200,
            "hook:Stop",
        );
        let result = r.get("s", 100, 220).unwrap();
        assert!(result.summary.contains("15,129"));
        assert!(!result.summary.contains("secret-value"));
        r.clear("s", 300);
        r.observe("s", "Old result", None, 250, "hook:Stop");
        assert!(r.get("s", 300, 400).is_none());
    }
    #[test]
    fn prompts_are_sanitized_bounded_and_never_regress_to_an_older_turn() {
        let mut r = OutcomeRegistry::default();
        r.observe_prompt("s", Some("第一行\n\n  第二行  \ntoken=abc123"), 100);
        let prompt = r.get_prompt("s", 100, 110).unwrap();
        assert_eq!(prompt.text, "第一行\n第二行");
        assert!(prompt.truncated);
        r.observe_prompt("s", Some("较早的一轮"), 50);
        assert_eq!(r.get_prompt("s", 0, 110).unwrap().text, "第一行\n第二行");
        r.observe_prompt(
            "s",
            Some("-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----"),
            200,
        );
        assert!(r.get_prompt("s", 0, 210).is_none());
        r.observe_prompt("s", Some("短句"), 300);
        let prompt = r.get_prompt("s", 300, 310).unwrap();
        assert_eq!(prompt.text, "短句");
        assert!(!prompt.truncated);
        assert!(r.get_prompt("s", 0, 300 + TTL + 1).is_none());
    }

    #[test]
    fn files_require_existing_local_targets_and_revalidate_identity() {
        let root = std::env::temp_dir().join(format!("actrealm-result-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("report.html");
        fs::write(&path, "report").unwrap();
        let mut r = OutcomeRegistry::default();
        r.observe(
            "s",
            &format!(
                "完成 [预览]({}) [secret](/etc/passwd) [missing](missing.txt)",
                path.display()
            ),
            Some(&root),
            200,
            "hook:Stop",
        );
        let result = r.get("s", 0, 220).unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].name, "report.html");
        let wire = serde_json::to_string(&result).unwrap();
        assert!(!wire.contains(&root.to_string_lossy().to_string()));
        fs::remove_file(&path).unwrap();
        assert!(!r.get("s", 0, 230).unwrap().artifacts[0].can_reveal);
        fs::remove_dir_all(root).unwrap();
    }
}
