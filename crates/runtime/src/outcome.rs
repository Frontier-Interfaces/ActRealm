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
        let mut filter = CredentialLineFilter::default();
        for line in plain.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            let indent = leading_whitespace(line);
            let line = strip_markdown(line.trim());
            if line.trim().is_empty() {
                filter.blank_line();
                continue;
            }
            if let Some(safe) = filter.filter(indent, &line) {
                lines.push(safe);
            }
            if lines.len() >= 5 {
                break;
            }
        }
        let joined = lines.join("\n");
        let mut summary: String = joined.chars().take(600).collect();
        let mut truncated =
            filter.removed || joined.chars().count() > 600 || raw.len() < text.len();
        // The known-format detection runs once more over the whole excerpt.
        if sanitize_result_text(&summary).is_err() {
            summary.clear();
            truncated = true;
        }
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
                truncated,
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

/// Applies the Runtime result sanitizer line by line through
/// [`CredentialLineFilter`]: credential lines are dropped, paths, URLs, hosts
/// and emails are replaced by placeholders, and the result is bounded to
/// [`MAX_PROMPT_CHARS`]. Any removal marks the prompt `truncated`; a known
/// secret format found in the bounded text as a whole drops the prompt.
fn sanitize_prompt(text: &str) -> Option<(String, bool)> {
    let raw: String = text.chars().take(MAX_SOURCE).collect();
    let mut truncated = raw.len() < text.len();
    if raw.to_ascii_lowercase().contains("private key-----") {
        return None;
    }
    let mut lines = Vec::new();
    let mut filter = CredentialLineFilter::default();
    for line in raw.lines() {
        let indent = leading_whitespace(line);
        let line = line.trim();
        if line.is_empty() {
            filter.blank_line();
            continue;
        }
        if let Some(safe) = filter.filter(indent, line) {
            lines.push(safe);
        }
    }
    truncated |= filter.removed;
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

/// The line filter shared by the turn prompt and result excerpts. On top of
/// the known-format detection of the result sanitizer, a line that names a
/// credential ([`SECRET_LABELS`], a short `pass` / `pwd` / `pin` key in an
/// assignment, a bare `key` / `SK` label with a key-like value, or a
/// command-line credential such as `mysql -p<password>`) is removed as a
/// whole. When such a label carries no value on its own line (a Chinese
/// password label with a full-width colon, `token:`, `password: |`, or just
/// `Password`), the next non-empty line is treated as its value and removed
/// too, together with any following lines indented deeper than the label (a
/// YAML block scalar or a bracketed list). A Markdown table whose header
/// names a credential column loses its separator and data rows as well.
#[derive(Default)]
struct CredentialLineFilter {
    value_on_next_line: bool,
    /// Indentation of the last bare label line: deeper-indented lines after
    /// its value continue that value.
    block_indent: Option<usize>,
    /// The previous line was a table header with credential columns: if this
    /// line is the table separator, the table's rows are filtered.
    table_header: Option<SecretColumns>,
    /// Inside the rows of a table whose header has credential columns.
    secret_table: Option<SecretColumns>,
    /// At least one line was removed.
    removed: bool,
}

/// The credential columns of a Markdown table header.
#[derive(Default)]
struct SecretColumns {
    /// A column named by a credential label (`Password`, `Token`, `API Key`):
    /// every data row of the table is removed.
    labeled: bool,
    /// Columns named only by a key name ([`is_key_name`]: `Key`, `App Key`,
    /// `SK`; cell indexes after splitting at `|`): a data row is removed
    /// when such a cell holds a key-like value, so a table of setting names
    /// survives.
    keys: Vec<usize>,
}

impl CredentialLineFilter {
    /// Filters one non-empty, trimmed line whose source was indented by
    /// `indent` whitespace characters. Returns the sanitized line to show, or
    /// `None` when nothing of it may be shown.
    fn filter(&mut self, indent: usize, line: &str) -> Option<String> {
        if let Some(columns) = &self.secret_table {
            if line.contains('|') {
                if columns.labeled || row_has_key_value(line, &columns.keys) {
                    self.removed = true;
                    return None;
                }
            } else {
                self.secret_table = None;
            }
        }
        if let Some(columns) = self.table_header.take() {
            if is_table_separator(line) {
                // The header's "value" is a whole table, not the next line.
                self.value_on_next_line = false;
                self.block_indent = None;
                let labeled = columns.labeled;
                self.secret_table = Some(columns);
                if labeled {
                    self.removed = true;
                    return None;
                }
            }
        }
        if !self.value_on_next_line {
            if let Some(block_indent) = self.block_indent {
                if indent > block_indent {
                    self.removed = true;
                    return None;
                }
                self.block_indent = None;
            }
        }
        // Markdown decoration never hides a label (`**Password**`, `` `token` ``).
        let label = scan_secret_label(&strip_markdown(line));
        if line.contains('|') && self.secret_table.is_none() {
            self.table_header = secret_columns(line);
        }
        if self.value_on_next_line || label != SecretLabel::None {
            // Either this line holds the value of a bare label on the
            // previous line, or it names a credential itself. A dropped value
            // line that is itself a bare label keeps the next line pending.
            self.removed = true;
            self.value_on_next_line = label == SecretLabel::ValueOnNextLine;
            if self.value_on_next_line {
                self.block_indent = Some(indent);
            }
            return None;
        }
        match sanitize_result_text(line) {
            Ok(safe) => (!safe.is_empty()).then_some(safe),
            Err(_) => {
                self.removed = true;
                None
            }
        }
    }

    /// A blank line ends a Markdown table (but not a pending value: a bare
    /// label's value may follow after a blank line).
    fn blank_line(&mut self) {
        self.table_header = None;
        self.secret_table = None;
    }
}

/// A Markdown table separator row: `|---|:--:|`, `--- | ---`.
fn is_table_separator(line: &str) -> bool {
    line.contains('|')
        && line.contains('-')
        && line
            .chars()
            .all(|character| matches!(character, '|' | '-' | ':') || character.is_whitespace())
}

/// The credential columns of a Markdown table header row, or `None` when it
/// has none ([`names_credential_column`], [`is_key_name`]). Headers such as
/// `Token count`, `Input Token` or "token count" in Chinese are not
/// credential columns.
fn secret_columns(line: &str) -> Option<SecretColumns> {
    let mut columns = SecretColumns::default();
    for (index, cell) in line.split('|').enumerate() {
        let cell = normalize_label_text(&strip_markdown(cell.trim()));
        let cell = strip_trailing_note(&cell);
        if cell.is_empty() {
            continue;
        }
        if names_credential_column(cell) {
            columns.labeled = true;
        } else if is_key_name(
            &cell
                .iter()
                .filter(|character| !character.is_whitespace())
                .collect::<String>(),
        ) {
            columns.keys.push(index);
        }
    }
    (columns.labeled || !columns.keys.is_empty()).then_some(columns)
}

/// Words that make `<word>key` (`appKey`, `secret_key`, `Signing-Key`) the
/// name of a key.
const KEY_PREFIXES: [&str; 15] = [
    "access",
    "account",
    "app",
    "auth",
    "client",
    "encryption",
    "license",
    "master",
    "private",
    "secret",
    "server",
    "session",
    "sign",
    "signing",
    "webhook",
];

/// A key name without a credential label, lowercase with separators
/// removed: `key`, `sk`, `ak`, or one of [`KEY_PREFIXES`] followed by `key`.
/// Names such as `cachekey` or `sortkey` are not keys.
fn is_key_name(joined: &str) -> bool {
    matches!(joined, "key" | "sk" | "ak")
        || joined
            .strip_suffix("key")
            .is_some_and(|prefix| KEY_PREFIXES.contains(&prefix))
}

/// A data row whose cell in one of the `keys` columns holds a key-like value.
fn row_has_key_value(line: &str, keys: &[usize]) -> bool {
    let cells = line.split('|').collect::<Vec<_>>();
    keys.iter().any(|index| {
        cells.get(*index).is_some_and(|cell| {
            let cell = strip_markdown(cell.trim());
            let value = cell
                .trim()
                .trim_matches(|character: char| is_quote(character))
                .chars()
                .collect::<Vec<_>>();
            looks_like_key_value(&value)
        })
    })
}

/// Folds full-width forms, lowercases, and maps `_` / `-` to a space, the
/// view in which [`SECRET_LABELS`] are matched.
fn normalize_label_text(text: &str) -> Vec<char> {
    fold_width(text)
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| match character {
            '_' | '-' => ' ',
            other => other,
        })
        .collect()
}

/// Drops trailing whitespace, decoration, colons and one bracketed note
/// (`Password (prod):`, or a full-width bracketed note after a Chinese
/// label).
fn strip_trailing_note(text: &[char]) -> &[char] {
    let trim = |text: &[char]| -> usize {
        text.len()
            - text
                .iter()
                .rev()
                .take_while(|character| {
                    character.is_whitespace() || is_decoration(**character) || **character == ':'
                })
                .count()
    };
    let mut end = trim(text);
    if matches!(text[..end].last(), Some(')' | ']')) {
        if let Some(open) = text[..end]
            .iter()
            .rposition(|character| matches!(character, '(' | '['))
        {
            end = trim(&text[..open]);
        }
    }
    &text[..end]
}

/// Words that make a column header ending with a label a count or a usage
/// figure (`Input Token`, `Max Token`, "token usage" in Chinese) rather
/// than a credential.
const COUNT_WORDS: [&str; 15] = [
    "count",
    "usage",
    "input",
    "output",
    "total",
    "used",
    "max",
    "limit",
    "cost",
    "\u{6570}",         // number
    "\u{91cf}",         // amount
    "\u{8f93}\u{5165}", // input
    "\u{8f93}\u{51fa}", // output
    "\u{603b}",         // total
    "\u{6d88}\u{8017}", // consumption
];

/// A table header cell that names a credential column: it ends with a
/// credential label (`Password`, `AccessToken`, "initial password" in
/// Chinese) and is not a count ([`COUNT_WORDS`]).
fn names_credential_column(cell: &[char]) -> bool {
    let text = cell.iter().collect::<String>();
    SECRET_LABELS.iter().any(|label| text.ends_with(label))
        && !COUNT_WORDS.iter().any(|word| text.contains(word))
}

fn leading_whitespace(line: &str) -> usize {
    line.chars()
        .take_while(|character| character.is_whitespace())
        .count()
}

/// Drops Markdown decoration: leading heading, list and quote markers, bold
/// markers and backticks.
fn strip_markdown(line: &str) -> String {
    line.trim_start_matches(['#', '-', '*', '>', ' '])
        .replace("**", "")
        .replace('`', "")
}

/// Credential labels a user or Agent may write in front of a secret. Matching
/// is case-insensitive, `_` and `-` match a space (`API_KEY`, `access-key`),
/// and full-width letters and punctuation match their ASCII forms. Chinese
/// labels are written as escapes because Runtime production source stays free
/// of Han text; the comment gives each meaning.
const SECRET_LABELS: [&str; 28] = [
    "password",
    "passwd",
    "passphrase",
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
    "\u{79c1}\u{94a5}",         // private key
    "\u{79c1}\u{9470}",         // private key, traditional
    "\u{4ee4}\u{724c}",         // token
    "\u{51ed}\u{8bc1}",         // credential
    "\u{6191}\u{8b49}",         // credential, traditional
    "\u{51ed}\u{636e}",         // credential, variant
    "\u{6388}\u{6743}\u{7801}", // authorization code
    "\u{6388}\u{6b0a}\u{78bc}", // authorization code, traditional
    "\u{9a8c}\u{8bc1}\u{7801}", // verification code
];

/// Chinese predicates between a label and its value: copulas ("the password
/// is ..."), verbs that set it ("the password was changed to ...") and "as
/// follows". The value must still look like a credential.
const LABEL_PREDICATES: [&str; 30] = [
    "\u{662f}",                         // is
    "\u{4e3a}",                         // as
    "\u{70ba}",                         // as, traditional
    "\u{7232}",                         // as, variant
    "\u{5c31}\u{662f}",                 // is just
    "\u{90fd}\u{662f}",                 // are all
    "\u{8fd8}\u{662f}",                 // is still
    "\u{9084}\u{662f}",                 // is still, traditional
    "\u{6539}\u{6210}",                 // changed to
    "\u{6539}\u{4e3a}",                 // changed to
    "\u{6539}\u{70ba}",                 // changed to, traditional
    "\u{4fee}\u{6539}\u{4e3a}",         // modified to
    "\u{4fee}\u{6539}\u{6210}",         // modified to
    "\u{8bbe}\u{4e3a}",                 // set to
    "\u{8bbe}\u{6210}",                 // set to
    "\u{8a2d}\u{70ba}",                 // set to, traditional
    "\u{8bbe}\u{7f6e}\u{4e3a}",         // set to
    "\u{8bbe}\u{7f6e}\u{6210}",         // set to
    "\u{8a2d}\u{7f6e}\u{70ba}",         // set to, traditional
    "\u{8a2d}\u{5b9a}\u{70ba}",         // set to, traditional
    "\u{6362}\u{6210}",                 // replaced with
    "\u{6362}\u{4e3a}",                 // replaced with
    "\u{63db}\u{6210}",                 // replaced with, traditional
    "\u{66f4}\u{65b0}\u{4e3a}",         // updated to
    "\u{66f4}\u{65b0}\u{6210}",         // updated to
    "\u{66f4}\u{65b0}\u{70ba}",         // updated to, traditional
    "\u{91cd}\u{7f6e}\u{4e3a}",         // reset to
    "\u{91cd}\u{7f6e}\u{6210}",         // reset to
    "\u{5982}\u{4e0b}",                 // as follows
    "\u{5982}\u{4e0b}\u{6240}\u{793a}", // as shown below
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretLabel {
    /// The line names no credential.
    None,
    /// The line names a credential and may carry its value.
    OnLine,
    /// The line is a bare label: only the label, or the label followed by
    /// nothing but separators and the opening of a multi-line value (`|`,
    /// `>`, `[`, `{`, `(`, `\`, a quote), or a Chinese label followed by a
    /// predicate such as "is" or "changed to", or a short Chinese phrase and
    /// a final colon.
    /// The value is expected on the next line.
    ValueOnNextLine,
}

/// Separators that turn a credential word into a label: a colon or equals
/// sign (after full-width folding) or any whitespace.
fn is_label_separator(character: char) -> bool {
    matches!(character, ':' | '=') || character.is_whitespace()
}

/// Quotation marks: ASCII quotes and backticks, and the Chinese corner
/// brackets, lenticular brackets, curly quotes and title marks.
fn is_quote(character: char) -> bool {
    matches!(
        character,
        '"' | '\''
            | '`'
            | '\u{300c}'
            | '\u{300d}'
            | '\u{300e}'
            | '\u{300f}'
            | '\u{3010}'
            | '\u{3011}'
            | '\u{201c}'
            | '\u{201d}'
            | '\u{2018}'
            | '\u{2019}'
            | '\u{300a}'
            | '\u{300b}'
    )
}

/// Decoration around a label or a value: quotes and Markdown emphasis.
fn is_decoration(character: char) -> bool {
    character == '*' || is_quote(character)
}

/// Characters that open a value continuing on the next lines: YAML block
/// scalar indicators, brackets, a line continuation, or an opening quote.
fn is_continuation(character: char) -> bool {
    matches!(character, '|' | '>' | '-' | '+' | '[' | '{' | '(' | '\\') || is_decoration(character)
}

/// The length of a printable ASCII run without spaces at `value`.
fn ascii_run(value: &[char]) -> usize {
    value
        .iter()
        .take_while(|character| character.is_ascii_graphic())
        .count()
}

/// A run of at least six printable ASCII characters without spaces that
/// contains a digit or a symbol, starting right at `value`. Ordinary Chinese
/// words after a label (`... is empty`) never qualify.
fn looks_like_credential(value: &[char]) -> bool {
    let run = &value[..ascii_run(value)];
    run.len() >= 6
        && run
            .iter()
            .any(|character| character.is_ascii_digit() || character.is_ascii_punctuation())
}

/// Folds full-width ASCII forms (`：`, `＝`, `ＡＰＩ`) and the ideographic
/// space to ASCII, keeping the case.
fn fold_width(line: &str) -> String {
    line.chars()
        .map(|character| match character {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => {
                char::from_u32(u32::from(character) - 0xFEE0).unwrap_or(character)
            }
            other => other,
        })
        .collect()
}

fn scan_secret_label(line: &str) -> SecretLabel {
    // Fold full-width forms and lowercase; for label matching, `_` / `-` also
    // match a space. Both views have the same length.
    let plain = fold_width(line);
    let folded = plain
        .chars()
        .flat_map(char::to_lowercase)
        .collect::<Vec<_>>();
    let normalized = folded
        .iter()
        .map(|character| match character {
            '_' | '-' => ' ',
            other => *other,
        })
        .collect::<Vec<_>>();
    let mut result = SecretLabel::None;
    for label in SECRET_LABELS {
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
            // Closing decoration of the label itself (`"password":`, `**token**`).
            let after = end
                + normalized[end..]
                    .iter()
                    .take_while(|character| is_decoration(**character))
                    .count();
            let rest = &normalized[after..];
            let bare_line = normalized[..start]
                .iter()
                .all(|character| character.is_whitespace() || is_decoration(*character));
            // A label, a bracketed note and a colon with an ASCII value
            // (`password(prod):...`, or a full-width note after a Chinese
            // label), like a label followed by the colon itself.
            if names_value_after_note(rest) {
                result = SecretLabel::OnLine;
                continue;
            }
            if rest
                .first()
                .is_none_or(|character| is_label_separator(*character))
            {
                let value_missing = rest
                    .iter()
                    .all(|character| is_label_separator(*character) || is_continuation(*character));
                let assigns = rest.iter().any(|character| !character.is_whitespace());
                if value_missing && (bare_line || assigns) {
                    return SecretLabel::ValueOnNextLine;
                }
                result = SecretLabel::OnLine;
                continue;
            }
            if label_is_ascii {
                continue;
            }
            match scan_chinese_label_value(&folded, end) {
                SecretLabel::ValueOnNextLine => return SecretLabel::ValueOnNextLine,
                SecretLabel::OnLine => result = SecretLabel::OnLine,
                SecretLabel::None => {}
            }
        }
    }
    if result == SecretLabel::None
        && (names_short_credential_key(&plain)
            || names_bare_key_label(&plain)
            || names_command_line_credential(&plain))
    {
        result = SecretLabel::OnLine;
    }
    result
}

/// A bracketed note right after a label (`(prod)`, a full-width note folded
/// to ASCII brackets), then a colon or equals sign and a value that starts
/// with a printable ASCII character. A Chinese value (`(optional): leave
/// empty` in Chinese) is not a credential.
fn names_value_after_note(rest: &[char]) -> bool {
    let mut index = rest
        .iter()
        .take_while(|character| character.is_whitespace())
        .count();
    let close = match rest.get(index) {
        Some('(') => ')',
        Some('[') => ']',
        _ => return false,
    };
    let Some(length) = rest[index + 1..]
        .iter()
        .take(24)
        .position(|character| *character == close)
    else {
        return false;
    };
    index += length + 2;
    while rest
        .get(index)
        .is_some_and(|character| character.is_whitespace())
    {
        index += 1;
    }
    if !matches!(rest.get(index), Some(':' | '=')) {
        return false;
    }
    index += 1;
    while rest
        .get(index)
        .is_some_and(|character| is_label_separator(*character) || is_decoration(*character))
    {
        index += 1;
    }
    rest.get(index)
        .is_some_and(|character| character.is_ascii_graphic())
}

/// Matches one of `phrases` at the start of `value` (the longest one wins)
/// and returns its length.
fn starts_with_phrase(value: &[char], phrases: &[&str]) -> Option<usize> {
    phrases
        .iter()
        .map(|phrase| phrase.chars().collect::<Vec<_>>())
        .filter(|phrase| value.starts_with(phrase))
        .map(|phrase| phrase.len())
        .max()
}

/// A Chinese label written straight against its value, or followed by
/// decoration, a predicate ([`LABEL_PREDICATES`], optionally preceded by
/// "already" and followed by the perfective particle), separators or quotes.
/// Only a credential-looking or quoted value counts, so sentences such as
/// "the code is empty" are kept.
fn scan_chinese_label_value(folded: &[char], end: usize) -> SecretLabel {
    let mut index = end;
    let mut quoted = false;
    while let Some(character) = folded.get(index).filter(|c| is_decoration(**c)) {
        quoted |= is_quote(*character);
        index += 1;
    }
    let verb_start = index;
    // "already" (U+5DF2, optionally followed by U+7ECF / U+7D93).
    let mut adverb = 0;
    if folded.get(index) == Some(&'\u{5df2}') {
        adverb = 1;
        if matches!(folded.get(index + 1), Some('\u{7ecf}' | '\u{7d93}')) {
            adverb = 2;
        }
    }
    let rest = &folded[index + adverb..];
    let verb = starts_with_phrase(rest, &LABEL_PREDICATES).unwrap_or(0);
    if verb > 0 {
        index += adverb + verb;
        // The perfective particle (U+4E86): "was changed to".
        if folded.get(index) == Some(&'\u{4e86}') {
            index += 1;
        }
    }
    while let Some(character) = folded
        .get(index)
        .filter(|c| is_label_separator(**c) || is_decoration(**c))
    {
        quoted |= is_quote(*character);
        index += 1;
    }
    let value = &folded[index..];
    if value.is_empty() {
        // "The password is:" or "the password is as follows:" at the end of
        // the line. (A label followed by nothing but decoration and
        // separators never reaches this function.)
        return SecretLabel::ValueOnNextLine;
    }
    // A quoted value (`「opensesame」`) needs no digit or symbol.
    if looks_like_credential(value) || (quoted && ascii_run(value) >= 4) {
        return SecretLabel::OnLine;
    }
    if verb == 0 && ends_a_short_chinese_label(&folded[verb_start..]) {
        return SecretLabel::ValueOnNextLine;
    }
    // Any other wording between the label and its value ("the default
    // password is", "the password, I changed it to", "user name and
    // password are admin and", an arrow or emoji).
    if finds_value_after_label(folded, end) {
        return SecretLabel::OnLine;
    }
    SecretLabel::None
}

/// How far after a Chinese label [`finds_value_after_label`] looks: at most
/// this many Chinese characters, symbols and ASCII words in between.
const LABEL_VALUE_WINDOW: usize = 12;

/// Characters that separate a value from the words around it.
fn is_value_boundary(character: char) -> bool {
    character.is_whitespace()
        || is_quote(character)
        || matches!(
            character,
            ':' | '=' | ',' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '|'
        )
}

/// Searches the rest of the sentence after a Chinese label (`end` is the
/// label end in `folded`) for its value: within [`LABEL_VALUE_WINDOW`]
/// Chinese characters, symbols or ASCII words, an ASCII word that looks like
/// a password ([`looks_like_secret_value`]), or a quoted ASCII word of at
/// least six characters. The search stops at the end of the sentence (a
/// Chinese full stop, or `!`, `?`, `;` standing alone) once anything
/// separated the label from it.
fn finds_value_after_label(folded: &[char], end: usize) -> bool {
    let mut index = end;
    let mut units = 0;
    let mut quoted = false;
    // The ASCII word before: a value right after `commit`, `id` or `port`
    // names that, not the credential.
    let mut reference_next = false;
    while index < folded.len() && units <= LABEL_VALUE_WINDOW {
        let character = folded[index];
        if is_value_boundary(character) {
            quoted = is_quote(character);
            index += 1;
            continue;
        }
        if !character.is_ascii() {
            if character == '\u{3002}' && units > 0 {
                break;
            }
            units += 1;
            quoted = false;
            index += 1;
            continue;
        }
        let start = index;
        while folded
            .get(index)
            .is_some_and(|character| character.is_ascii_graphic() && !is_value_boundary(*character))
        {
            index += 1;
        }
        let word = &folded[start..index];
        if word
            .iter()
            .all(|character| matches!(character, '.' | '!' | '?' | ';'))
        {
            if units > 0 {
                break;
            }
            continue;
        }
        let closed = folded
            .get(index)
            .is_some_and(|character| is_quote(*character));
        if !reference_next
            && (looks_like_secret_value(word) || (quoted && closed && word.len() >= 6))
        {
            return true;
        }
        let word = word.iter().collect::<String>();
        reference_next = REFERENCE_WORDS.contains(&word.as_str());
        units += 1;
        quoted = false;
    }
    false
}

/// ASCII words after which a value names something other than the
/// credential (`commit 3f2a9c1`, `port 50051`).
const REFERENCE_WORDS: [&str; 15] = [
    "branch", "build", "commit", "hash", "id", "issue", "line", "pid", "port", "pr", "sha", "tag",
    "ticket", "uuid", "version",
];

/// Symbols that make a word look like a password.
fn is_password_symbol(character: char) -> bool {
    matches!(
        character,
        '!' | '@' | '#' | '$' | '%' | '^' | '&' | '*' | '+' | '=' | '~' | '?'
    )
}

/// A word, found some way after a label, that looks like a password: six to
/// 128 printable ASCII characters with letters and digits, only digits, or a
/// password symbol next to letters or digits (`Admin@123`, `hunter2`,
/// `123456`), and not a reference to something else
/// ([`looks_like_reference`]).
fn looks_like_secret_value(word: &[char]) -> bool {
    let word = word
        .strip_suffix(&['.'])
        .or_else(|| word.strip_suffix(&[';']))
        .unwrap_or(word);
    if !(6..=128).contains(&word.len()) {
        return false;
    }
    let letters = word.iter().any(char::is_ascii_alphabetic);
    let digits = word
        .iter()
        .filter(|character| character.is_ascii_digit())
        .count();
    let symbol = word.iter().any(|character| is_password_symbol(*character));
    ((letters && digits > 0) || digits == word.len() || (symbol && (letters || digits > 0)))
        && !looks_like_reference(word)
}

/// A value in a `key` / `SK` position or a table key column: ten to 128
/// printable ASCII characters with letters and digits, and mixed case, a
/// password symbol or at least 20 characters. Identifiers with `_`, `.`,
/// `:` or `/` (`user_profile_2024`, `user:1234:profile`) and references
/// ([`looks_like_reference`]) are not key values.
fn looks_like_key_value(value: &[char]) -> bool {
    (10..=128).contains(&value.len())
        && value.iter().all(char::is_ascii_graphic)
        && value.iter().any(char::is_ascii_alphabetic)
        && value.iter().any(char::is_ascii_digit)
        && !value
            .iter()
            .any(|character| matches!(character, '_' | '.' | ':' | '/' | '\\'))
        && ((value.iter().any(char::is_ascii_uppercase)
            && value.iter().any(char::is_ascii_lowercase))
            || value.iter().any(|character| is_password_symbol(*character))
            || value.len() >= 20)
        && !looks_like_reference(value)
}

/// Algorithm, encoding and protocol names, without digits and `-` / `_` /
/// `.` (`SHA-256`, `AES-256-GCM`, `argon2id`, `OAuth2.0`, `HS256`, `base64`).
const ALGORITHM_WORDS: [&str; 49] = [
    "aes",
    "aescbc",
    "aesctr",
    "aesgcm",
    "argon",
    "argond",
    "argoni",
    "argonid",
    "base",
    "bcrypt",
    "blake",
    "blakeb",
    "blakes",
    "chacha",
    "chachapoly",
    "crc",
    "curve",
    "des",
    "ecdh",
    "ecdsa",
    "ed",
    "es",
    "gcm",
    "hmac",
    "hmacsha",
    "hotp",
    "hs",
    "jwt",
    "jwtes",
    "jwths",
    "jwtps",
    "jwtrs",
    "md",
    "oauth",
    "pbkdf",
    "pbkdfsha",
    "pkcs",
    "ps",
    "rc",
    "rs",
    "rsa",
    "rsaoaep",
    "rsapss",
    "scrypt",
    "secp",
    "sha",
    "sm",
    "tls",
    "utf",
];

/// Words that look like a password by their characters but name something
/// else: paths and URLs, emails, `package@version`, versions and IP
/// addresses (also after a name: `Python3.11`), file and dotted names
/// (`auth.rs`, `bcrypt.compare`),
/// dimensions (`120px`, `1.5em`, `1920x1080`), hex colors and algorithm
/// names ([`ALGORITHM_WORDS`]).
fn looks_like_reference(word: &[char]) -> bool {
    let text = word.iter().collect::<String>().to_ascii_lowercase();
    if text.contains('/') || text.contains('\\') {
        return true;
    }
    if let Some((name, rest)) = text.rsplit_once('@') {
        // `vite@5.0.1`, `react@^18`; `Admin@123` is a password.
        let version = rest.contains(['.', '^', '~'])
            && rest.chars().all(|character| {
                character.is_ascii_digit() || matches!(character, '.' | '^' | '~' | 'x')
            });
        let domain = rest.rsplit_once('.').is_some_and(|(host, tld)| {
            !host.is_empty() && !tld.is_empty() && tld.chars().all(|c| c.is_ascii_alphabetic())
        });
        if !name.is_empty() && (version || domain) {
            return true;
        }
    }
    // Versions and IP addresses (`v1.2.3`, `10.0.0.5`), also after a name
    // (`Python3.11`, `iOS17.2`).
    let unversioned = text.trim_start_matches(|character: char| character.is_ascii_alphabetic());
    if unversioned.contains('.')
        && unversioned.starts_with(|character: char| character.is_ascii_digit())
        && unversioned
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
    {
        return true;
    }
    if let Some((_, extension)) = text.rsplit_once('.') {
        let dotted = text.split('.').all(|segment| {
            segment.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
        });
        if dotted
            && (1..=10).contains(&extension.len())
            && extension
                .chars()
                .all(|character| character.is_ascii_alphabetic())
        {
            return true;
        }
    }
    let number = text
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .count();
    if number > 0 {
        let unit = &text[number..];
        if [
            "px", "em", "rem", "pt", "vh", "vw", "ms", "s", "m", "h", "d", "k", "kb", "mb", "gb",
            "tb", "w", "%", "deg", "fps", "hz", "khz", "mhz", "ghz", "min",
        ]
        .contains(&unit)
            || unit
                .strip_prefix('x')
                .is_some_and(|other| !other.is_empty() && other.chars().all(|c| c.is_ascii_digit()))
        {
            return true;
        }
    }
    if let Some(hex) = text.strip_prefix('#') {
        if matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
    }
    let letters = text
        .chars()
        .filter(|character| !character.is_ascii_digit() && !matches!(character, '-' | '_' | '.'))
        .collect::<String>();
    ALGORITHM_WORDS.contains(&letters.as_str())
}

/// A bare key name ([`is_key_name`]: `key`, `SK`, `AK`, `appKey`,
/// `secret_key`) followed by a colon or equals sign and a key-like value
/// ([`looks_like_key_value`]): `key: Abc123!xyz`,
/// `SK: Xyz12345678abcdefghij`. A key that is part of a dotted path
/// (`config.key`) or another identifier (`cache_key`) does not count.
fn names_bare_key_label(line: &str) -> bool {
    let characters = line.chars().collect::<Vec<_>>();
    for (index, separator) in characters.iter().enumerate() {
        if !matches!(separator, ':' | '=') {
            continue;
        }
        let mut key_end = index;
        while key_end > 0 && (characters[key_end - 1] == ' ' || is_quote(characters[key_end - 1])) {
            key_end -= 1;
        }
        let mut key_start = key_end;
        while key_start > 0
            && (characters[key_start - 1].is_ascii_alphanumeric()
                || matches!(characters[key_start - 1], '_' | '-'))
        {
            key_start -= 1;
        }
        let key = characters[key_start..key_end]
            .iter()
            .filter(|character| !matches!(character, '_' | '-'))
            .map(char::to_ascii_lowercase)
            .collect::<String>();
        if !is_key_name(&key) {
            continue;
        }
        let preceded = key_start.checked_sub(1).map(|before| characters[before]);
        if preceded.is_some_and(|before| {
            before.is_ascii()
                && !before.is_whitespace()
                && !is_decoration(before)
                && !matches!(before, '(' | '[' | '{' | ',' | ';' | '|')
        }) {
            continue;
        }
        let mut value_start = index + 1;
        while characters
            .get(value_start)
            .is_some_and(|character| *character == ' ' || is_quote(*character))
        {
            value_start += 1;
        }
        let value = characters[value_start.min(characters.len())..]
            .iter()
            .take(256)
            .take_while(|character| {
                character.is_ascii_graphic()
                    && !is_quote(**character)
                    && !matches!(character, ',' | ';')
            })
            .copied()
            .collect::<Vec<_>>();
        let trimmed = value
            .iter()
            .rposition(|character| !matches!(character, '.' | ')' | ']' | '}'))
            .map_or(&value[..0], |last| &value[..=last]);
        if looks_like_key_value(trimmed) {
            return true;
        }
    }
    false
}

/// A label followed only by a short Chinese phrase and a final colon
/// ("password (test environment):"): the value follows on the next line.
fn ends_a_short_chinese_label(tail: &[char]) -> bool {
    let mut tail = tail;
    while let Some((last, head)) = tail.split_last() {
        if last.is_whitespace() {
            tail = head;
        } else {
            break;
        }
    }
    let Some((':', between)) = tail.split_last() else {
        return false;
    };
    // Stops at the seventh character, so a long line with many labels stays
    // linear.
    let mut count = 0;
    for character in between
        .iter()
        .filter(|character| !character.is_whitespace())
    {
        count += 1;
        if count > 6 || (character.is_ascii() && !matches!(character, '(' | ')' | '[' | ']')) {
            return false;
        }
    }
    true
}

/// `pass`, `pwd` and `pin` keys count only in an assignment with a value of
/// at least four characters: `db_pass=hunter2`, `PIN: 884213`,
/// `userPin=1234`, `PIN` + "code" (U+7801) in Chinese. Words that merely
/// contain them (`pinned`, `passing`, `bypass`, `pass_rate`), test-runner
/// lines (`--- PASS: TestName`) and, after a colon, values without a digit or
/// symbol (type annotations such as `pin: string`) are not credentials.
fn names_short_credential_key(line: &str) -> bool {
    let characters = line.chars().collect::<Vec<_>>();
    for (index, separator) in characters.iter().enumerate() {
        if !matches!(separator, ':' | '=') {
            continue;
        }
        let mut key_end = index;
        while key_end > 0 && characters[key_end - 1] == ' ' {
            key_end -= 1;
        }
        if key_end > 0 && matches!(characters[key_end - 1], '"' | '\'' | '`') {
            key_end -= 1;
        }
        // "PIN code" written in Chinese (simplified or traditional).
        if key_end > 0 && matches!(characters[key_end - 1], '\u{7801}' | '\u{78bc}') {
            key_end -= 1;
        }
        let mut key_start = key_end;
        while key_start > 0
            && (characters[key_start - 1].is_ascii_alphanumeric()
                || matches!(characters[key_start - 1], '_' | '-' | '.'))
        {
            key_start -= 1;
        }
        let key = characters[key_start..key_end].iter().collect::<String>();
        if !is_short_credential_key(&key, *separator) {
            continue;
        }
        let mut value_start = index + 1;
        while matches!(characters.get(value_start), Some(' ' | '"' | '\'' | '`')) {
            value_start += 1;
        }
        // Bounded, so a long line of repeated keys stays linear.
        let value = characters[value_start.min(characters.len())..]
            .iter()
            .take(256)
            .take_while(|character| character.is_ascii_graphic())
            .collect::<String>();
        let value = value.trim_end_matches([',', ';', '.', ')', ']', '}', '"', '\'', '`']);
        // After a colon the value must look like a secret, so that type
        // annotations (`pin: string`, `pin: Pin<&mut Self>`) are kept.
        let secret_like = *separator == '='
            || value.chars().any(|character| {
                character.is_ascii_digit()
                    || matches!(
                        character,
                        '!' | '@' | '#' | '$' | '%' | '^' | '+' | '=' | '~' | '?'
                    )
            });
        if value.len() >= 4
            && secret_like
            && !matches!(
                value.to_ascii_lowercase().as_str(),
                "true" | "false" | "null" | "none"
            )
        {
            return true;
        }
    }
    false
}

fn is_short_credential_key(key: &str, separator: char) -> bool {
    let key = key.trim_start_matches('-');
    // Test runners report `PASS: TestName`.
    if key.is_empty() || (key == "PASS" && separator == ':') {
        return false;
    }
    // Split `db_pass`, `db.pass`, `user-pin` and `userPin` into lowercase words.
    let mut segments = vec![String::new()];
    let mut previous_lowercase = false;
    for character in key.chars() {
        if matches!(character, '_' | '-' | '.') {
            segments.push(String::new());
            previous_lowercase = false;
            continue;
        }
        if character.is_ascii_uppercase() && previous_lowercase {
            segments.push(String::new());
        }
        previous_lowercase = character.is_ascii_lowercase() || character.is_ascii_digit();
        if let Some(segment) = segments.last_mut() {
            segment.push(character.to_ascii_lowercase());
        }
    }
    segments.retain(|segment| !segment.is_empty());
    let Some(last) = segments.last() else {
        return false;
    };
    const KEYS: [&str; 9] = [
        "pass",
        "passwd",
        "password",
        "passphrase",
        "passcode",
        "pwd",
        "pin",
        "pincode",
        "pinnumber",
    ];
    let joined = segments
        .len()
        .checked_sub(2)
        .map(|index| format!("{}{}", segments[index], last));
    KEYS.contains(&last.as_str())
        || joined.is_some_and(|joined| KEYS.contains(&joined.as_str()))
        || (["pass", "passwd", "password", "pwd"]
            .iter()
            .any(|suffix| last.ends_with(suffix))
            && ![
                "bypass",
                "compass",
                "surpass",
                "trespass",
                "overpass",
                "underpass",
                "encompass",
            ]
            .contains(&last.as_str()))
}

/// Credentials written as command-line options: `mysql -p<password>` (and the
/// other MySQL / MariaDB clients) or `--password=`, `curl` / `wget` with
/// `-u` / `--user` and `user:password`, and the password options of other
/// common clients: `sshpass -p`, `redis-cli -a` / `--pass`, `docker` /
/// `podman` / `helm` / `nerdctl login -p` / `--password`, `mongo` /
/// `mongosh` / `mongodump` / `mongorestore -p` / `--password`, `sqlcmd` /
/// `bcp -P`, `ldapsearch` (and the other OpenLDAP clients) `-w`, `zip` /
/// `unzip -P`, `7z -p<password>`, `keytool -storepass` / `-keypass`,
/// `smbclient -U user%password` and `lftp -u user,password`. A tool's
/// options end at a shell separator (`|`, `&&`, `||`, `;`, `&`) or the next
/// command name, so `docker run -p 8080:80`, `ssh -p 22` and `redis-cli -h`
/// are kept.
fn names_command_line_credential(line: &str) -> bool {
    #[derive(Clone, Copy)]
    enum Tool {
        Mysql,
        Http,
        /// Options whose next token (or `--option=` value) is a password.
        Separate(&'static [&'static str]),
        /// An option with the password attached (`7z -p<password>`).
        Attached(&'static str),
        /// An option with `user<separator>password` (`smbclient -U`,
        /// `lftp -u`).
        UserPair(&'static str, char),
        /// A registry client: its password options count after `login`.
        Registry,
    }
    const PASSWORD: &[&str] = &["-p", "--password"];
    let tokens = line
        .split(|character: char| character.is_whitespace() || !character.is_ascii())
        .map(|token| token.trim_matches(['"', '\'', '`', '(', ')']))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let mut tool = None;
    for (index, raw_token) in tokens.iter().enumerate() {
        // `cmd; next` written without a space before the separator.
        let ends_command = raw_token.len() > 1 && raw_token.ends_with(';');
        let token = raw_token.trim_end_matches(';');
        if matches!(token, "" | "|" | "||" | "&&" | "&") {
            tool = None;
            continue;
        }
        let name = token.rsplit('/').next().unwrap_or(token);
        let next = match name {
            "mysql" | "mysqladmin" | "mysqldump" | "mysqlimport" | "mysqlshow" | "mysqlcheck"
            | "mysqlslap" | "mariadb" | "mariadb-dump" | "mariadb-admin" => Some(Some(Tool::Mysql)),
            "curl" | "wget" => Some(Some(Tool::Http)),
            "sshpass" | "mongo" | "mongosh" | "mongodump" | "mongorestore" => {
                Some(Some(Tool::Separate(PASSWORD)))
            }
            "redis-cli" => Some(Some(Tool::Separate(&["-a", "--pass"]))),
            "sqlcmd" | "bcp" | "zip" | "unzip" => Some(Some(Tool::Separate(&["-P"]))),
            "ldapsearch" | "ldapmodify" | "ldapadd" | "ldapdelete" | "ldappasswd" => {
                Some(Some(Tool::Separate(&["-w"])))
            }
            "keytool" => Some(Some(Tool::Separate(&[
                "-storepass",
                "-keypass",
                "-srcstorepass",
                "-deststorepass",
            ]))),
            "7z" | "7za" | "7zz" => Some(Some(Tool::Attached("-p"))),
            "smbclient" => Some(Some(Tool::UserPair("-U", '%'))),
            "lftp" => Some(Some(Tool::UserPair("-u", ','))),
            "docker" | "podman" | "helm" | "nerdctl" => Some(Some(Tool::Registry)),
            // Another command ends the previous tool's options
            // (`curl ... && docker run -u 1000:1000`, `sshpass ... ssh -p 22`).
            "ssh" | "scp" | "sftp" | "git" | "npm" | "npx" | "pnpm" | "yarn" | "cargo"
            | "python" | "python3" | "node" | "go" | "make" | "kubectl" | "sudo" | "echo"
            | "cd" | "export" => Some(None),
            _ => None,
        };
        if let Some(next) = next {
            tool = next;
            if ends_command {
                tool = None;
            }
            continue;
        }
        let found = match tool {
            Some(Tool::Mysql) => {
                let attached = token.starts_with("-p") && token.len() > 2;
                let long = token
                    .strip_prefix("--password=")
                    .is_some_and(|value| !value.is_empty());
                attached || long
            }
            Some(Tool::Http) => {
                let credentials = match token {
                    "-u" | "-U" | "--user" | "--proxy-user" => tokens.get(index + 1).copied(),
                    _ => token
                        .strip_prefix("--user=")
                        .or_else(|| token.strip_prefix("--proxy-user="))
                        .or_else(|| token.strip_prefix("-u"))
                        .or_else(|| token.strip_prefix("-U"))
                        .filter(|value| !value.is_empty() && !value.starts_with('-')),
                };
                credentials.is_some_and(|value| {
                    value
                        .split_once(':')
                        .is_some_and(|(_, secret)| !secret.is_empty())
                })
            }
            Some(Tool::Separate(options)) => {
                let separate = options.contains(&token)
                    && tokens
                        .get(index + 1)
                        .is_some_and(|value| !value.starts_with('-'));
                let joined = options.iter().any(|option| {
                    option.starts_with("--")
                        && token
                            .strip_prefix(option)
                            .and_then(|rest| rest.strip_prefix('='))
                            .is_some_and(|value| !value.is_empty())
                });
                separate || joined
            }
            Some(Tool::Attached(option)) => token
                .strip_prefix(option)
                .is_some_and(|value| !value.is_empty()),
            Some(Tool::UserPair(option, separator)) => {
                let value = if token == option {
                    tokens.get(index + 1).copied()
                } else {
                    token.strip_prefix(option).filter(|value| !value.is_empty())
                };
                value.is_some_and(|value| {
                    value
                        .split_once(separator)
                        .is_some_and(|(_, secret)| !secret.is_empty())
                })
            }
            Some(Tool::Registry) => {
                if token == "login" {
                    tool = Some(Tool::Separate(PASSWORD));
                }
                false
            }
            None => false,
        };
        if found {
            return true;
        }
        if ends_command {
            tool = None;
        }
    }
    false
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
    fn result_excerpts_use_the_same_credential_line_filter() {
        let mut r = OutcomeRegistry::default();
        r.observe(
            "s",
            "已创建测试账号\n**密码**：Abc123!\n令牌：\ntok-live-1234\n修改密码重置页面已完成\n令牌桶限流已改成 10",
            None,
            200,
            "hook:Stop",
        );
        let result = r.get("s", 0, 220).unwrap();
        assert_eq!(
            result.summary,
            "已创建测试账号\n修改密码重置页面已完成\n令牌桶限流已改成 10"
        );
        assert!(result.truncated);

        // A copula followed by an ordinary word is a normal sentence.
        r.observe(
            "s",
            "验证码为空时报错，已修复\n检查密码是否正确的逻辑也补了测试",
            None,
            300,
            "hook:Stop",
        );
        let result = r.get("s", 0, 320).unwrap();
        assert_eq!(
            result.summary,
            "验证码为空时报错，已修复\n检查密码是否正确的逻辑也补了测试"
        );
        assert!(!result.truncated);

        r.observe(
            "s",
            "我的密码是Abc123!\n- Password\n\nhunter2\n其余完成",
            None,
            400,
            "hook:Stop",
        );
        let result = r.get("s", 0, 420).unwrap();
        assert_eq!(result.summary, "其余完成");
        assert!(result.truncated);

        // A result that is only a credential leaves no excerpt.
        r.observe(
            "s",
            "API key：sk-live-abcdefghijklmnop",
            None,
            500,
            "hook:Stop",
        );
        assert_eq!(r.get("s", 0, 520).unwrap().observed_at, 400);

        // The same setter verbs, quotes, unlabeled key formats, multi-line
        // values and command-line credentials as in the prompt.
        r.observe(
            "s",
            "服务器 root 密码设置为 P@ssw0rd2026\n签名密钥用 9f8e7d6c5b4a39281706f5e4d3c2b1a0。\n口令「opensesame」\n配置：\n  password: |\n    Abc123!\n  user: app\nmysql -uroot -pHunter2024 app 已连通\n其余完成",
            None,
            600,
            "hook:Stop",
        );
        let result = r.get("s", 0, 620).unwrap();
        assert_eq!(result.summary, "配置：\nuser: app\n其余完成");
        assert!(result.truncated);
    }

    #[test]
    fn prompts_drop_unlabeled_key_formats_next_to_chinese_text_or_full_width_punctuation() {
        for line in [
            "用这个AIzaSyD-9tSrke72PouQMnMX-a7eZSW0jkFMBWY调一下地图接口",
            "地图接口用这个：AIzaSyD-9tSrke72PouQMnMX-a7eZSW0jkFMBWY",
            "Google Maps key：AIzaSyD-9tSrke72PouQMnMX-a7eZSW0jkFMBWY",
            "地图 key：AIzaSyD-9tSrke72PouQMnMX-a7eZSW0jkFMBWY，用它调一下",
            "请用 AIzaSyD-9tSrke72PouQMnMX-a7eZSW0jkFMBWY。",
            "用glpat-abcdefghij1234567890推一下",
            "用 glpat-AbCdEfGhIjKlMnOpQrSt，推送到仓库",
            // Split so push protection does not mistake the samples for keys.
            concat!("stripe 用sk_", "live_abcdefghij1234567890ABCD测一下"),
            concat!("限制权限的用rk_", "live_abcdefghij1234567890ABCD"),
            concat!("测试环境用sk_", "test_abcdefghij1234567890ABCD"),
            "Webhook 签名用 9f8e7d6c5b4a39281706f5e4d3c2b1a0。",
            "hf_AbCdEfGhIjKlMnOpQrStUvWxYz01234567（HuggingFace）",
            "发布用npm_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789",
            "Slack 用xapp-1-A0123456789-1234567890123-abcdef，别外传",
            "旧的xoxa-2-1234-abcd也停用",
            "OAuth 用gho_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789",
            "服务端用ghs_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789，",
            "用户态ghu_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789。",
            "AK：LTAI5tAbCdEfGh12 SK：Xyz12345678abcdefghij",
        ] {
            let (text, truncated) = sanitize_prompt(&format!("开始\n{line}\n结束")).unwrap();
            assert_eq!(text, "开始\n结束", "{line}");
            assert!(truncated, "{line}");
        }
        // Identifiers that only start like a key format are kept.
        for sentence in [
            "调用hf_hub_download下载模型",
            "检查npm_config_registry配置",
            "用 altair 画图",
            "flexapp-server 起不来",
            "highs_count 统计不对",
            "ghs_count 是多少",
        ] {
            assert_eq!(
                sanitize_prompt(&format!("{sentence}\n然后跑测试")).unwrap(),
                (format!("{sentence}\n然后跑测试"), false),
                "{sentence}"
            );
        }
    }

    #[test]
    fn prompts_drop_command_line_and_short_key_credentials() {
        for line in [
            "mysql -h 10.0.0.5 -uroot -pHunter2024 app 连不上，帮我看看",
            "mysqldump -uroot -pHunter2024 app 导出一下",
            "mysqladmin --password=Hunter2024 status",
            "用 curl -u admin:Hunter2024 测一下接口",
            "用curl -uadmin:Hunter2024测一下",
            "curl --user=admin:Hunter2024 看返回",
            "wget --user admin:Hunter2024 下载",
            "db_pass=hunter2",
            "DB_PASS=hunter2",
            "pass：Abc123!",
            "Pass: hunter2",
            "PIN：884213",
            "pin: 1234",
            "PIN码：884213",
            "userPin=1234",
            "sim_pin = 0000",
            "rootpass=hunter2",
            "\"db_pass\": \"hunter2\"",
            "passphrase: correct horse battery staple",
        ] {
            let (text, truncated) = sanitize_prompt(&format!("开始\n{line}\n结束")).unwrap();
            assert_eq!(text, "开始\n结束", "{line}");
            assert!(truncated, "{line}");
        }
        for sentence in [
            "pinned: yes",
            "spinner: dots",
            "mapping: none",
            "passing tests 都过了",
            "bypass=false",
            "pass_rate: 95%",
            "--- PASS: TestCheckout (0.01s)",
            "Pass: 12",
            "PIN 码长度 6 位",
            "mysql -uroot -p app 连不上",
            "mysql -P3306 app",
            "curl -u admin 然后手动输入",
            "compass: north",
            "pin: string",
            "userPin: number",
            "pin: Pin<&mut Self>",
        ] {
            assert_eq!(
                sanitize_prompt(&format!("{sentence}\n然后跑测试")).unwrap(),
                (format!("{sentence}\n然后跑测试"), false),
                "{sentence}"
            );
        }
    }

    #[test]
    fn values_that_continue_on_the_next_lines_are_dropped() {
        assert_eq!(
            sanitize_prompt("db:\n  password: |\n    Abc123!\n    second line\n  user: app")
                .unwrap(),
            ("db:\nuser: app".to_owned(), true)
        );
        for (source, expected) in [
            (
                "先看配置\ntoken: |\n  abcDEF123\n然后部署",
                "先看配置\n然后部署",
            ),
            (
                "先看配置\npassword: >-\n  Abc123!\n  more\n然后部署",
                "先看配置\n然后部署",
            ),
            (
                "先看配置\n\"password\": [\n  \"Abc123!\",\n  \"Def456?\"\n]\n然后部署",
                "先看配置\n]\n然后部署",
            ),
            (
                "先看配置\npassword = (\n  \"Qwer1234!\"\n)\n然后部署",
                "先看配置\n)\n然后部署",
            ),
            (
                "先看配置\nsecret: {\n  a1b2c3\n}\n然后部署",
                "先看配置\n}\n然后部署",
            ),
            (
                "测试账号\n**密码**\nQwer1234!\n然后部署",
                "测试账号\n然后部署",
            ),
            (
                "先登录\n**Password**\nhunter2024\n然后部署",
                "先登录\n然后部署",
            ),
            ("先登录\n`token`\nabcDEF123\n然后部署", "先登录\n然后部署"),
            ("先登录\n「密码」\nopensesame\n然后部署", "先登录\n然后部署"),
            (
                "先登录\n测试账号的密码如下：\nAbc123!\n然后部署",
                "先登录\n然后部署",
            ),
            (
                "先登录\n数据库密码（测试环境）：\nAbc123!\n然后部署",
                "先登录\n然后部署",
            ),
            ("先登录\n密码改成：\nAbc123!\n然后部署", "先登录\n然后部署"),
        ] {
            let (text, truncated) = sanitize_prompt(source).unwrap();
            assert_eq!(text, expected, "{source}");
            assert!(truncated, "{source}");
        }
        // Deeper-indented lines after an ordinary line are not values.
        assert_eq!(
            sanitize_prompt("步骤：\n  1. 打开设置\n  2. 保存").unwrap(),
            ("步骤：\n1. 打开设置\n2. 保存".to_owned(), false)
        );
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
            "我的密码是 hunter2",
            "密码是：Abc123!",
            "令牌为 abc123",
            "密码Abc123!",
            "验证码884213已发送",
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
            "令牌為 Abc-123",
            // A predicate other than a single copula, or quotes.
            "测试账号的密码改成了 Qwer1234!，你登录试试",
            "服务器 root 密码设置为 P@ssw0rd2026",
            "把数据库密码改为 Abc123!",
            "数据库密码就是 Abc123!",
            "密码设为Abc123!",
            "令牌换成 9f8e7d6c5b4a",
            "密码已经改成 Welcome2026",
            "密码重置为 Welcome2026",
            "令牌更新为：tok-2026-x",
            "口令「opensesame」",
            "密码“Abc123!”",
            "密码【Abc123!】",
            "密码『Abc123!』",
            "密码：「Abc123!」",
            "**密码**：Abc123!",
            "`token`: abc",
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
        // Words that merely contain a label, or a label followed by a copula
        // and ordinary words, are not credentials.
        for sentence in [
            "修改密码重置页面的按钮",
            "把令牌桶限流改成 10",
            "验证码为空时报错",
            "检查密码是否正确",
            "令牌为 abc",
            "验证码为6位数字",
            "密码是abcdef",
            "密钥ID是多少",
            "密码改成 bcrypt 加密",
            "把令牌有效期改成 30 分钟",
            "密码已过期",
            "密码重置页面改为新样式",
            "点击“修改密码”按钮",
            "验证码为空时报错：",
        ] {
            assert_eq!(
                sanitize_prompt(&format!("{sentence}\n然后跑测试")).unwrap(),
                (format!("{sentence}\n然后跑测试"), false),
                "{sentence}"
            );
        }
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
        // Short prefixes count at the start of a key, also after a colon or
        // Chinese text.
        for line in [
            "用sk-proj-abcdefghij1234567890调一下".to_owned(),
            "OPENAI_KEY:sk-proj-abcdefghij1234567890".to_owned(),
            // Split so push protection does not mistake the sample for a key.
            format!("AWS 用 {}{}", "AKIA", "ABCDEFGHIJ012345"),
        ] {
            assert_line_removed(&line);
        }
        // Words that merely contain `sk-` or `akia` are not keys.
        for sentence in [
            "task-runner 起不来，帮我看看",
            "用 flask-login 做登录",
            "disk-usage 报警阈值改成 80%",
            "切到 agent-desk-redesign 分支跑一下测试",
            "sk-learn 的版本",
            "Slovakia 的时区不对",
        ] {
            assert_eq!(
                sanitize_prompt(&format!("{sentence}\n然后跑测试")).unwrap(),
                (format!("{sentence}\n然后跑测试"), false),
                "{sentence}"
            );
        }
    }

    /// Asserts that `line` is removed between two ordinary lines.
    fn assert_line_removed(line: &str) {
        let (text, truncated) = sanitize_prompt(&format!("开始\n{line}\n结束")).unwrap();
        assert_eq!(text, "开始\n结束", "{line}");
        assert!(truncated, "{line}");
    }

    /// Asserts that nothing of `sentence` is removed (placeholders may
    /// replace paths, URLs and hosts).
    fn assert_line_kept(sentence: &str) {
        let (text, truncated) = sanitize_prompt(&format!("{sentence}\n然后跑测试")).unwrap();
        assert!(!truncated, "{sentence} -> {text}");
        assert_eq!(text.lines().count(), 2, "{sentence} -> {text}");
    }

    #[test]
    fn chinese_labels_find_a_credential_value_anywhere_in_the_sentence() {
        for line in [
            "初始密码默认是 Admin@123",
            "初始密码默认为Admin@123",
            "密码统一是 Abc123!",
            "测试环境的密码统一用 Abc123!",
            "密码用 Abc123!",
            "密码用的是 Abc123!",
            "密码应该是 Abc123!",
            "密码好像是 Abc123!",
            "密码其实是 Abc123!",
            "密码也是 Abc123!",
            "密码仍然是 Abc123!",
            "密码依然是 Abc123!",
            "密码我改成了 Abc123!",
            "密码变成了 Abc123!",
            "密码被改成了 Abc123!",
            "密码暂时设成 Abc123!",
            "密码临时改成 Abc123!",
            "密码刚改成 Abc123!",
            "密码填 Abc123!",
            "密码输入 Abc123!",
            "密码叫 Abc123!",
            "密码给你 Abc123!",
            "密码发你了：Abc123!",
            "密码在这：Abc123!",
            "用户名和密码分别是 admin 和 Abc123!",
            "密码改了，新的是 Abc123!",
            "密码→Abc123!",
            "密码👉Abc123!",
            "密码、Abc123!",
            "密码。Abc123!",
            "WiFi 默认密码统一是 12345678",
            "令牌我换成了 tok-2026-x9",
            "口令我设成了「opensesame」",
            "私钥：abc123def",
            // A bracketed note between the label and the colon.
            "数据库密码（测试环境）：Abc123!",
            "数据库密码（测试环境）：opensesame",
            "密码(prod)：opensesame",
            "Password（测试）：opensesame",
        ] {
            assert_line_removed(line);
        }
        for sentence in [
            "修改密码的接口在 src/auth.rs 里",
            "密码输入框的宽度改成 320px",
            "密码校验用 bcrypt.compare()",
            "令牌刷新逻辑在 refreshToken() 里",
            "密钥轮换用 AES-256",
            "令牌用 JWT-HS256 签名",
            "密钥对存在 ~/.ssh/id_ed25519",
            "密码已过期，请运行 npm i -D vite@5.0.1",
            "密码输入框边框颜色改成 #FF0000",
            "密码输入框的行高改成 1.5em",
            "验证码倒计时改成 60s",
            "密码模块升级到 Python3.11",
            "密钥文件 id_rsa.pub 已上传",
            "密码页面的 commit 是 3f2a9c1",
            "验证码图片尺寸 120x40",
            "令牌过期返回 401",
            "修复令牌过期后 iOS 17 上白屏的问题",
            "密码（可选）：留空则不修改",
            "密码规则（新）：至少 8 位",
            "密码错误时提示：Invalid credentials",
            "私钥放在 Keychain 里",
        ] {
            assert_line_kept(sentence);
        }
    }

    #[test]
    fn markdown_tables_with_a_credential_column_lose_their_rows() {
        for (source, expected) in [
            (
                "| 用户名 | 密码 |\n|---|---|\n| admin | Abc123! |\n用这个账号登录测一下",
                "用这个账号登录测一下",
            ),
            (
                "测试账号：\n\n| 角色 | 用户名 | 初始密码 |\n|------|--------|------|\n| 管理员 | admin | Admin@2024 |\n| 访客 | guest | Guest@2024 |\n\n可以直接登录。",
                "测试账号：\n可以直接登录。",
            ),
            (
                "| User | Password |\n|:--|:--:|\n| admin | hunter2 |\nthen log in",
                "then log in",
            ),
            ("用户名 | 密码\n--- | ---\nadmin | Abc123!\n然后部署", "然后部署"),
            (
                "| 服务 | Token (prod) | 备注 |\n| --- | --- | --- |\n| api | tok-2026-x | 主 |\n然后部署",
                "然后部署",
            ),
            (
                "| env | AccessToken |\n|---|---|\n| prod | tok-2026-x |\n然后部署",
                "然后部署",
            ),
            // A key column drops only the rows that hold a key-like value.
            (
                "| 服务 | Key |\n|---|---|\n| 地图 | AbCd1234EfGh5678 |\n| timeout | 30s |\n然后部署",
                "| 服务 | Key |\n|---|---|\n| timeout | 30s |\n然后部署",
            ),
            // A blank line ends the table.
            (
                "| 用户名 | 密码 |\n|---|---|\n| admin | Abc123! |\n\n| 步骤 | 状态 |\n|---|---|\n| 构建 | 完成 |",
                "| 步骤 | 状态 |\n|---|---|\n| 构建 | 完成 |",
            ),
        ] {
            assert_eq!(
                sanitize_prompt(source).unwrap(),
                (expected.to_owned(), true),
                "{source}"
            );
        }
        // Tables without a credential column keep their rows.
        for source in [
            "| 文件 | 改动 |\n|---|---|\n| 首页 | +10 |\n然后跑测试",
            "| Key | 作用 |\n|---|---|\n| timeout | 超时时间 |\n| retries | 重试次数 |\n然后跑测试",
        ] {
            assert_eq!(
                sanitize_prompt(source).unwrap(),
                (source.to_owned(), false),
                "{source}"
            );
        }
        // A header that only mentions a credential word is removed itself,
        // as before, but its rows stay.
        for source in [
            "| 模型 | Token 数 | 耗时 |\n|---|---|---|\n| opus | 12000 | 30s |",
            "| 模型 | 输入 Token | 输出 Token |\n|---|---|---|\n| opus | 12000 | 800 |",
            "| Model | Input Token | Output Token |\n|---|---|---|\n| opus | 12000 | 800 |",
        ] {
            let (text, _) = sanitize_prompt(source).unwrap();
            assert!(text.contains("| opus | 12000 |"), "{text}");
        }

        // Result excerpts use the same filter.
        let mut r = OutcomeRegistry::default();
        r.observe(
            "s",
            "已创建测试账号：\n\n| 用户名 | 密码 |\n|---|---|\n| admin | Abc123! |\n\n可以直接登录。",
            None,
            200,
            "hook:Stop",
        );
        let result = r.get("s", 0, 220).unwrap();
        assert_eq!(result.summary, "已创建测试账号：\n可以直接登录。");
        assert!(result.truncated);
    }

    #[test]
    fn keys_glued_to_ascii_punctuation_and_bot_tokens_are_dropped() {
        let hex = "9f8e7d6c5b4a39281706f5e4d3c2b1a0";
        // Built from parts so push protection does not mistake the samples
        // for real tokens.
        let telegram = format!("{}:{}", "123456789", "AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw");
        let discord = [
            "MTA4NjQ5NzY1NDMyMTA5ODc2NQ",
            "GabcDE",
            "abcdefghijklmnopqrstuvwxyz0123456789AB",
        ]
        .join(".");
        for line in [
            format!("{{\"key\":\"{hex}\"}}"),
            format!("配置是 {{\"appKey\":\"{hex}\"}} 为什么还是 401"),
            format!("appKey=\"{hex}\""),
            format!("appKey:'{hex}'"),
            format!("key:{hex}"),
            format!("new Client(\"{hex}\")"),
            format!("Client(key=\"{hex}\")"),
            format!("headers={{\"X-Key\":\"{hex}\"}}"),
            format!("高德key|{hex}"),
            format!("{{\"privateKey\":\"0x{hex}{hex}\"}}"),
            format!("AMapLoader.load({{key:'{hex}'}})"),
            format!("用 {telegram} 发消息"),
            format!("用{telegram}发消息"),
            format!("机器人 {discord} 上线"),
            concat!(
                "psql postgres",
                "://admin:Hunter2024@",
                "10.0.0.5:5432/app 连不上"
            )
            .to_owned(),
            "psql \"host=10.0.0.5 user=admin password=Hunter2024 dbname=app\"".to_owned(),
        ] {
            assert_line_removed(&line);
        }
        for sentence in [
            "{\"name\":\"handleCompanionSnapshotRequest\",\"ok\":true}",
            "store.handQuestionBack(prompt) 改一下",
            "AgentPhoneActionPolicyTests.swift:120 失败",
            "10:30:45 开始",
            "127.0.0.1:8080 连不上",
            "ssh://git@github.com/org/repo.git 拉不下来",
            "https://example.feishu.cn/docx/AbCdEfGhIjKlMnOpQrStUvWx1 打开看看",
            "Sources/App/ControlCenter/AgentPage.swift 改一下",
            "1234567890:abcdefghij 是时间戳:序号",
            "\"version\": \"1.2.3\"",
        ] {
            assert_line_kept(sentence);
        }
    }

    #[test]
    fn more_command_line_password_options_are_dropped() {
        for line in [
            "sshpass -p Hunter2024 ssh root@10.0.0.5 连不上",
            "redis-cli -h 10.0.0.5 -a Hunter2024 ping 超时，帮我看看",
            "redis-cli --pass Hunter2024 info",
            "docker login -u admin -p Hunter2024 registry.example.com",
            "docker login registry.example.com --password=Hunter2024",
            "podman login -p Hunter2024 quay.io",
            "mongo -u admin -p Hunter2024",
            "mongosh --password Hunter2024",
            "sqlcmd -S host -U sa -P Hunter2024",
            "ldapsearch -D cn=admin -w Hunter2024",
            "unzip -P Hunter2024 backup.zip",
            "7z x -pHunter2024 backup.7z",
            "keytool -list -keystore a.jks -storepass Hunter2024",
            "smbclient //host/share -U admin%Hunter2024",
            "lftp -u admin,Hunter2024 ftp.example.com",
        ] {
            assert_line_removed(line);
        }
        for sentence in [
            "docker run -p 8080:80 nginx",
            "ssh -p 22 root@10.0.0.5",
            "redis-cli -h 10.0.0.5 ping",
            "zip -r out.zip dir",
            "7z a -t7z out.7z dir",
            "mongo --port 27017",
            "docker login -u admin registry.example.com",
            "sshpass -f pass.txt ssh root@host",
            "curl -X POST https://api.example.com && docker run -u 1000:1000 alpine",
            "curl -s https://api.example.com; docker run -u 1000:1000 alpine",
        ] {
            assert_line_kept(sentence);
        }
    }

    #[test]
    fn bare_key_labels_with_a_key_like_value_are_dropped() {
        for line in [
            "key：Abc123!xyz",
            "Key: Abc123!xyz",
            "SK：Xyz12345678abcdefghij",
            "SecretKey：Xyz12345678abcdefghij",
            "\"appKey\": \"Abc123!xyz\"",
            "signing_key=Xyz12345678abcdefghij",
        ] {
            assert_line_removed(line);
        }
        for sentence in [
            "primary key: id",
            "cache key: user:1234:profile",
            "key: string",
            "foreign key: user_id",
            "key: Cmd+Shift+P",
            "Key: F12",
            "sort key: createdAt",
            "key: com.example.app2",
            "key: v1.2.3",
            "key: user_profile_2024",
            "cacheKey: userProfileV2",
            "desk: 1234567890ab",
            "task: build2024release",
        ] {
            assert_line_kept(sentence);
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
