use actrealm_core::Provider;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

const MAX_PROVIDER_TITLE_CHARS: usize = 120;
const MAX_CODEX_INDEX_BYTES: u64 = 2 * 1024 * 1024;
const CLAUDE_TRANSCRIPT_WINDOW_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderTitle {
    pub title: String,
    pub source: &'static str,
}

pub(crate) fn resolve_event_title(
    provider: Provider,
    raw: &Value,
    provider_session_id: &str,
    cwd: Option<&str>,
) -> Option<ProviderTitle> {
    if let Some(title) = explicit_provider_title(provider, raw) {
        return Some(title);
    }

    match provider {
        Provider::Claude => {
            let transcript = trusted_claude_transcript_path(raw, provider_session_id)
                .or_else(|| claude_transcript_path(cwd?, provider_session_id));
            transcript.and_then(|path| claude_title_from_transcript(&path))
        }
        Provider::Codex => codex_title_from_index(&codex_index_path(), provider_session_id),
        Provider::Gemini => None,
    }
}

pub(crate) fn resolve_session_title(
    provider: &str,
    provider_session_id: &str,
    cwd: Option<&str>,
) -> Option<ProviderTitle> {
    match provider {
        "claude" => claude_transcript_path(cwd?, provider_session_id)
            .and_then(|path| claude_title_from_transcript(&path)),
        "codex" => codex_title_from_index(&codex_index_path(), provider_session_id),
        _ => None,
    }
}

pub(crate) fn resolve_codex_session_titles(
    provider_session_ids: &HashSet<String>,
) -> HashMap<String, ProviderTitle> {
    codex_titles_from_index(&codex_index_path(), provider_session_ids)
}

fn explicit_provider_title(provider: Provider, raw: &Value) -> Option<ProviderTitle> {
    let title = raw
        .get("session_title")
        .and_then(Value::as_str)
        .and_then(normalize_title)
        .or_else(|| {
            if provider != Provider::Codex {
                return None;
            }
            raw.pointer("/thread/name")
                .or_else(|| raw.pointer("/params/thread/name"))
                .or_else(|| raw.get("thread_name"))
                .and_then(Value::as_str)
                .and_then(normalize_title)
        })?;
    let source = match provider {
        Provider::Claude => "claude_session_title",
        Provider::Codex => "codex_thread_name",
        Provider::Gemini => return None,
    };
    Some(ProviderTitle { title, source })
}

fn codex_index_path() -> PathBuf {
    env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home_directory().map(|home| home.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
        .join("session_index.jsonl")
}

fn claude_config_root() -> Option<PathBuf> {
    env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home_directory().map(|home| home.join(".claude")))
}

fn home_directory() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn claude_transcript_path(cwd: &str, provider_session_id: &str) -> Option<PathBuf> {
    if !safe_path_component(provider_session_id) || cwd.trim().is_empty() {
        return None;
    }
    let encoded = cwd
        .chars()
        .map(|character| {
            if character == '/' || character == ' ' || !character.is_ascii() {
                '-'
            } else {
                character
            }
        })
        .collect::<String>();
    Some(
        claude_config_root()?
            .join("projects")
            .join(encoded)
            .join(format!("{provider_session_id}.jsonl")),
    )
}

fn trusted_claude_transcript_path(raw: &Value, provider_session_id: &str) -> Option<PathBuf> {
    if !safe_path_component(provider_session_id) {
        return None;
    }
    let path = PathBuf::from(raw.get("transcript_path")?.as_str()?);
    if !path.is_absolute()
        || path.extension().and_then(|value| value.to_str()) != Some("jsonl")
        || path.file_stem().and_then(|value| value.to_str()) != Some(provider_session_id)
    {
        return None;
    }
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    components
        .windows(2)
        .any(|pair| pair == [".claude", "projects"])
        .then_some(path)
}

fn safe_path_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn codex_title_from_index(path: &Path, provider_session_id: &str) -> Option<ProviderTitle> {
    let ids = HashSet::from([provider_session_id.to_owned()]);
    codex_titles_from_index(path, &ids).remove(provider_session_id)
}

fn codex_titles_from_index(
    path: &Path,
    provider_session_ids: &HashSet<String>,
) -> HashMap<String, ProviderTitle> {
    if provider_session_ids.is_empty() {
        return HashMap::new();
    }
    let Some(contents) = read_tail_text(path, MAX_CODEX_INDEX_BYTES) else {
        return HashMap::new();
    };
    let mut latest = HashMap::<String, (Option<String>, usize, String)>::new();
    for (sequence, line) in contents.lines().enumerate() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !provider_session_ids.contains(id) {
            continue;
        }
        let Some(title) = value
            .get("thread_name")
            .and_then(Value::as_str)
            .and_then(normalize_title)
        else {
            continue;
        };
        let updated_at = value
            .get("updated_at")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let replace = latest
            .get(id)
            .is_none_or(|(current_updated, current_sequence, _)| {
                match (&updated_at, current_updated) {
                    (Some(candidate), Some(current)) => {
                        candidate > current
                            || (candidate == current && sequence >= *current_sequence)
                    }
                    _ => sequence >= *current_sequence,
                }
            });
        if replace {
            latest.insert(id.to_owned(), (updated_at, sequence, title));
        }
    }
    latest
        .into_iter()
        .map(|(id, (_, _, title))| {
            (
                id,
                ProviderTitle {
                    title,
                    source: "codex_thread_name",
                },
            )
        })
        .collect()
}

fn claude_title_from_transcript(path: &Path) -> Option<ProviderTitle> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let head_length = length.min(CLAUDE_TRANSCRIPT_WINDOW_BYTES);
    let mut head = vec![0; head_length as usize];
    file.read_exact(&mut head).ok()?;

    let tail = if length > head_length {
        let tail_length = length.min(CLAUDE_TRANSCRIPT_WINDOW_BYTES);
        file.seek(SeekFrom::Start(length.saturating_sub(tail_length)))
            .ok()?;
        let mut value = vec![0; tail_length as usize];
        file.read_exact(&mut value).ok()?;
        value
    } else {
        Vec::new()
    };

    let mut custom = None;
    let mut ai = None;
    collect_claude_titles(&head, &mut custom, &mut ai);
    collect_claude_titles(&tail, &mut custom, &mut ai);
    custom
        .map(|title| ProviderTitle {
            title,
            source: "claude_custom_title",
        })
        .or_else(|| {
            ai.map(|title| ProviderTitle {
                title,
                source: "claude_ai_title",
            })
        })
}

fn collect_claude_titles(bytes: &[u8], custom: &mut Option<String>, ai: &mut Option<String>) {
    let contents = String::from_utf8_lossy(bytes);
    for line in contents.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("custom-title") => {
                if let Some(title) = value
                    .get("customTitle")
                    .and_then(Value::as_str)
                    .and_then(normalize_title)
                {
                    *custom = Some(title);
                }
            }
            Some("ai-title") => {
                if let Some(title) = value
                    .get("aiTitle")
                    .and_then(Value::as_str)
                    .and_then(normalize_title)
                {
                    *ai = Some(title);
                }
            }
            _ => {}
        }
    }
}

fn read_tail_text(path: &Path, max_bytes: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((length - start).min(max_bytes) as usize);
    file.take(max_bytes).read_to_end(&mut bytes).ok()?;
    let mut contents = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 {
        let newline = contents.find('\n')?;
        contents.drain(..=newline);
    }
    Some(contents)
}

fn normalize_title(value: &str) -> Option<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }
    let mut title = normalized
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_PROVIDER_TITLE_CHARS)
        .collect::<String>();
    if normalized.chars().count() > MAX_PROVIDER_TITLE_CHARS {
        title.push('…');
    }
    (!title.is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn temp_file(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("actrealm-title-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        root.join(name)
    }

    #[test]
    fn codex_index_prefers_latest_matching_thread_name() {
        let path = temp_file("session_index.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"id\":\"wanted\",\"thread_name\":\"旧标题\",\"updated_at\":\"2026-01-01T00:00:00Z\"}\n",
                "not-json\n",
                "{\"id\":\"other\",\"thread_name\":\"别的任务\"}\n",
                "{\"id\":\"wanted\",\"thread_name\":\"客户端新标题\",\"updated_at\":\"2026-01-02T00:00:00Z\"}\n"
            ),
        )
        .unwrap();

        let resolved = codex_title_from_index(&path, "wanted").unwrap();
        assert_eq!(resolved.title, "客户端新标题");
        assert_eq!(resolved.source, "codex_thread_name");
    }

    #[test]
    fn claude_custom_title_wins_over_ai_title_without_persisting_transcript() {
        let path = temp_file("claude.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"user\",\"message\":{\"content\":\"private prompt\"}}\n",
                "{\"type\":\"ai-title\",\"aiTitle\":\"AI 生成标题\"}\n",
                "{\"type\":\"custom-title\",\"customTitle\":\"用户最终标题\"}\n"
            ),
        )
        .unwrap();

        let resolved = claude_title_from_transcript(&path).unwrap();
        assert_eq!(resolved.title, "用户最终标题");
        assert_eq!(resolved.source, "claude_custom_title");
        assert!(!resolved.title.contains("private prompt"));
    }

    #[test]
    fn official_claude_session_title_is_bounded_and_preferred() {
        let raw = serde_json::json!({
            "session_title": format!("{}  tail", "x".repeat(140)),
            "transcript_path": "/tmp/not-trusted.jsonl"
        });
        let resolved = resolve_event_title(
            Provider::Claude,
            &raw,
            "019d6331-3593-7b53-9513-c1dd25d708b0",
            Some("/tmp/project"),
        )
        .unwrap();
        assert_eq!(resolved.source, "claude_session_title");
        assert!(resolved.title.ends_with('…'));
        assert!(resolved.title.chars().count() <= MAX_PROVIDER_TITLE_CHARS + 1);
    }
}
