//! The latest Agent message of each session, for Companion clients. Like
//! result excerpts, message text lives only in Runtime memory: it is never
//! written to SQLite, the spool or an export, and only sanitized, bounded
//! excerpts leave this module.
//!
//! Messages arrive from Provider events (the `Stop` Hook's
//! `last_assistant_message`, Codex agent messages) and, for Claude Code, from
//! the session transcript, which is read from its end on demand and cached by
//! file identity, length and modification time.
use crate::outcome::{message_excerpts, Excerpt};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// Messages older than this are not reported.
pub(crate) const LATEST_MESSAGE_TTL_MS: u64 = 24 * 60 * 60 * 1000;
const MAX_SESSIONS: usize = 256;
/// The first read covers the end of the transcript; each further read
/// doubles the window until [`MAX_TRANSCRIPT_WINDOW`].
const FIRST_TRANSCRIPT_WINDOW: u64 = 64 * 1024;
const MAX_TRANSCRIPT_WINDOW: u64 = 4 * 1024 * 1024;
/// A message recorded this long after its turn's end (clock skew between the
/// Provider's transcript and Runtime receipt) still belongs to that turn.
const TURN_END_SLACK_MS: u64 = 5_000;

/// The latest Agent message of a session, as a Companion receives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestMessage {
    /// A sanitized excerpt of the message.
    pub text: String,
    /// When the Agent wrote the message (milliseconds since the epoch).
    pub at: u64,
    /// The message is the final answer of its turn (it came with the turn's
    /// end, or the turn ended right after it).
    #[serde(rename = "final")]
    pub is_final: bool,
    /// Something of the message is not in `text`.
    pub truncated: bool,
}

/// Which excerpt of a message to return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatestMessageForm {
    /// The end of the message, at most 400 characters (snapshot sessions).
    Glance,
    /// The start of the message within the result excerpt bounds (60 lines,
    /// 4,000 characters).
    Full,
}

/// A session whose latest message is wanted, with what the Runtime knows
/// about its current turn.
#[derive(Clone, Debug)]
pub struct LatestMessageQuery {
    pub session_id: String,
    /// A Claude Code transcript to read the message from.
    pub transcript: Option<PathBuf>,
    pub turn_started_at: Option<u64>,
    pub turn_ended_at: Option<u64>,
}

#[derive(Clone, Debug)]
struct Observed {
    at: u64,
    is_final: bool,
    full: Excerpt,
    glance: Excerpt,
}

impl Observed {
    fn new(text: &str, at: u64, is_final: bool) -> Option<Self> {
        let (full, glance) = message_excerpts(text)?;
        Some(Self {
            at,
            is_final,
            full,
            glance,
        })
    }

    fn message(&self, form: LatestMessageForm, is_final: bool) -> LatestMessage {
        let excerpt = match form {
            LatestMessageForm::Glance => &self.glance,
            LatestMessageForm::Full => &self.full,
        };
        LatestMessage {
            text: excerpt.text.clone(),
            at: self.at,
            is_final,
            truncated: excerpt.truncated,
        }
    }
}

/// The identity of a transcript file at one moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileState {
    device: u64,
    inode: u64,
    length: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

impl FileState {
    fn of(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        }
    }

    fn modified_millis(&self) -> u64 {
        u64::try_from(self.modified_seconds)
            .unwrap_or_default()
            .saturating_mul(1000)
            .saturating_add(u64::try_from(self.modified_nanoseconds / 1_000_000).unwrap_or(0))
    }
}

#[derive(Clone, Debug)]
struct TranscriptCache {
    path: PathBuf,
    file: FileState,
    /// Offset after the last complete line that was examined.
    scanned_to: u64,
    /// The latest qualifying message and its Provider message identifier.
    message: Option<(Observed, Option<String>)>,
}

#[derive(Default)]
pub(crate) struct LatestMessages {
    observed: HashMap<String, Observed>,
    transcripts: HashMap<String, TranscriptCache>,
    #[cfg(test)]
    pub(crate) transcript_reads: usize,
}

impl LatestMessages {
    /// Records an Agent message reported by a Provider event. A message
    /// older than the one already known is ignored; of two at the same time
    /// the final one wins.
    pub fn observe(&mut self, session: &str, text: &str, at: u64, is_final: bool) {
        if self
            .observed
            .get(session)
            .is_some_and(|known| known.at > at || (known.at == at && known.is_final && !is_final))
        {
            return;
        }
        if let Some(observed) = Observed::new(text, at, is_final) {
            self.observed.insert(session.to_owned(), observed);
        }
        self.prune(at);
    }

    /// The latest message of a session: the newer of the last message a
    /// Provider event reported and the last one in its transcript.
    pub fn latest(
        &mut self,
        query: &LatestMessageQuery,
        form: LatestMessageForm,
        now: u64,
    ) -> Option<LatestMessage> {
        let transcript = query
            .transcript
            .as_deref()
            .and_then(|path| self.transcript_message(&query.session_id, path));
        let observed = self.observed.get(&query.session_id).cloned();
        let latest = match (observed, transcript) {
            (Some(observed), Some(transcript)) if transcript.at > observed.at => {
                let is_final = ended_turn_message(query, transcript.at);
                transcript.message(form, is_final)
            }
            (Some(observed), _) => observed.message(form, observed.is_final),
            (None, Some(transcript)) => {
                let is_final = ended_turn_message(query, transcript.at);
                transcript.message(form, is_final)
            }
            (None, None) => return None,
        };
        (now.saturating_sub(latest.at) <= LATEST_MESSAGE_TTL_MS).then_some(latest)
    }

    fn prune(&mut self, now: u64) {
        self.observed
            .retain(|_, message| now.saturating_sub(message.at) <= LATEST_MESSAGE_TTL_MS);
        while self.observed.len() > MAX_SESSIONS {
            let Some(oldest) = self
                .observed
                .iter()
                .min_by_key(|(_, message)| message.at)
                .map(|(session, _)| session.clone())
            else {
                break;
            };
            self.observed.remove(&oldest);
        }
        if self.transcripts.len() > MAX_SESSIONS {
            self.transcripts.clear();
        }
    }

    /// The latest qualifying message of a Claude Code transcript, from the
    /// cache while the file is unchanged. When the file only grew, just the
    /// appended lines are examined.
    fn transcript_message(&mut self, session: &str, path: &Path) -> Option<Observed> {
        let (mut file, state) = open_transcript(path)?;
        let cached = self
            .transcripts
            .get(session)
            .filter(|cache| cache.path == path);
        if let Some(cache) = cached {
            if cache.file == state {
                return cache.message.as_ref().map(|(message, _)| message.clone());
            }
        }
        #[cfg(test)]
        {
            self.transcript_reads += 1;
        }
        let grown = cached.filter(|cache| {
            cache.file.device == state.device
                && cache.file.inode == state.inode
                && cache.scanned_to <= state.length
                && state.length - cache.scanned_to <= MAX_TRANSCRIPT_WINDOW
        });
        let (message, scanned_to) = match grown {
            Some(cache) => {
                let previous = cache.message.clone();
                let (appended, scanned_to) =
                    read_appended(&mut file, cache.scanned_to, state.length)?;
                match latest_record(&appended) {
                    // The newest message continues the cached one (several
                    // text blocks of one message): read it whole again.
                    Some(found)
                        if found.id.is_some()
                            && previous.as_ref().and_then(|(_, id)| id.as_ref())
                                == found.id.as_ref() =>
                    {
                        let (found, scanned_to) = read_tail(&mut file, state.length)?;
                        (found.and_then(|found| found.observed(&state)), scanned_to)
                    }
                    Some(found) => (found.observed(&state).or(previous), scanned_to),
                    None => (previous, scanned_to),
                }
            }
            None => {
                let (found, scanned_to) = read_tail(&mut file, state.length)?;
                (found.and_then(|found| found.observed(&state)), scanned_to)
            }
        };
        self.transcripts.insert(
            session.to_owned(),
            TranscriptCache {
                path: path.to_owned(),
                file: state,
                scanned_to,
                message: message.clone(),
            },
        );
        message.map(|(message, _)| message)
    }
}

/// A transcript message counts as its turn's final answer when that turn
/// has ended and the message was written during it.
fn ended_turn_message(query: &LatestMessageQuery, at: u64) -> bool {
    query.turn_ended_at.is_some_and(|ended_at| {
        at <= ended_at.saturating_add(TURN_END_SLACK_MS)
            && query
                .turn_started_at
                .is_none_or(|started_at| at.saturating_add(TURN_END_SLACK_MS) >= started_at)
    })
}

/// Opens a transcript only when it is a regular file owned by this user
/// that no one else may write, without following a symbolic link.
fn open_transcript(path: &Path) -> Option<(File, FileState)> {
    if !path.is_absolute() || path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o022 != 0
    {
        return None;
    }
    Some((file, FileState::of(&metadata)))
}

/// A qualifying assistant record found in a transcript.
#[derive(Debug, PartialEq, Eq)]
struct FoundRecord {
    text: String,
    at: Option<u64>,
    id: Option<String>,
}

impl FoundRecord {
    fn observed(self, state: &FileState) -> Option<(Observed, Option<String>)> {
        let at = self.at.unwrap_or_else(|| state.modified_millis());
        Some((Observed::new(&self.text, at, false)?, self.id))
    }
}

/// Reads the end of a transcript, doubling the window until a qualifying
/// message is found, the start of the file is reached or the window reaches
/// [`MAX_TRANSCRIPT_WINDOW`]. Returns the message and the offset after the
/// last complete line.
fn read_tail(file: &mut File, length: u64) -> Option<(Option<FoundRecord>, u64)> {
    let mut window = FIRST_TRANSCRIPT_WINDOW.min(length);
    let mut buffer: Vec<u8> = Vec::new();
    let mut start = length;
    loop {
        let next_start = length.saturating_sub(window);
        if next_start < start {
            let mut chunk = vec![0; usize::try_from(start - next_start).ok()?];
            file.seek(SeekFrom::Start(next_start)).ok()?;
            file.read_exact(&mut chunk).ok()?;
            chunk.extend_from_slice(&buffer);
            buffer = chunk;
            start = next_start;
        }
        let complete = complete_lines(&buffer, start == 0);
        if let Some(found) = latest_record(complete) {
            return Some((Some(found), scanned_end(&buffer, start)));
        }
        if start == 0 || window >= MAX_TRANSCRIPT_WINDOW {
            return Some((None, scanned_end(&buffer, start)));
        }
        window = (window * 2).min(MAX_TRANSCRIPT_WINDOW).min(length);
    }
}

/// Reads the lines appended after `from`, up to the last complete one.
fn read_appended(file: &mut File, from: u64, length: u64) -> Option<(Vec<u8>, u64)> {
    let mut appended = vec![0; usize::try_from(length.saturating_sub(from)).ok()?];
    file.seek(SeekFrom::Start(from)).ok()?;
    file.read_exact(&mut appended).ok()?;
    let complete = appended
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    appended.truncate(complete);
    Some((appended, from + complete as u64))
}

/// The offset after the last complete line of a buffer read from `start`.
fn scanned_end(buffer: &[u8], start: u64) -> u64 {
    start
        + buffer
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index as u64 + 1)
}

/// The complete lines of a buffer: a first line cut by the window and a last
/// line still being written are left out.
fn complete_lines(buffer: &[u8], at_file_start: bool) -> &[u8] {
    let begin = if at_file_start {
        0
    } else {
        buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |index| index + 1)
    };
    let end = buffer
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    if begin >= end {
        &[]
    } else {
        &buffer[begin..end]
    }
}

/// The latest qualifying assistant message in complete transcript lines:
/// the main conversation's (not a subagent's `isSidechain` record), neither
/// an API error nor a synthetic or meta record, with non-empty text. Thinking
/// and tool-use blocks are not text; a record holding only them is skipped.
/// Text blocks of the same message written as separate records just before
/// it are joined to it.
fn latest_record(lines: &[u8]) -> Option<FoundRecord> {
    let mut found: Option<FoundRecord> = None;
    let mut earlier_text: Vec<String> = Vec::new();
    for line in lines.rsplit(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let record = assistant_record(line);
        match (&mut found, record) {
            (None, Some(Record::Text(record))) => found = Some(record),
            (None, _) => {}
            (Some(current), Some(Record::Text(record)))
                if current.id.is_some() && record.id == current.id =>
            {
                earlier_text.push(record.text);
            }
            (Some(current), Some(Record::Other { id }))
                if current.id.is_some() && id == current.id => {}
            (Some(_), _) => break,
        }
    }
    let mut found = found?;
    if !earlier_text.is_empty() {
        earlier_text.reverse();
        earlier_text.push(found.text);
        found.text = earlier_text.join("\n\n");
    }
    Some(found)
}

enum Record {
    /// A qualifying assistant record with text.
    Text(FoundRecord),
    /// A main-conversation assistant record without text (tool use,
    /// thinking).
    Other { id: Option<String> },
}

fn assistant_record(line: &[u8]) -> Option<Record> {
    // Most lines are tool results and user records: skip them before parsing.
    if !contains(line, b"\"assistant\"") {
        return None;
    }
    let value: Value = serde_json::from_slice(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("assistant")
        || value.get("isSidechain").and_then(Value::as_bool) == Some(true)
        || value.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true)
        || value.get("isMeta").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let message = value.get("message")?;
    if message.get("model").and_then(Value::as_str) == Some("<synthetic>") {
        return None;
    }
    let id = message
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .map(ToOwned::to_owned);
    let text = match message.get("content") {
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    };
    if text.is_empty() {
        return Some(Record::Other { id });
    }
    let at = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(utc_timestamp_millis);
    Some(Record::Text(FoundRecord { text, at, id }))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Parses an RFC 3339 UTC timestamp (`2026-10-10T17:36:20.123Z`) into
/// milliseconds since the epoch.
fn utc_timestamp_millis(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || !matches!(bytes.last(), Some(b'Z' | b'z'))
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let digits = value.get(range)?;
        digits
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| digits.parse().ok())?
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let fraction = &value[19..value.len() - 1];
    let millis = match fraction.strip_prefix('.') {
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            format!("{digits:0<3}")[..3].parse::<i64>().ok()?
        }
        None if fraction.is_empty() => 0,
        _ => return None,
    };
    // Days since 1970-01-01 in the proleptic Gregorian calendar.
    let year_of_era = if month <= 2 { year - 1 } else { year };
    let era = year_of_era.div_euclid(400);
    let year_in_era = year_of_era - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_in_era * 365 + year_in_era / 4 - year_in_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(seconds * 1000 + millis).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn temp_transcript(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("actrealm-latest-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        root.join(format!("{name}.jsonl"))
    }

    fn write(path: &Path, lines: &[Value]) {
        let mut contents = String::new();
        for line in lines {
            contents.push_str(&line.to_string());
            contents.push('\n');
        }
        fs::write(path, contents).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn append(path: &Path, raw: &str) {
        OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(raw.as_bytes())
            .unwrap();
    }

    fn assistant(id: &str, at: &str, content: Value) -> Value {
        json!({
            "type": "assistant",
            "isSidechain": false,
            "timestamp": at,
            "message": {"id": id, "role": "assistant", "model": "claude-test", "content": content}
        })
    }

    fn text(value: &str) -> Value {
        json!([{"type": "text", "text": value}])
    }

    fn query(path: &Path) -> LatestMessageQuery {
        LatestMessageQuery {
            session_id: "session".to_owned(),
            transcript: Some(path.to_owned()),
            turn_started_at: None,
            turn_ended_at: None,
        }
    }

    const NOW: u64 = 1_791_000_000_000;

    #[test]
    fn utc_timestamps_parse_to_epoch_milliseconds() {
        assert_eq!(utc_timestamp_millis("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            utc_timestamp_millis("2026-10-10T17:36:20.123Z"),
            Some(1_791_653_780_123)
        );
        assert_eq!(
            utc_timestamp_millis("2000-02-29T12:00:00.5Z"),
            Some(951_825_600_500)
        );
        assert_eq!(utc_timestamp_millis("2026-10-10 17:36:20"), None);
        assert_eq!(utc_timestamp_millis("2026-13-10T17:36:20Z"), None);
    }

    #[test]
    fn the_latest_main_conversation_text_is_found_from_the_end() {
        let path = temp_transcript("main");
        write(
            &path,
            &[
                json!({"type": "user", "message": {"role": "user", "content": "private prompt"}}),
                assistant("m1", "2026-10-10T17:30:00.000Z", text("First answer.")),
                assistant(
                    "m2",
                    "2026-10-10T17:31:00.000Z",
                    text("The build is green."),
                ),
                // A subagent's message, thinking, a tool use, an API error and
                // a synthetic message never count.
                json!({
                    "type": "assistant", "isSidechain": true,
                    "timestamp": "2026-10-10T17:32:00.000Z",
                    "message": {"id": "s1", "content": [{"type": "text", "text": "Subagent report"}]}
                }),
                assistant(
                    "m3",
                    "2026-10-10T17:33:00.000Z",
                    json!([{"type": "thinking", "thinking": "private reasoning"}]),
                ),
                assistant(
                    "m3",
                    "2026-10-10T17:33:01.000Z",
                    json!([{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}]),
                ),
                json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "content": "assistant output"}]}}),
                json!({
                    "type": "assistant", "isApiErrorMessage": true,
                    "timestamp": "2026-10-10T17:34:00.000Z",
                    "message": {"id": "e1", "model": "<synthetic>", "content": [{"type": "text", "text": "API Error"}]}
                }),
                json!({
                    "type": "assistant",
                    "timestamp": "2026-10-10T17:34:30.000Z",
                    "message": {"id": "x1", "model": "<synthetic>", "content": [{"type": "text", "text": "No response requested."}]}
                }),
            ],
        );
        // A half-written last line is not read.
        append(
            &path,
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"te",
        );
        let mut latest = LatestMessages::default();
        let message = latest
            .latest(&query(&path), LatestMessageForm::Glance, NOW)
            .unwrap();
        assert_eq!(message.text, "The build is green.");
        assert_eq!(message.at, 1_791_653_460_000);
        assert!(!message.is_final);
        assert!(!message.truncated);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn text_blocks_of_one_message_are_joined() {
        let lines = [
            assistant("m1", "2026-10-10T17:30:00.000Z", text("Earlier.")),
            assistant("m2", "2026-10-10T17:31:00.000Z", text("Part one.")),
            assistant(
                "m2",
                "2026-10-10T17:31:00.500Z",
                json!([{"type": "tool_use", "id": "t", "name": "Read", "input": {}}]),
            ),
            assistant("m2", "2026-10-10T17:31:01.000Z", text("Part two?")),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        let found = latest_record(format!("{lines}\n").as_bytes()).unwrap();
        assert_eq!(found.text, "Part one.\n\nPart two?");
        assert_eq!(found.id.as_deref(), Some("m2"));
    }

    #[test]
    fn a_large_transcript_is_read_backwards_until_a_message_is_found() {
        let path = temp_transcript("large");
        let filler = "x".repeat(200 * 1024);
        write(
            &path,
            &[
                assistant("m1", "2026-10-10T17:30:00.000Z", text("Old answer.")),
                assistant(
                    "m2",
                    "2026-10-10T17:31:00.000Z",
                    text("Answer before the big output."),
                ),
                json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": filler}]}}),
                json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "y".repeat(150 * 1024)}]}}),
            ],
        );
        let mut latest = LatestMessages::default();
        let message = latest
            .latest(&query(&path), LatestMessageForm::Full, NOW)
            .unwrap();
        assert_eq!(message.text, "Answer before the big output.");

        // Nothing within the largest window: no message.
        let path_far = temp_transcript("far");
        write(
            &path_far,
            &[
                assistant("m1", "2026-10-10T17:30:00.000Z", text("Too far back.")),
                json!({"type": "user", "message": {"content": "z".repeat(5 * 1024 * 1024)}}),
            ],
        );
        assert!(latest
            .latest(&query(&path_far), LatestMessageForm::Full, NOW)
            .is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        fs::remove_dir_all(path_far.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_unchanged_transcript_is_served_from_the_cache_and_growth_is_read_incrementally() {
        let path = temp_transcript("cache");
        write(
            &path,
            &[assistant("m1", "2026-10-10T17:30:00.000Z", text("First."))],
        );
        let mut latest = LatestMessages::default();
        let first = latest.latest(&query(&path), LatestMessageForm::Glance, NOW);
        assert_eq!(first.unwrap().text, "First.");
        assert_eq!(latest.transcript_reads, 1);
        for _ in 0..3 {
            latest.latest(&query(&path), LatestMessageForm::Glance, NOW);
        }
        assert_eq!(latest.transcript_reads, 1);

        // Tool output appended: re-read, same message.
        append(
            &path,
            &format!(
                "{}\n",
                json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "ok"}]}})
            ),
        );
        let same = latest.latest(&query(&path), LatestMessageForm::Glance, NOW);
        assert_eq!(same.unwrap().text, "First.");
        assert_eq!(latest.transcript_reads, 2);

        // A newer message appended.
        append(
            &path,
            &format!(
                "{}\n",
                assistant("m2", "2026-10-10T17:40:00.000Z", text("Second."))
            ),
        );
        let newer = latest
            .latest(&query(&path), LatestMessageForm::Glance, NOW)
            .unwrap();
        assert_eq!(newer.text, "Second.");
        assert_eq!(newer.at, 1_791_654_000_000);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_newer_of_a_hook_message_and_the_transcript_wins_and_final_follows_the_turn() {
        let path = temp_transcript("merge");
        write(
            &path,
            &[assistant(
                "m1",
                "2026-10-10T17:30:00.000Z",
                text("Done. Should I push?"),
            )],
        );
        let written = 1_791_653_400_000;
        let mut latest = LatestMessages::default();
        let mut query = query(&path);

        // The turn is still open: a transcript message is not final.
        query.turn_started_at = Some(written - 60_000);
        let message = latest
            .latest(&query, LatestMessageForm::Glance, NOW)
            .unwrap();
        assert!(!message.is_final);

        // The turn ended right after it: final (also after a Runtime restart,
        // when only the transcript is left).
        query.turn_ended_at = Some(written + 1_000);
        assert!(
            latest
                .latest(&query, LatestMessageForm::Glance, NOW)
                .unwrap()
                .is_final
        );

        // The Stop Hook reported the same message a moment later: final.
        latest.observe("session", "Done. Should I push?", written + 1_000, true);
        let message = latest
            .latest(&query, LatestMessageForm::Glance, NOW)
            .unwrap();
        assert_eq!((message.at, message.is_final), (written + 1_000, true));

        // A new turn writes a newer message: the transcript wins again.
        append(
            &path,
            &format!(
                "{}\n",
                assistant(
                    "m2",
                    "2026-10-10T17:45:00.000Z",
                    text("Looking at the logs now.")
                )
            ),
        );
        query.turn_started_at = Some(written + 600_000);
        query.turn_ended_at = None;
        let message = latest
            .latest(&query, LatestMessageForm::Glance, NOW)
            .unwrap();
        assert_eq!(message.text, "Looking at the logs now.");
        assert!(!message.is_final);

        // An older observation never replaces a newer one, and a message
        // older than a day is not reported.
        latest.observe("other", "Newer.", 2_000, false);
        latest.observe("other", "Older.", 1_000, true);
        let other = LatestMessageQuery {
            session_id: "other".to_owned(),
            transcript: None,
            turn_started_at: None,
            turn_ended_at: None,
        };
        assert_eq!(
            latest
                .latest(&other, LatestMessageForm::Glance, 3_000)
                .unwrap()
                .text,
            "Newer."
        );
        assert!(latest
            .latest(
                &other,
                LatestMessageForm::Glance,
                2_000 + LATEST_MESSAGE_TTL_MS + 1
            )
            .is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn unsafe_transcript_files_are_not_read() {
        let path = temp_transcript("unsafe");
        write(
            &path,
            &[assistant("m1", "2026-10-10T17:30:00.000Z", text("Hello."))],
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        let mut latest = LatestMessages::default();
        assert!(latest
            .latest(&query(&path), LatestMessageForm::Glance, NOW)
            .is_none());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let link = path.with_file_name("link.jsonl");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(latest
            .latest(&query(&link), LatestMessageForm::Glance, NOW)
            .is_none());
        assert!(latest
            .latest(
                &query(&path.with_extension("txt")),
                LatestMessageForm::Glance,
                NOW
            )
            .is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
