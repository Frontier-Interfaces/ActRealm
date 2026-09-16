//! Transient question projection. Never writes question text or replies to disk.
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::PathBuf;
use uuid::Uuid;

const MAX_LINE: usize = 128 * 1024;
const SCAN_BUDGET: u64 = 2 * 1024 * 1024;
const QUESTION_TTL: u64 = 60 * 60 * 1000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Question {
    pub title: String,
    pub options: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum ReplyRoute {
    Observe,
    Steer {
        turn_id: String,
    },
    Rpc {
        id: Value,
        question_ids: Vec<String>,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuestionBatch {
    pub id: Uuid,
    pub session_id: String,
    pub created_at: u64,
    pub can_answer: bool,
    pub questions: Vec<Question>,
    #[serde(skip)]
    pub thread_id: String,
    #[serde(skip)]
    pub item_id: String,
    #[serde(skip)]
    pub route: ReplyRoute,
    #[serde(skip)]
    pub submitting: bool,
}

#[derive(Default)]
pub(crate) struct QuestionRegistry {
    entries: HashMap<Uuid, QuestionBatch>,
    cleared_at: HashMap<String, u64>,
}

impl QuestionRegistry {
    pub fn observe(&mut self, mut batch: QuestionBatch) {
        if self
            .cleared_at
            .get(&batch.thread_id)
            .is_some_and(|at| *at >= batch.created_at)
        {
            return;
        }
        if let Some(existing) = self.entries.values_mut().find(|old| {
            old.thread_id == batch.thread_id
                && (old.item_id == batch.item_id || old.questions == batch.questions)
        }) {
            // A transcript echo cannot remove an already verified reply route.
            if !existing.submitting
                && matches!(existing.route, ReplyRoute::Observe)
                && batch.can_answer
            {
                existing.route = batch.route;
                existing.can_answer = true;
            }
            return;
        }
        if self.entries.len() >= 128 {
            return;
        }
        batch.id = Uuid::now_v7();
        self.entries.insert(batch.id, batch);
    }

    pub fn clear_thread(&mut self, thread: &str, at: u64) {
        self.entries
            .retain(|_, q| q.thread_id != thread || q.created_at > at);
        self.cleared_at
            .entry(thread.to_owned())
            .and_modify(|v| *v = (*v).max(at))
            .or_insert(at);
    }

    pub fn resolve_rpc(&mut self, rpc: &Value) {
        self.entries
            .retain(|_, q| !matches!(&q.route, ReplyRoute::Rpc { id, .. } if id == rpc));
    }

    pub fn end_turn(&mut self, thread: &str) {
        for q in self.entries.values_mut().filter(|q| q.thread_id == thread) {
            q.can_answer = false;
            q.route = ReplyRoute::Observe;
        }
    }

    pub fn snapshot(&mut self, now: u64) -> Vec<QuestionBatch> {
        self.entries
            .retain(|_, q| now.saturating_sub(q.created_at) <= QUESTION_TTL);
        self.cleared_at
            .retain(|_, at| now.saturating_sub(*at) <= QUESTION_TTL);
        let mut entries: Vec<_> = self.entries.values().cloned().collect();
        entries.sort_by_key(|q| (q.created_at, q.id));
        entries
    }

    pub fn claim(&mut self, id: Uuid, now: u64) -> Option<QuestionBatch> {
        self.snapshot(now);
        let q = self.entries.get_mut(&id)?;
        if !q.can_answer || q.submitting {
            return None;
        }
        q.submitting = true;
        Some(q.clone())
    }

    pub fn finish(&mut self, id: Uuid, succeeded: bool) {
        if succeeded {
            if let Some(q) = self.entries.remove(&id) {
                self.clear_thread(&q.thread_id, q.created_at);
            }
        } else if let Some(q) = self.entries.get_mut(&id) {
            // A failed transport may have sent the answer: never offer automatic replay.
            q.can_answer = false;
            q.route = ReplyRoute::Observe;
            q.submitting = true;
        }
    }
}

pub(crate) fn questions(value: &Value, rpc: bool) -> Option<Vec<Question>> {
    let list = value
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 20)?;
    list.iter()
        .map(|q| {
            if q.get("isSecret").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            let title = text(q.get(if rpc { "question" } else { "title" })?, 2000)?;
            let options = match q.get("options").filter(|v| !v.is_null()) {
                Some(options) => options
                    .as_array()
                    .filter(|o| o.len() <= 50)?
                    .iter()
                    .map(|o| text(if rpc { o.get("label")? } else { o }, 500))
                    .collect::<Option<Vec<_>>>()?,
                None => Vec::new(),
            };
            Some(Question { title, options })
        })
        .collect()
}

fn text(v: &Value, max: usize) -> Option<String> {
    let s = v.as_str()?.trim();
    (!s.is_empty() && s.chars().count() <= max && !s.contains('\0')).then(|| s.to_owned())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObservedActivity {
    pub event_id: String,
    pub kind: String,
    pub tool_name: String,
    pub tool_call_id: String,
    pub status: String,
    pub occurred_at: u64,
    pub source: &'static str,
    #[serde(skip)]
    pub session_id: String,
    #[serde(skip)]
    pub started_at: u64,
}

pub(crate) enum Observation {
    TurnEnded {
        thread_id: String,
        turn_id: String,
        event: &'static str,
        at: u64,
    },
    Activity(ObservedActivity),
    Question(QuestionBatch),
    Clear {
        thread_id: String,
        at: u64,
    },
    Result {
        session_id: String,
        text: String,
        cwd: Option<PathBuf>,
        at: u64,
    },
}

#[derive(Default)]
struct Cursor {
    cwd: Option<PathBuf>,
    identity: (u64, u64),
    offset: u64,
    skipping: bool,
    pending: HashMap<String, QuestionBatch>,
    tools: HashMap<String, (String, u64)>,
}

#[derive(Default)]
pub(crate) struct QuestionScanner {
    cursors: HashMap<PathBuf, Cursor>,
    next: usize,
}

impl QuestionScanner {
    /// Only scans files belonging to currently known tasks. A bounded tail is
    /// used on first observation; old prompts and copied parent history are ignored.
    pub fn poll(
        &mut self,
        files: &[PathBuf],
        sessions: &HashMap<String, String>,
        now: u64,
    ) -> Vec<Observation> {
        let candidates: Vec<_> = files
            .iter()
            .filter_map(|path| {
                let name = path.file_stem()?.to_str()?;
                sessions
                    .iter()
                    .find(|(thread, _)| {
                        name.ends_with(&format!("-{thread}"))
                            || name.contains(&format!("-{thread}_"))
                    })
                    .map(|(thread, session)| (path, thread, session))
            })
            .collect();
        let active: HashSet<_> = candidates.iter().map(|(p, _, _)| (*p).clone()).collect();
        self.cursors.retain(|p, _| active.contains(p));
        if candidates.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut budget = SCAN_BUDGET;
        for delta in 0..candidates.len() {
            if budget < MAX_LINE as u64 {
                break;
            }
            let (path, thread, session) = candidates[(self.next + delta) % candidates.len()];
            let Ok(mut file) = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)
            else {
                continue;
            };
            let Ok(meta) = file.metadata() else {
                continue;
            };
            if !meta.is_file()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.mode() & 0o022 != 0
            {
                continue;
            }
            let identity = (meta.dev(), meta.ino());
            let cursor = self.cursors.entry(path.clone()).or_default();
            if cursor.identity != identity || cursor.offset > meta.len() {
                // Validate the canonical session identity, not only the file name.
                let mut first = String::new();
                if BufReader::new(&mut file)
                    .take(MAX_LINE as u64)
                    .read_line(&mut first)
                    .is_err()
                {
                    continue;
                }
                let Ok(root) = serde_json::from_str::<Value>(&first) else {
                    continue;
                };
                if root.get("type").and_then(Value::as_str) != Some("session_meta")
                    || root.pointer("/payload/id").and_then(Value::as_str) != Some(thread.as_str())
                {
                    continue;
                }
                *cursor = Cursor {
                    cwd: root
                        .pointer("/payload/cwd")
                        .and_then(Value::as_str)
                        .map(PathBuf::from),
                    identity,
                    offset: meta.len().saturating_sub(512 * 1024),
                    ..Cursor::default()
                };
                cursor.skipping = cursor.offset != 0;
            }
            if cursor.offset == meta.len() {
                continue;
            }
            if file.seek(SeekFrom::Start(cursor.offset)).is_err() {
                continue;
            }
            let mut reader = BufReader::new(file);
            let mut line = Vec::new();
            while budget >= MAX_LINE as u64 {
                line.clear();
                let Ok(n) = reader
                    .by_ref()
                    .take(MAX_LINE as u64)
                    .read_until(b'\n', &mut line)
                else {
                    break;
                };
                if n == 0 {
                    break;
                }
                let complete = line.last() == Some(&b'\n');
                if !complete && n < MAX_LINE {
                    break;
                } // retry a partially written record
                cursor.offset += n as u64;
                budget = budget.saturating_sub(n as u64);
                if cursor.skipping {
                    cursor.skipping = !complete;
                    continue;
                }
                if !complete {
                    cursor.skipping = true;
                    continue;
                }
                let Ok(root) = serde_json::from_slice::<Value>(&line) else {
                    continue;
                };
                let Some(at) = root
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(actrealm_usage::timestamp_millis)
                else {
                    continue;
                };
                if at > now.saturating_add(5000) {
                    continue;
                }
                Self::record_lifecycle(&root, thread, at, &mut out);
                if now.saturating_sub(at) > QUESTION_TTL {
                    if now.saturating_sub(at) <= 24 * 60 * 60 * 1000 {
                        Self::record_result(cursor, &root, session, at, &mut out);
                    }
                    continue;
                }
                Self::record(cursor, &root, thread, session, at, &mut out);
            }
        }
        self.next = (self.next + 1) % candidates.len();
        out
    }

    fn record_lifecycle(root: &Value, thread: &str, at: u64, out: &mut Vec<Observation>) {
        if root["type"] != "event_msg" {
            return;
        }
        let payload = &root["payload"];
        let Some(turn_id) = payload["turn_id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 160)
        else {
            return;
        };
        let event = match payload["type"].as_str() {
            Some("task_complete") if !payload["error"].is_null() => "StopFailure",
            Some("task_complete") => "Stop",
            Some("turn_aborted" | "task_aborted") => "TurnInterrupted",
            _ => return,
        };
        out.push(Observation::TurnEnded {
            thread_id: thread.to_owned(),
            turn_id: turn_id.to_owned(),
            event,
            at,
        });
    }

    fn record_result(
        cursor: &Cursor,
        root: &Value,
        session: &str,
        at: u64,
        out: &mut Vec<Observation>,
    ) {
        let payload = &root["payload"];
        let kind = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if root["type"] == "response_item"
            && kind == "message"
            && payload["role"] == "assistant"
            && (payload["channel"] == "final" || payload["phase"] == "final_answer")
        {
            let text = payload["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                out.push(Observation::Result {
                    session_id: session.to_owned(),
                    text,
                    cwd: cursor.cwd.clone(),
                    at,
                });
            }
        }
    }

    fn record(
        cursor: &mut Cursor,
        root: &Value,
        thread: &str,
        session: &str,
        at: u64,
        out: &mut Vec<Observation>,
    ) {
        let payload = &root["payload"];
        let kind = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if (root["type"] == "event_msg" && kind == "user_message")
            || (kind == "message" && payload["role"] == "user")
        {
            cursor.pending.clear();
            out.push(Observation::Clear {
                thread_id: thread.to_owned(),
                at,
            });
        }
        Self::record_result(cursor, root, session, at, out);
        if root["type"] == "response_item" {
            if matches!(kind, "function_call" | "custom_tool_call") {
                if let (Some(call), Some(name)) =
                    (payload["call_id"].as_str(), payload["name"].as_str())
                {
                    if call.len() <= 256
                        && name.len() <= 128
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
                        && ![
                            "request_user_input_async",
                            "functions.request_user_input_async",
                        ]
                        .contains(&name)
                    {
                        let name = name.strip_prefix("functions.").unwrap_or(name).to_owned();
                        if cursor.tools.len() >= 64 {
                            if let Some(oldest) = cursor
                                .tools
                                .iter()
                                .min_by_key(|(_, (_, time))| *time)
                                .map(|(id, _)| id.clone())
                            {
                                cursor.tools.remove(&oldest);
                            }
                        }
                        cursor.tools.insert(call.to_owned(), (name.clone(), at));
                        out.push(Observation::Activity(ObservedActivity {
                            event_id: format!("native:{call}:started"),
                            kind: "tool.started".into(),
                            tool_name: name,
                            tool_call_id: call.to_owned(),
                            status: "started".into(),
                            occurred_at: at,
                            source: "codex:tool_event",
                            session_id: session.to_owned(),
                            started_at: at,
                        }));
                    }
                }
            } else if matches!(kind, "function_call_output" | "custom_tool_call_output") {
                if let Some(call) = payload["call_id"].as_str() {
                    if let Some((name, started_at)) = cursor.tools.remove(call) {
                        // Receipt of a tool response is not proof of successful validation.
                        out.push(Observation::Activity(ObservedActivity {
                            event_id: format!("native:{call}:completed"),
                            kind: "tool.completed".into(),
                            tool_name: name,
                            tool_call_id: call.to_owned(),
                            status: "completed".into(),
                            occurred_at: at,
                            source: "codex:tool_event",
                            session_id: session.to_owned(),
                            started_at,
                        }));
                    }
                }
            }
        }
        if kind == "function_call"
            && matches!(
                payload.get("name").and_then(Value::as_str),
                Some("request_user_input_async" | "functions.request_user_input_async")
            )
        {
            let Some(id) = payload
                .get("call_id")
                .and_then(Value::as_str)
                .filter(|s| s.len() <= 256)
            else {
                return;
            };
            let Some(args) = payload
                .get("arguments")
                .and_then(Value::as_str)
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
            else {
                return;
            };
            let Some(questions) = questions(&args["questions"], false) else {
                return;
            };
            if cursor.pending.len() >= 20 {
                return;
            }
            cursor.pending.insert(
                id.to_owned(),
                QuestionBatch {
                    id: Uuid::nil(),
                    session_id: session.to_owned(),
                    thread_id: thread.to_owned(),
                    item_id: id.to_owned(),
                    created_at: at,
                    can_answer: false,
                    questions,
                    route: ReplyRoute::Observe,
                    submitting: false,
                },
            );
        }
        if kind == "function_call_output" {
            let Some(id) = payload.get("call_id").and_then(Value::as_str) else {
                return;
            };
            let Some(question) = cursor.pending.remove(id) else {
                return;
            };
            let accepted = payload
                .get("output")
                .and_then(Value::as_str)
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .is_some_and(|v| v["accepted"] == true);
            if accepted {
                out.push(Observation::Question(question));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn batch(at: u64) -> QuestionBatch {
        QuestionBatch {
            id: Uuid::nil(),
            session_id: "session".into(),
            thread_id: "thread".into(),
            item_id: "call-1".into(),
            created_at: at,
            can_answer: false,
            questions: vec![Question {
                title: "Choose a layout".into(),
                options: vec!["Wide".into()],
            }],
            route: ReplyRoute::Observe,
            submitting: false,
        }
    }

    #[test]
    fn native_tool_activity_keeps_only_metadata_and_never_claims_test_success() {
        let mut cursor = Cursor::default();
        let mut out = Vec::new();
        QuestionScanner::record(
            &mut cursor,
            &json!({"type":"response_item","payload":{"type":"function_call","name":"functions.exec","call_id":"call-real","arguments":"PRIVATE_COMMAND"}}),
            "thread",
            "session",
            100,
            &mut out,
        );
        QuestionScanner::record(
            &mut cursor,
            &json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"call-real","output":"PRIVATE_OUTPUT"}}),
            "thread",
            "session",
            200,
            &mut out,
        );
        let activities = out
            .iter()
            .filter_map(|o| {
                if let Observation::Activity(a) = o {
                    Some(a)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(activities.len(), 2);
        assert_eq!(activities[0].tool_name, "exec");
        assert_eq!(activities[1].kind, "tool.completed");
        assert_eq!(activities[1].started_at, 100);
        let wire = serde_json::to_string(&activities).unwrap();
        assert!(!wire.contains("PRIVATE"));
        assert!(!wire.contains("passed"));
    }

    #[test]
    fn native_final_replies_are_observed_without_treating_commentary_as_results() {
        let mut cursor = Cursor::default();
        let mut observations = Vec::new();
        let mut message = json!({"type":"response_item","payload":{"type":"message","role":"assistant","channel":"commentary","content":[{"type":"output_text","text":"Still working"}]}});
        QuestionScanner::record(
            &mut cursor,
            &message,
            "thread",
            "session",
            100,
            &mut observations,
        );
        assert!(observations.is_empty());
        message["payload"]["phase"] = json!("final_answer");
        message["payload"]["content"][0]["text"] = json!("计算结果：15129");
        QuestionScanner::record(
            &mut cursor,
            &message,
            "thread",
            "session",
            200,
            &mut observations,
        );
        assert!(
            matches!(&observations[0], Observation::Result { session_id, text, at: 200, .. } if session_id == "session" && text.contains("15129"))
        );
    }

    #[test]
    fn echoes_do_not_resurrect_answered_questions_or_downgrade_routes() {
        let mut registry = QuestionRegistry::default();
        registry.observe(batch(100));
        let id = registry.snapshot(110)[0].id;
        let mut managed = batch(100);
        managed.can_answer = true;
        managed.route = ReplyRoute::Steer {
            turn_id: "turn".into(),
        };
        registry.observe(managed);
        registry.observe(batch(100));
        assert_eq!(registry.snapshot(110).len(), 1);
        assert!(registry.claim(id, 110).is_some());
        assert!(registry.claim(id, 110).is_none());
        registry.finish(id, true);
        registry.observe(batch(100));
        assert!(registry.snapshot(120).is_empty());
        registry.observe(batch(130));
        assert_eq!(registry.snapshot(140).len(), 1);
    }

    #[test]
    fn observation_cannot_be_answered_and_closed_turn_cannot_be_steered() {
        let mut registry = QuestionRegistry::default();
        registry.observe(batch(100));
        let id = registry.snapshot(101)[0].id;
        assert!(registry.claim(id, 102).is_none());
        let mut live = batch(100);
        live.can_answer = true;
        live.route = ReplyRoute::Steer {
            turn_id: "turn".into(),
        };
        registry.observe(live);
        registry.end_turn("thread");
        assert!(registry.claim(id, 103).is_none());
        assert!(registry.snapshot(QUESTION_TTL + 101).is_empty());
    }

    #[test]
    fn failed_send_cannot_be_retried_by_a_stale_client() {
        let mut registry = QuestionRegistry::default();
        let mut live = batch(100);
        live.can_answer = true;
        live.route = ReplyRoute::Rpc {
            id: json!(10),
            question_ids: vec!["choice".into()],
        };
        registry.observe(live);
        let id = registry.snapshot(101)[0].id;
        assert!(registry.claim(id, 102).is_some());
        registry.finish(id, false);
        assert!(registry.claim(id, 103).is_none());
    }

    #[test]
    fn native_question_requires_accepted_tool_result_and_clears_on_input() {
        let mut cursor = Cursor::default();
        let mut output = Vec::new();
        let call = json!({"type":"response_item","payload":{
            "type":"function_call","name":"request_user_input_async","call_id":"q1",
            "arguments":json!({"questions":[{"title":"Which layout?","options":["Wide","Compact"]}]}).to_string()
        }});
        QuestionScanner::record(&mut cursor, &call, "thread", "session", 100, &mut output);
        assert!(output.is_empty());
        let result = json!({"type":"response_item","payload":{
            "type":"function_call_output","call_id":"q1","output":"{\"accepted\":true}"
        }});
        QuestionScanner::record(&mut cursor, &result, "thread", "session", 110, &mut output);
        assert!(
            matches!(&output[0], Observation::Question(q) if !q.can_answer && q.questions[0].options.len() == 2)
        );
        QuestionScanner::record(
            &mut cursor,
            &json!({"type":"event_msg","payload":{"type":"user_message"}}),
            "thread",
            "session",
            120,
            &mut output,
        );
        assert!(matches!(&output[1], Observation::Clear { at: 120, .. }));
        assert!(questions(&json!([{"question":"secret", "isSecret":true}]), true).is_none());
    }

    #[test]
    fn partial_lines_are_retried_and_unchanged_files_are_not_replayed() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("actrealm-question-tail-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("rollout-test-thread.jsonl");
        let timestamp = "2026-09-07T12:00:00Z";
        let now = actrealm_usage::timestamp_millis(timestamp).unwrap();
        let call = json!({"timestamp":timestamp,"type":"response_item","payload":{
            "type":"function_call","name":"request_user_input_async","call_id":"q1",
            "arguments":"{\"questions\":[{\"title\":\"Choose?\"}]}"
        }});
        let result = json!({"timestamp":timestamp,"type":"response_item","payload":{
            "type":"function_call_output","call_id":"q1","output":"{\"accepted\":true}"
        }});
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n{}",
                json!({"type":"session_meta","payload":{"id":"thread"}}),
                call,
                result
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut scanner = QuestionScanner::default();
        let sessions = HashMap::from([("thread".to_owned(), "session".to_owned())]);
        assert!(scanner
            .poll(std::slice::from_ref(&path), &sessions, now)
            .is_empty());
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        assert_eq!(
            scanner
                .poll(std::slice::from_ref(&path), &sessions, now)
                .len(),
            1
        );
        assert!(scanner
            .poll(std::slice::from_ref(&path), &sessions, now)
            .is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn failed_compaction_and_cancel_are_terminal_without_copying_error_text() {
        let mut out = Vec::new();
        QuestionScanner::record_lifecycle(
            &json!({"type":"event_msg","payload":{
                "type":"task_complete","turn_id":"turn","error":{"message":"PRIVATE ERROR"}
            }}),
            "thread",
            1000,
            &mut out,
        );
        assert!(
            matches!(&out[0], Observation::TurnEnded { thread_id, turn_id, event: "StopFailure", at: 1000 } if thread_id == "thread" && turn_id == "turn")
        );
        QuestionScanner::record_lifecycle(
            &json!({"type":"event_msg","payload":{
                "type":"turn_aborted","turn_id":"turn"
            }}),
            "thread",
            2000,
            &mut out,
        );
        assert!(matches!(
            &out[1],
            Observation::TurnEnded {
                event: "TurnInterrupted",
                ..
            }
        ));
        QuestionScanner::record_lifecycle(
            &json!({"type":"event_msg","payload":{"type":"task_complete"}}),
            "thread",
            3000,
            &mut out,
        );
        assert_eq!(out.len(), 2);
    }
}
