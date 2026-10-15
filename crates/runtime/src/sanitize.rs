use std::net::{IpAddr, SocketAddr};

#[derive(Debug)]
pub(crate) enum SanitizationError {
    SensitiveContentDetected,
}

pub(crate) fn sanitize_result_text(value: &str) -> Result<String, SanitizationError> {
    let lower = value.to_ascii_lowercase();
    if [
        "-----begin private key-----",
        "-----begin rsa private key-----",
        "-----begin openssh private key-----",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return Err(SanitizationError::SensitiveContentDetected);
    }

    let mut lines = Vec::new();
    for raw_line in value.lines() {
        let normalized = raw_line
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect::<String>();
        if contains_high_confidence_secret(&normalized) {
            return Err(SanitizationError::SensitiveContentDetected);
        }

        let mut tokens = Vec::new();
        for token in normalized.split_whitespace() {
            tokens.push(sanitize_token(token)?);
        }
        let line = tokens.join(" ");
        if !line.is_empty() {
            lines.push(line);
        }
    }
    // Preserve word boundaries without control characters.
    Ok(lines.join(" "))
}

fn sanitize_token(token: &str) -> Result<String, SanitizationError> {
    let trimmed = token.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    let lower = trimmed.to_ascii_lowercase();
    if has_secret_prefix(&lower) || has_strict_secret_prefix(trimmed) || looks_high_entropy(trimmed)
    {
        return Err(SanitizationError::SensitiveContentDetected);
    }
    if let Some((key, _)) = trimmed.split_once('=') {
        if looks_environment_key(key) || is_sensitive_key(key) {
            return Err(SanitizationError::SensitiveContentDetected);
        }
    }
    // A secret written straight against CJK text or full-width punctuation
    // (common with a Chinese input method) shares its whitespace token with
    // them; check every ASCII fragment on its own as well.
    if ascii_fragments(trimmed).any(|fragment| {
        let fragment = trim_fragment(fragment);
        looks_high_entropy(fragment)
            || fragment
                .split_once('=')
                .is_some_and(|(key, _)| looks_environment_key(key) || is_sensitive_key(key))
    }) {
        return Err(SanitizationError::SensitiveContentDetected);
    }
    // A key glued to ASCII punctuation in compact JSON or code
    // (`{"appKey":"..."}`, `Client("...")`, `key:...`), or a Telegram or
    // Discord bot token: every run of key characters is checked on its own.
    // A run needs a digit here, so long identifiers between dots and quotes
    // (`store.handleCompanionSnapshotRequest(`) are kept. URLs and paths are
    // replaced by a placeholder as a whole below.
    if !lower.contains("://")
        && !looks_path(trimmed)
        && (key_runs(trimmed)
            .any(|run| looks_high_entropy(run) && run.bytes().any(|byte| byte.is_ascii_digit()))
            || looks_bot_token(trimmed))
    {
        return Err(SanitizationError::SensitiveContentDetected);
    }
    // A connection string or URL with a password in it (`user:password@`
    // before the host).
    if url_has_password(&lower) {
        return Err(SanitizationError::SensitiveContentDetected);
    }
    if lower.contains("://")
        || lower.starts_with("mailto:")
        || lower.starts_with("git@")
        || lower.starts_with("ssh:")
    {
        return Ok("<url>".to_owned());
    }
    if looks_path(trimmed) {
        return Ok("<path>".to_owned());
    }
    if looks_email(trimmed) {
        return Ok("<email>".to_owned());
    }
    if trimmed.parse::<IpAddr>().is_ok() {
        return Ok("<host>".to_owned());
    }
    if trimmed.parse::<SocketAddr>().is_ok() || looks_host_port(trimmed) || looks_hostname(trimmed)
    {
        return Ok("<host>".to_owned());
    }
    Ok(token.to_owned())
}

fn contains_high_confidence_secret(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if [
        "authorization:",
        "authorization ",
        "proxy-authorization:",
        "cookie:",
        "set-cookie:",
        "client secret",
        "private key",
        "access token",
        "refresh token",
        "id token",
        "api key",
        "passcode",
        "one-time password",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return true;
    }
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    if tokens.windows(2).any(|pair| {
        matches!(
            pair[0]
                .trim_matches(|character: char| !character.is_ascii_alphanumeric())
                .to_ascii_lowercase()
                .as_str(),
            "basic" | "bearer" | "digest"
        )
    }) {
        return true;
    }
    tokens.iter().any(|token| {
        let token = token.trim_matches(|character: char| {
            matches!(
                character,
                '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
            )
        });
        if !token.is_ascii()
            && ascii_fragments(token).any(|fragment| {
                let fragment = trim_fragment(fragment);
                if let Some((key, _)) = fragment.split_once('=') {
                    return looks_environment_key(key) || is_sensitive_key(key);
                }
                fragment
                    .split_once(':')
                    .is_some_and(|(key, _)| is_sensitive_key(key))
            })
        {
            return true;
        }
        if let Some((key, _)) = token.split_once('=') {
            return looks_environment_key(key) || is_sensitive_key(key);
        }
        if let Some((key, _)) = token.split_once(':') {
            return is_sensitive_key(key);
        }
        let normalized = token.trim_start_matches('-');
        (is_sensitive_key(normalized)
            || (normalized.contains('_') && looks_environment_key(normalized)))
            && lower.contains(&format!("{} ", token.to_ascii_lowercase()))
    })
}

fn has_secret_prefix(value: &str) -> bool {
    [
        "sk-",
        "ghp_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxs-",
        "ya29.",
        "eyjhb",
        "akia",
    ]
    .iter()
    .any(|prefix| {
        // A prefix shorter than five characters also occurs inside ordinary
        // words (`task-runner`, `flask-login`, `Slovakia`): it counts only at
        // the start of a run of key characters followed by at least 16 more.
        if prefix.len() < 5 {
            starts_a_key_run(value, prefix, 16)
        } else {
            value.contains(prefix)
        }
    })
}

/// `prefix` starts a run of `[a-z0-9_-]` in the lowercase `value` and is
/// followed by at least `tail` more key characters.
fn starts_a_key_run(value: &str, prefix: &str, tail: usize) -> bool {
    value
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        })
        .any(|run| {
            run.strip_prefix(prefix)
                .is_some_and(|rest| rest.len() >= tail)
        })
}

/// Provider key formats whose prefix alone is too short or too common to
/// match anywhere in a word (`hf_hub_download`, `npm_config_registry`). Each
/// counts only at the start of a run of `[A-Za-z0-9_-]`, case-sensitively,
/// followed by at least the given number of key characters.
const STRICT_SECRET_PREFIXES: [(&str, usize); 14] = [
    ("AIza", 30),     // Google API key
    ("glpat-", 16),   // GitLab personal access token
    ("sk_live_", 16), // Stripe secret key
    ("rk_live_", 16), // Stripe restricted key
    ("sk_test_", 16), // Stripe test secret key
    ("rk_test_", 16), // Stripe test restricted key
    ("hf_", 30),      // Hugging Face token
    ("npm_", 30),     // npm access token
    ("xapp-", 16),    // Slack app-level token
    ("gho_", 30),     // GitHub OAuth token
    ("ghs_", 30),     // GitHub server-to-server token
    ("ghu_", 30),     // GitHub user-to-server token
    ("ghr_", 30),     // GitHub refresh token
    ("LTAI", 12),     // Alibaba Cloud AccessKey ID
];

fn has_strict_secret_prefix(value: &str) -> bool {
    value
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        })
        .any(|run| {
            STRICT_SECRET_PREFIXES.iter().any(|(prefix, tail)| {
                run.strip_prefix(prefix)
                    .is_some_and(|rest| rest.len() >= *tail)
            })
        })
}

/// The ASCII runs of a token, split at every non-ASCII character: CJK text,
/// CJK punctuation (U+3000-303F), full-width forms (U+FF00-FFEF) and Chinese
/// quotation marks.
fn ascii_fragments(token: &str) -> impl Iterator<Item = &str> {
    token
        .split(|character: char| !character.is_ascii())
        .filter(|fragment| !fragment.is_empty())
}

/// The runs of key characters (`[A-Za-z0-9_+/-]`) of a token, split at
/// quotes, colons, commas, brackets, braces, `=`, `;`, `.`, `@` and every
/// other character.
fn key_runs(token: &str) -> impl Iterator<Item = &str> {
    token
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '+' | '/'))
        })
        .filter(|run| !run.is_empty())
}

/// A Telegram bot token (`<bot id>:<secret>`: six to twelve digits, a colon
/// and at least 30 key characters) or a Discord bot token (three
/// dot-separated base64url parts of at least 23 characters with a digit, six
/// or seven, and at least 27 characters).
fn looks_bot_token(token: &str) -> bool {
    let bytes = token.as_bytes();
    let telegram = bytes.iter().enumerate().any(|(colon, byte)| {
        if *byte != b':' {
            return false;
        }
        let digits = bytes[..colon]
            .iter()
            .rev()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        let starts_word = bytes[..colon - digits]
            .last()
            .is_none_or(|byte| !byte.is_ascii_alphanumeric());
        let secret = bytes[colon + 1..]
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            .count();
        (6..=12).contains(&digits) && starts_word && secret >= 30
    });
    let discord = token
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
        })
        .any(|run| {
            let parts = run.split('.').collect::<Vec<_>>();
            parts.len() == 3
                && parts[0].len() >= 23
                && parts[0].bytes().any(|byte| byte.is_ascii_digit())
                && (6..=7).contains(&parts[1].len())
                && parts[2].len() >= 27
        });
    telegram || discord
}

/// A URL whose authority carries `user:password@`.
fn url_has_password(lower: &str) -> bool {
    let Some((_, after_scheme)) = lower.split_once("://") else {
        return false;
    };
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    authority
        .rsplit_once('@')
        .and_then(|(user_info, _)| user_info.split_once(':'))
        .is_some_and(|(_, password)| !password.is_empty())
}

/// Strips punctuation that surrounds a value in prose or Markdown (`(key)`,
/// `key.`, `**key**`) but never belongs to a key format.
fn trim_fragment(fragment: &str) -> &str {
    fragment.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\''
                | '`'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
                | ','
                | ';'
                | ':'
                | '.'
                | '!'
                | '?'
                | '*'
        )
    })
}

fn looks_high_entropy(value: &str) -> bool {
    if value.len() < 24
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'+' | b'/' | b'=')
        })
    {
        return false;
    }
    let has_lower = value.bytes().any(|byte| byte.is_ascii_lowercase());
    let has_upper = value.bytes().any(|byte| byte.is_ascii_uppercase());
    let has_digit = value.bytes().any(|byte| byte.is_ascii_digit());
    let has_symbol = value
        .bytes()
        .any(|byte| matches!(byte, b'-' | b'_' | b'+' | b'/' | b'='));
    let classes = [has_lower, has_upper, has_digit, has_symbol]
        .into_iter()
        .filter(|present| *present)
        .count();
    let unique = value
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    classes >= 3 || (value.len() >= 32 && classes >= 2) || (value.len() >= 48 && unique >= 12)
}

fn looks_environment_key(value: &str) -> bool {
    value.len() >= 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn is_sensitive_key(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "api_key",
        "apikey",
        "authorization",
        "credential",
        "cookie",
        "_auth",
        "private_key",
        "access_key",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn looks_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("~/")
        || value.starts_with("./")
        || value.starts_with("../")
        || value.contains('\\')
        || (value.contains('/') && !value.contains("://"))
}

fn looks_email(value: &str) -> bool {
    let value = value.trim_matches(|character: char| {
        !character.is_ascii_alphanumeric() && !matches!(character, '@' | '.' | '_' | '+' | '-')
    });
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.ends_with('.')
}

fn looks_hostname(value: &str) -> bool {
    let value = value.trim_matches(|character: char| {
        !character.is_ascii_alphanumeric() && !matches!(character, '.' | '-')
    });
    if value.len() < 4 || value.contains("..") || value.parse::<f64>().is_ok() {
        return false;
    }
    let Some((head, tail)) = value.rsplit_once('.') else {
        return false;
    };
    !head.is_empty()
        && (2..=24).contains(&tail.len())
        && tail.bytes().all(|byte| byte.is_ascii_alphabetic())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

fn looks_host_port(value: &str) -> bool {
    let value = value.trim_matches(|character: char| {
        !character.is_ascii_alphanumeric() && !matches!(character, '.' | '-' | ':' | '[' | ']')
    });
    let Some((host, port)) = value.rsplit_once(':') else {
        return false;
    };
    if port.parse::<u16>().is_err() {
        return false;
    }
    let host = host.trim_matches(|character| matches!(character, '[' | ']'));
    host.parse::<IpAddr>().is_ok() || looks_hostname(host)
}
