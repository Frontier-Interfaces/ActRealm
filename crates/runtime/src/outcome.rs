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
///
/// On top of the known-format detection of the result sanitizer, a line that
/// names a credential ([`PROMPT_SECRET_LABELS`], English or Chinese, followed
/// by a half- or full-width colon or equals sign, whitespace, or the end of
/// the line) is removed as a whole. When such a label carries no value on its
/// own line (a Chinese password label with a full-width colon, `token:`, or
/// just `Password`), the next non-empty line is treated as the value and
/// removed too. Any removal marks the prompt `truncated`.
fn sanitize_prompt(text: &str) -> Option<(String, bool)> {
    let raw: String = text.chars().take(MAX_SOURCE).collect();
    let mut truncated = raw.len() < text.len();
    if raw.to_ascii_lowercase().contains("private key-----") {
        return None;
    }
    let mut lines = Vec::new();
    let mut value_on_next_line = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let label = scan_secret_label(line);
        if value_on_next_line || label != SecretLabel::None {
            // Either this line holds the value of a bare label on the
            // previous line, or it names a credential itself. A dropped value
            // line that is itself a bare label keeps the next line pending.
            truncated = true;
            value_on_next_line = label == SecretLabel::ValueOnNextLine;
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
    // Run the known-format detection once more over the whole bounded text so
    // nothing the per-line pass let through can leave Runtime.
    if sanitize_result_text(&bounded).is_err() {
        return None;
    }
    (!bounded.is_empty()).then_some((bounded, truncated))
}

/// Credential labels a user may type in front of a secret. Matching is
/// case-insensitive, `_` and `-` match a space (`API_KEY`, `access-key`), and
/// full-width letters and punctuation match their ASCII forms. Chinese labels
/// are written as escapes because Runtime production source stays free of Han
/// text; the comment gives each meaning.
const PROMPT_SECRET_LABELS: [&str; 25] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "api key",
    "apikey",
    "access key",
    "private key",
    "cookie",
    "authorization",
    "bearer",
    "\u{5bc6}\u{7801}",         // password
    "\u{5bc6}\u{78bc}",         // password, traditional
    "\u{53e3}\u{4ee4}",         // passphrase
    "\u{5bc6}\u{94a5}",         // secret key
    "\u{5bc6}\u{9470}",         // secret key, traditional
    "\u{79d8}\u{94a5}",         // secret key, variant
    "\u{4ee4}\u{724c}",         // token
    "\u{51ed}\u{8bc1}",         // credential
    "\u{6191}\u{8b49}",         // credential, traditional
    "\u{51ed}\u{636e}",         // credential, variant
    "\u{6388}\u{6743}\u{7801}", // authorization code
    "\u{6388}\u{6b0a}\u{78bc}", // authorization code, traditional
    "\u{9a8c}\u{8bc1}\u{7801}", // verification code
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretLabel {
    /// The line names no credential.
    None,
    /// The line names a credential and may carry its value.
    OnLine,
    /// The line is a bare label: only the label, or the label followed by
    /// nothing but a colon, equals sign or copula. The value is expected on
    /// the next line.
    ValueOnNextLine,
}

/// Separators that turn a credential word into a label: a colon or equals
/// sign (after full-width folding), any whitespace, and, after a Chinese
/// label, the copulas "is" (U+662F) and "as" (U+4E3A, traditional U+70BA),
/// as in "the password is ...".
fn is_label_separator(character: char, label_is_ascii: bool) -> bool {
    matches!(character, ':' | '=')
        || character.is_whitespace()
        || (!label_is_ascii && matches!(character, '\u{662f}' | '\u{4e3a}' | '\u{70ba}'))
}

fn scan_secret_label(line: &str) -> SecretLabel {
    // Fold full-width ASCII forms (`：`, `＝`, `ＡＰＩ`) and the ideographic
    // space to ASCII, lowercase, and let `_` / `-` match a space.
    let normalized = line
        .chars()
        .map(|character| match character {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => {
                char::from_u32(u32::from(character) - 0xFEE0).unwrap_or(character)
            }
            other => other,
        })
        .flat_map(char::to_lowercase)
        .map(|character| match character {
            '_' | '-' => ' ',
            other => other,
        })
        .collect::<Vec<_>>();
    let mut result = SecretLabel::None;
    for label in PROMPT_SECRET_LABELS {
        let label = label.chars().collect::<Vec<_>>();
        let label_is_ascii = label.iter().all(char::is_ascii);
        for start in 0..normalized.len() {
            if !normalized[start..].starts_with(&label) {
                continue;
            }
            let mut end = start + label.len();
            // English plurals (`passwords:`, `tokens =`) are labels too.
            if label_is_ascii && normalized.get(end) == Some(&'s') {
                end += 1;
            }
            let rest = &normalized[end..];
            if !rest
                .first()
                .is_none_or(|character| is_label_separator(*character, label_is_ascii))
            {
                continue;
            }
            let value_missing = rest
                .iter()
                .all(|character| is_label_separator(*character, label_is_ascii));
            let bare_line = normalized[..start]
                .iter()
                .all(|character| character.is_whitespace());
            let assigns = rest.iter().any(|character| !character.is_whitespace());
            if value_missing && (bare_line || assigns) {
                return SecretLabel::ValueOnNextLine;
            }
            result = SecretLabel::OnLine;
        }
    }
    result
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
    fn prompts_drop_whole_lines_that_name_a_credential_in_any_language() {
        for secret_line in [
            "密码：Abc123!",
            "密码:Abc123!",
            "密码 = Abc123!",
            "令牌：tok-live-1234",
            "API key：sk-live-abcdefghijklmnop",
            "API_KEY：abc",
            "口令 opensesame",
            "我的密码是Abc123!",
            "令牌为 abc",
            "ＴＯＫＥＮ＝abc",
            "授权码 884213",
            "验证码：884213",
            "凭证：x",
            "秘钥：x",
            "密钥：x",
            "Passwords: hunter2",
            "access-key：AK123",
            "private key：x",
            "Cookie：sid=1",
            "authorization：x",
            "Bearer abc",
            "pwd：x",
            "passwd x",
            "apikey：x",
            "secret：x",
            "密碼：Abc123!",
            "密鑰：x",
            "憑證：x",
            "凭据：x",
            "授權碼 884213",
            "令牌為 abc",
        ] {
            let (text, truncated) =
                sanitize_prompt(&format!("修改首页\n{secret_line}\n然后跑测试")).unwrap();
            assert_eq!(text, "修改首页\n然后跑测试", "{secret_line}");
            assert!(truncated, "{secret_line}");
        }
        // A line that only ends with a label is removed, but the next line is
        // not assumed to hold its value.
        assert_eq!(
            sanitize_prompt("请重置密码\n然后跑测试").unwrap(),
            ("然后跑测试".to_owned(), true)
        );
        // Words that merely contain a label are not credentials.
        assert_eq!(
            sanitize_prompt("修改密码重置页面的按钮\n把令牌桶限流改成 10").unwrap(),
            (
                "修改密码重置页面的按钮\n把令牌桶限流改成 10".to_owned(),
                false
            )
        );
        assert!(sanitize_prompt("密码：Abc123!").is_none());
    }

    #[test]
    fn bare_credential_labels_also_drop_the_value_on_the_next_line() {
        for label in [
            "密码:",
            "密码：",
            "token：",
            "Token =",
            "密码",
            "Password",
            "- 口令：",
            "请输入密码：",
            "我的令牌是",
            "API_KEY ＝",
        ] {
            let (text, truncated) =
                sanitize_prompt(&format!("先登录\n{label}\n\n  Abc123!\n再部署")).unwrap();
            assert_eq!(text, "先登录\n再部署", "{label}");
            assert!(truncated, "{label}");
        }
        // A bare label whose "value" line is another bare label keeps the
        // following line pending as well.
        assert_eq!(
            sanitize_prompt("密码：\n令牌：\nAbc123!\n继续").unwrap(),
            ("继续".to_owned(), true)
        );
        assert!(sanitize_prompt("token:\nabc").is_none());
    }

    #[test]
    fn prompt_bounds_cut_on_character_boundaries() {
        let long = "汉".repeat(MAX_PROMPT_CHARS + 50);
        let (text, truncated) = sanitize_prompt(&long).unwrap();
        assert_eq!(text.chars().count(), MAX_PROMPT_CHARS);
        assert!(text.chars().all(|character| character == '汉'));
        assert!(truncated);

        let emoji = "😀".repeat(MAX_SOURCE + 1);
        let (text, truncated) = sanitize_prompt(&emoji).unwrap();
        assert_eq!(text.chars().count(), MAX_PROMPT_CHARS);
        assert!(truncated);

        // The source bound cuts the credential line to `密码：Ab`; the cut
        // line is still recognized and removed.
        let source = format!("{}\n密码：Abc123!", "字".repeat(MAX_SOURCE - 6));
        let (text, truncated) = sanitize_prompt(&source).unwrap();
        assert!(truncated);
        assert!(!text.contains("Ab"));
        assert!(!text.contains('密'));
        assert_eq!(text.chars().count(), MAX_PROMPT_CHARS);

        // A credential line inside the bound is removed before bounding.
        let near_bound = format!(
            "{}\n密码：Abc123!\n{}",
            "前".repeat(MAX_PROMPT_CHARS - 10),
            "后".repeat(20)
        );
        let (text, truncated) = sanitize_prompt(&near_bound).unwrap();
        assert!(truncated);
        assert!(!text.contains("Abc"));
        assert_eq!(text.chars().count(), MAX_PROMPT_CHARS);
        assert!(text.ends_with(&"后".repeat(9)));
    }

    #[test]
    fn prompts_still_drop_known_secret_formats_without_labels() {
        for line in [
            "用这个 sk-live-abcdefghijklmnopqrstuv",
            "ghp_abcdefghijklmnopqrstuvwxyz0123",
            "export GITHUB_TOKEN=abc",
            "xoxb-1234-abcd",
        ] {
            let (text, truncated) = sanitize_prompt(&format!("开始\n{line}\n结束")).unwrap();
            assert_eq!(text, "开始\n结束", "{line}");
            assert!(truncated, "{line}");
        }
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
