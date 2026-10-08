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
/// assignment, or a command-line credential such as `mysql -p<password>`) is
/// removed as a whole. When such a label carries no value on its own line (a
/// Chinese password label with a full-width colon, `token:`, `password: |`,
/// or just `Password`), the next non-empty line is treated as its value and
/// removed too, together with any following lines indented deeper than the
/// label (a YAML block scalar or a bracketed list).
#[derive(Default)]
struct CredentialLineFilter {
    value_on_next_line: bool,
    /// Indentation of the last bare label line: deeper-indented lines after
    /// its value continue that value.
    block_indent: Option<usize>,
    /// At least one line was removed.
    removed: bool,
}

impl CredentialLineFilter {
    /// Filters one non-empty, trimmed line whose source was indented by
    /// `indent` whitespace characters. Returns the sanitized line to show, or
    /// `None` when nothing of it may be shown.
    fn filter(&mut self, indent: usize, line: &str) -> Option<String> {
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
const SECRET_LABELS: [&str; 26] = [
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
        && (names_short_credential_key(&plain) || names_command_line_credential(&plain))
    {
        result = SecretLabel::OnLine;
    }
    result
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
    SecretLabel::None
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
    let between = between
        .iter()
        .filter(|character| !character.is_whitespace())
        .collect::<Vec<_>>();
    between.len() <= 6
        && between
            .iter()
            .all(|character| !character.is_ascii() || matches!(character, '(' | ')' | '[' | ']'))
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
        let value = characters[value_start.min(characters.len())..]
            .iter()
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
/// other MySQL / MariaDB clients) or `--password=`, and `curl` / `wget` with
/// `-u` / `--user` and `user:password`.
fn names_command_line_credential(line: &str) -> bool {
    #[derive(Clone, Copy)]
    enum Tool {
        Mysql,
        Http,
    }
    let tokens = line
        .split(|character: char| character.is_whitespace() || !character.is_ascii())
        .map(|token| token.trim_matches(['"', '\'', '`', '(', ')']))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let mut tool = None;
    for (index, token) in tokens.iter().enumerate() {
        let name = token.rsplit('/').next().unwrap_or(token);
        match name {
            "mysql" | "mysqladmin" | "mysqldump" | "mysqlimport" | "mysqlshow" | "mysqlcheck"
            | "mysqlslap" | "mariadb" | "mariadb-dump" | "mariadb-admin" => {
                tool = Some(Tool::Mysql);
                continue;
            }
            "curl" | "wget" => {
                tool = Some(Tool::Http);
                continue;
            }
            _ => {}
        }
        match tool {
            Some(Tool::Mysql) => {
                let attached = token.starts_with("-p") && token.len() > 2;
                let long = token
                    .strip_prefix("--password=")
                    .is_some_and(|value| !value.is_empty());
                if attached || long {
                    return true;
                }
            }
            Some(Tool::Http) => {
                let credentials = match *token {
                    "-u" | "-U" | "--user" | "--proxy-user" => tokens.get(index + 1).copied(),
                    _ => token
                        .strip_prefix("--user=")
                        .or_else(|| token.strip_prefix("--proxy-user="))
                        .or_else(|| token.strip_prefix("-u"))
                        .or_else(|| token.strip_prefix("-U"))
                        .filter(|value| !value.is_empty() && !value.starts_with('-')),
                };
                if credentials.is_some_and(|value| {
                    value
                        .split_once(':')
                        .is_some_and(|(_, secret)| !secret.is_empty())
                }) {
                    return true;
                }
            }
            None => {}
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
