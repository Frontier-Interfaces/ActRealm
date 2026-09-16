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
    if has_secret_prefix(&lower) || looks_high_entropy(trimmed) {
        return Err(SanitizationError::SensitiveContentDetected);
    }
    if let Some((key, _)) = trimmed.split_once('=') {
        if looks_environment_key(key) || is_sensitive_key(key) {
            return Err(SanitizationError::SensitiveContentDetected);
        }
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
        "ya29.",
        "eyjhb",
        "akia",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix) || value.contains(prefix))
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
