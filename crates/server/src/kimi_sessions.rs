//! Existing Kimi desktop/web server sessions. Never starts a model, loads a
//! dormant session, stores a bearer token, or posts without a live user reply.
use actrealm_core::{
    BlockingRequestKind, BridgeRequest, BridgeResponse, Provider, ReplyAction, ReplyPayload,
};
use actrealm_runtime::{RuntimeStore, WaiterRegistry, WaiterTicket};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LIMIT: usize = 2 * 1024 * 1024;
struct Question {
    id: String,
    options: Vec<(String, String)>,
    multi: bool,
    other: bool,
}
enum Route {
    Approval,
    Questions(Vec<Question>),
}
struct Pending {
    endpoint: String,
    path: String,
    route: Route,
    ticket: WaiterTicket,
    expires: u64,
}

pub fn start(
    home: PathBuf,
    runtime_home: PathBuf,
    store: RuntimeStore,
    waiters: WaiterRegistry,
    shutdown: Arc<AtomicBool>,
) -> io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("actrealm-kimi-sessions".into())
        .spawn(move || {
            let Ok(client) = Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
            else {
                return;
            };
            let mut pending = HashMap::<String, Pending>::new();
            let mut handled = HashSet::new();
            while !shutdown.load(Ordering::Acquire) {
                let installed = private_json(&runtime_home.join("providers/kimi-hooks.json"), 8192)
                    .is_ok_and(|v| v["installed"] == true);
                let mut seen = HashSet::new();
                if installed {
                    if let Ok(token) = private_bytes(&home.join(".kimi-code/server.token"), 8192) {
                        if let Ok(token) = std::str::from_utf8(&token) {
                            for (instance, endpoint) in endpoints(&home) {
                                if shutdown.load(Ordering::Acquire) {
                                    break;
                                }
                                let _ = poll(
                                    &client,
                                    &endpoint,
                                    &instance,
                                    token.trim(),
                                    &store,
                                    &waiters,
                                    &mut pending,
                                    &mut seen,
                                    &handled,
                                );
                            }
                            // Only request ids returned in this fresh poll may be
                            // submitted. Original-client answers win at the API too.
                            for (key, p) in &pending {
                                if !seen.contains(key) || now() >= p.expires {
                                    continue;
                                }
                                if let Ok(reply) = p.ticket.recv_timeout(Duration::ZERO) {
                                    if let Some((suffix, body)) = p.route.reply(&reply) {
                                        let result = http(
                                            &client,
                                            &p.endpoint,
                                            token.trim(),
                                            &(p.path.clone() + suffix),
                                            Some(body),
                                        );
                                        if result.is_err() {
                                            let _ = store.expire_approval(
                                                p.ticket.request_id(),
                                                "connector_response_failed",
                                                now(),
                                            );
                                        }
                                    }
                                    handled.insert(key.clone());
                                }
                            }
                        }
                    }
                }
                let remove = pending
                    .iter()
                    .filter(|(key, p)| {
                        !seen.contains(*key) || handled.contains(*key) || now() >= p.expires
                    })
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>();
                for key in remove {
                    if let Some(p) = pending.remove(&key) {
                        let id = p.ticket.request_id();
                        let _ = waiters.pass_through(id, "provider_handled");
                        if !handled.contains(&key) {
                            let _ = store.expire_approval(id, "provider_handled", now());
                        }
                    }
                }
                handled.retain(|key| seen.contains(key));
                for _ in 0..10 {
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
            for (_, p) in pending {
                let id = p.ticket.request_id();
                let _ = waiters.pass_through(id, "connector_disconnected");
                let _ = store.expire_approval(id, "connector_disconnected", now());
            }
        })
}

fn private_bytes(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    for parent in path.ancestors() {
        if fs::symlink_metadata(parent)?.file_type().is_symlink() {
            return Err(io::Error::other("Symlinked Kimi metadata"));
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o022 != 0
        || m.len() > limit as u64
    {
        return Err(io::Error::other("Untrusted Kimi metadata"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other("Kimi metadata too large"));
    }
    Ok(bytes)
}
fn private_json(path: &Path, limit: usize) -> io::Result<Value> {
    Ok(serde_json::from_slice(&private_bytes(path, limit)?)?)
}
fn endpoints(home: &Path) -> Vec<(String, String)> {
    let Ok(files) = fs::read_dir(home.join(".kimi-code/server/instances")) else {
        return vec![];
    };
    files
        .flatten()
        .take(16)
        .filter_map(|f| {
            let v = private_json(&f.path(), 8192).ok()?;
            if v["host"] != "127.0.0.1"
                || now().saturating_sub(v["heartbeat_at"].as_u64()?) > 30_000
            {
                return None;
            }
            let pid = i32::try_from(v["pid"].as_u64()?).ok()?;
            if pid <= 0 || unsafe { libc::kill(pid, 0) } != 0 {
                return None;
            }
            let port = u16::try_from(v["port"].as_u64()?).ok()?;
            if port == 0 {
                return None;
            }
            Some((
                id(&v["server_id"])?.into(),
                format!("http://127.0.0.1:{port}"),
            ))
        })
        .take(4)
        .collect()
}
fn http(
    client: &Client,
    endpoint: &str,
    token: &str,
    path: &str,
    body: Option<Value>,
) -> io::Result<Value> {
    let request = if let Some(body) = body {
        client
            .post(format!("{endpoint}{path}"))
            .header("content-type", "application/json")
            .body(body.to_string())
    } else {
        client.get(format!("{endpoint}{path}"))
    };
    let response = request
        .bearer_auth(token)
        .send()
        .map_err(|_| io::Error::other("Kimi request failed"))?;
    if !response.status().is_success() {
        return Err(io::Error::other("Kimi rejected request"));
    }
    let mut bytes = Vec::new();
    response.take(LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err(io::Error::other("Kimi response too large"));
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    if !matches!(value["code"].as_u64(), Some(0))
        && !(path.ends_with(":dismiss") && value["code"] == 40909)
    {
        return Err(io::Error::other("Kimi interaction unavailable"));
    }
    Ok(value["data"].clone())
}

#[allow(clippy::too_many_arguments)]
fn poll(
    client: &Client,
    endpoint: &str,
    instance: &str,
    token: &str,
    store: &RuntimeStore,
    waiters: &WaiterRegistry,
    pending: &mut HashMap<String, Pending>,
    seen: &mut HashSet<String>,
    handled: &HashSet<String>,
) -> io::Result<()> {
    let meta = http(client, endpoint, token, "/api/v1/meta", None)?;
    if meta["backend"] != "v2" || id(&meta["server_id"]).is_none() {
        return Err(io::Error::other("Unsupported Kimi server"));
    }
    let rows = http(
        client,
        endpoint,
        token,
        "/api/v1/sessions?busy=true&page_size=100",
        None,
    )?;
    let Some(rows) = rows["items"].as_array() else {
        return Err(io::Error::other("Invalid Kimi sessions"));
    };
    for session in rows.iter().filter(|s| s["busy"] == true).take(32) {
        let (Some(sid), Some(cwd)) = (id(&session["id"]), session["metadata"]["cwd"].as_str())
        else {
            continue;
        };
        if !Path::new(cwd).is_absolute() || cwd.len() > 4096 {
            continue;
        }
        let kind = match session["pending_interaction"].as_str() {
            Some("question") => "questions",
            Some("approval") => "approvals",
            _ => continue,
        };
        let path = format!("/api/v1/sessions/{sid}/{kind}");
        let items = http(
            client,
            endpoint,
            token,
            &format!("{path}?status=pending"),
            None,
        )?;
        let Some(items) = items["items"].as_array() else {
            continue;
        };
        for item in items.iter().take(16) {
            let Some(rid) = id(&item[if kind == "questions" {
                "question_id"
            } else {
                "approval_id"
            }]) else {
                continue;
            };
            if item["session_id"] != sid {
                continue;
            }
            let key = format!("{instance}:{}:{sid}:{rid}", meta["server_id"]);
            seen.insert(key.clone());
            if pending.contains_key(&key) || handled.contains(&key) || pending.len() >= 32 {
                continue;
            }
            let Some((mut request, route)) = parse(item, sid, cwd, kind, now()) else {
                continue;
            };
            request.raw["source_version"] = json!("kimi-server/v1");
            let registration = waiters
                .register_at(&request, now())
                .map_err(io::Error::other)?;
            match store.ingest(request.clone()) {
                Ok(result) if !result.suppressed => {}
                _ => {
                    let _ = waiters.pass_through(registration.ticket.request_id(), "runtime_error");
                    continue;
                }
            }
            pending.insert(
                key,
                Pending {
                    endpoint: endpoint.into(),
                    path: format!("{path}/{rid}"),
                    route,
                    ticket: registration.ticket,
                    expires: request.deadline_at.unwrap_or(now()),
                },
            );
        }
    }
    Ok(())
}

fn parse(
    item: &Value,
    session: &str,
    cwd: &str,
    kind: &str,
    at: u64,
) -> Option<(BridgeRequest, Route)> {
    let (blocking, raw, route) = if kind == "questions" {
        let rows = item["questions"]
            .as_array()
            .filter(|q| !q.is_empty() && q.len() <= 4)?;
        let mut questions = Vec::new();
        let mut mapping = Vec::new();
        let mut ids = HashSet::new();
        for row in rows {
            let qid = id(&row["id"])?;
            if !ids.insert(qid) {
                return None;
            }
            let text = row["question"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 2000)?;
            let multi = row["multi_select"].as_bool().unwrap_or(false);
            let other = row["allow_other"].as_bool().unwrap_or(false);
            let mut options = Vec::new();
            let mut labels = HashSet::new();
            let mut option_ids = HashSet::new();
            for option in row["options"].as_array()?.iter().take(50) {
                let oid = id(&option["id"])?;
                let label = option["label"].as_str().filter(|s| s.len() <= 500)?;
                if !labels.insert(label) || !option_ids.insert(oid) {
                    return None;
                }
                options.push((label.to_owned(), oid.to_owned()));
            }
            questions.push(json!({"id":qid,"header":row["header"],"question":text,"options":row["options"],"multiSelect":multi,"isOther":other}));
            mapping.push(Question {
                id: qid.into(),
                options,
                multi,
                other,
            });
        }
        (
            BlockingRequestKind::AgentQuestion,
            json!({"tool_name":"AskUserQuestion","questions":questions}),
            Route::Questions(mapping),
        )
    } else {
        let name = item["tool_name"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)?;
        let input = match item.get("tool_input_display") {
            Some(Value::Object(o)) => Value::Object(o.clone()),
            Some(Value::String(s)) if s.len() <= 8192 => json!({"description":s}),
            _ => json!({}),
        };
        (
            BlockingRequestKind::Permission,
            json!({"tool_name":name,"tool_input":input}),
            Route::Approval,
        )
    };
    let expires = item["expires_at"]
        .as_u64()
        .unwrap_or(at + 60 * 60 * 1000)
        .min(at + 24 * 60 * 60 * 1000);
    if expires <= at {
        return None;
    }
    let mut request = BridgeRequest::from_agent_request_at(
        Provider::Kimi,
        session,
        item["turn_id"].as_str(),
        blocking,
        raw,
        at,
        expires,
    )?;
    request.raw["cwd"] = json!(cwd);
    request.raw["tool_call_id"] = item["tool_call_id"].clone();
    Some((request, route))
}
impl Route {
    fn reply(&self, reply: &BridgeResponse) -> Option<(&'static str, Value)> {
        match self {
            Self::Approval => Some((
                "",
                json!({"decision":match reply.action{ReplyAction::Allow=>"approved",ReplyAction::Deny=>"rejected",_=>return None}}),
            )),
            Self::Questions(questions) => {
                let Some(ReplyPayload::AgentQuestion { answers }) = &reply.payload else {
                    return None;
                };
                if answers.is_empty() {
                    return Some((":dismiss", json!({})));
                }
                if answers.len() != questions.len() {
                    return None;
                }
                let mut result = serde_json::Map::new();
                for question in questions {
                    let selected = answers.get(&question.id)?;
                    if selected.is_empty() || (!question.multi && selected.len() != 1) {
                        return None;
                    }
                    let mut known = Vec::new();
                    let mut other = Vec::new();
                    for answer in selected {
                        if let Some((_, id)) =
                            question.options.iter().find(|(label, _)| label == answer)
                        {
                            known.push(id.clone());
                        } else if question.other {
                            other.push(answer.clone());
                        } else {
                            return None;
                        }
                    }
                    let value = if other.is_empty() {
                        if question.multi {
                            json!({"kind":"multi","option_ids":known})
                        } else {
                            json!({"kind":"single","option_id":known.first()?})
                        }
                    } else if known.is_empty() {
                        json!({"kind":"other","text":other.join("\n")})
                    } else {
                        json!({"kind":"multi_with_other","option_ids":known,"other_text":other.join("\n")})
                    };
                    result.insert(question.id.clone(), value);
                }
                Some(("", json!({"answers":result,"method":"click"})))
            }
        }
    }
}
fn id(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| {
        !s.is_empty()
            && s.len() <= 256
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    })
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use uuid::Uuid;

    fn item() -> Value {
        json!({"question_id":"question_1","session_id":"session_1","tool_call_id":"tool1","questions":[{"id":"q_0","question":"Colors?","options":[{"id":"opt_blue","label":"Blue"},{"id":"opt_green","label":"Green"}],"multi_select":true,"allow_other":true}]})
    }
    #[test]
    fn question_choices_keep_provider_ids_and_support_multiple_and_free_text() {
        let (request, route) = parse(&item(), "session_1", "/tmp", "questions", 1000).unwrap();
        let answers = BTreeMap::from([("q_0".into(), vec!["Blue".into(), "Custom".into()])]);
        let result = route
            .reply(&BridgeResponse::answered(
                request.id,
                ReplyPayload::AgentQuestion { answers },
            ))
            .unwrap();
        assert_eq!(
            result.1,
            json!({"answers":{"q_0":{"kind":"multi_with_other","option_ids":["opt_blue"],"other_text":"Custom"}},"method":"click"})
        );
        assert_eq!(
            route
                .reply(&BridgeResponse::answered(
                    request.id,
                    ReplyPayload::AgentQuestion {
                        answers: BTreeMap::new()
                    }
                ))
                .unwrap()
                .0,
            ":dismiss"
        );
        assert!(route
            .reply(&BridgeResponse::pass_through(
                request.id,
                "native_provider_ui"
            ))
            .is_none());
        let mut invalid = item();
        invalid["questions"][0]["options"][1]["label"] = json!("Blue");
        assert!(parse(&invalid, "session_1", "/tmp", "questions", 1000).is_none());
    }
    #[test]
    fn approval_never_expands_one_time_approval_to_session_scope() {
        let (request,route)=parse(&json!({"tool_name":"WriteFile","tool_input_display":{"path":"file"},"expires_at":3000}),"session_1","/tmp","approvals",1000).unwrap();
        let mut reply = BridgeResponse::pass_through(request.id, "test");
        reply.action = ReplyAction::Allow;
        assert_eq!(
            route.reply(&reply).unwrap().1,
            json!({"decision":"approved"})
        );
        reply.action = ReplyAction::Deny;
        assert_eq!(
            route.reply(&reply).unwrap().1,
            json!({"decision":"rejected"})
        );
        assert!(parse(
            &json!({"tool_name":"WriteFile","expires_at":500}),
            "s",
            "/tmp",
            "approvals",
            1000
        )
        .is_none());
    }
    #[test]
    fn local_server_pending_question_reaches_registry_and_receives_exact_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for (path, data) in [
                (
                    "/api/v1/meta",
                    json!({"backend":"v2","server_id":"engine1"}),
                ),
                (
                    "/api/v1/sessions?busy=true&page_size=100",
                    json!({"items":[{"id":"session_1","busy":true,"pending_interaction":"question","metadata":{"cwd":"/tmp"}}]}),
                ),
                (
                    "/api/v1/sessions/session_1/questions?status=pending",
                    json!({"items":[item()]}),
                ),
                (
                    "/api/v1/sessions/session_1/questions/question_1",
                    json!({"resolved":true}),
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                assert_eq!(first.split_whitespace().nth(1), Some(path));
                let mut length = 0;
                let mut authorized = false;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if line.to_lowercase().starts_with("authorization:") {
                        authorized = line.trim().ends_with("Bearer test-token");
                    }
                    if line.to_lowercase().starts_with("content-length:") {
                        length = line
                            .split(':')
                            .nth(1)
                            .unwrap()
                            .trim()
                            .parse::<usize>()
                            .unwrap();
                    }
                }
                assert!(authorized);
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                if first.starts_with("POST ") {
                    let body: Value = serde_json::from_slice(&body).unwrap();
                    assert_eq!(
                        body["answers"]["q_0"],
                        json!({"kind":"multi","option_ids":["opt_blue"]})
                    );
                }
                let body = json!({"code":0,"data":data}).to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let root = std::env::temp_dir().join(format!("ar-kimi-server-{}", Uuid::now_v7()));
        let store = RuntimeStore::open(root.join("test.sqlite")).unwrap();
        let waiters = WaiterRegistry::default();
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let mut pending = HashMap::new();
        let mut seen = HashSet::new();
        poll(
            &client,
            &endpoint,
            "instance",
            "test-token",
            &store,
            &waiters,
            &mut pending,
            &mut seen,
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(pending.len(), 1);
        let p = pending.values().next().unwrap();
        let id = p.ticket.request_id();
        assert_eq!(
            waiters.interactive_prompt(id).unwrap().unwrap().questions[0].prompt,
            "Colors?"
        );
        waiters
            .answer(id, &json!({"answers":{"q_0":["Blue"]}}))
            .unwrap();
        let reply = p.ticket.recv_timeout(Duration::ZERO).unwrap();
        let (suffix, body) = p.route.reply(&reply).unwrap();
        assert_eq!(
            http(
                &client,
                &endpoint,
                "test-token",
                &(p.path.clone() + suffix),
                Some(body)
            )
            .unwrap()["resolved"],
            true
        );
        server.join().unwrap();
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
