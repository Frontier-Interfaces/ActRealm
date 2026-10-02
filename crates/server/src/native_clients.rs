use super::*;
use crate::native_transport::{ConnectRequest, NativeIdentity};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DisplayPolicy {
    enabled: bool,
    companion_id: Option<String>,
    scopes: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyFile {
    schema_version: u32,
    display: Option<DisplayPolicy>,
}

pub(super) struct Registrar {
    state: AppState,
    pub(super) certificate_sha256: Option<String>,
    policy: Mutex<PolicyFile>,
    path: PathBuf,
}

impl Registrar {
    pub(super) fn new(state: AppState) -> io::Result<Self> {
        let path = state
            .data_paths
            .companion_auth
            .with_file_name("native-client-policy.json");
        let policy = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || metadata.uid() != unsafe { libc::geteuid() }
                    || metadata.mode() & 0o077 != 0
                    || metadata.len() > 16 * 1024
                {
                    return Err(io::Error::other(
                        "native client policy must be a private regular file",
                    ));
                }
                let policy: PolicyFile =
                    serde_json::from_slice(&fs::read(&path)?).map_err(io::Error::other)?;
                if policy.schema_version != 1
                    || policy.display.as_ref().is_some_and(|entry| {
                        entry
                            .companion_id
                            .as_ref()
                            .is_some_and(|id| Uuid::parse_str(id).is_err())
                            || entry.scopes.iter().any(|scope| !is_companion_scope(scope))
                    })
                {
                    return Err(io::Error::other("invalid native client policy"));
                }
                policy
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => PolicyFile {
                schema_version: 1,
                display: None,
            },
            Err(error) => return Err(error),
        };
        Ok(Self {
            state,
            certificate_sha256: None,
            policy: Mutex::new(policy),
            path,
        })
    }

    pub(super) fn connect(&self, identity: NativeIdentity, request: ConnectRequest) -> Value {
        match self.try_connect(identity, request) {
            Ok(response) => response,
            Err(code) => json!({"error": code}),
        }
    }

    fn try_connect(
        &self,
        identity: NativeIdentity,
        request: ConnectRequest,
    ) -> Result<Value, &'static str> {
        let mut response = json!({
            "schemaVersion": 1,
            "protocolVersion": PUBLIC_PROTOCOL_VERSION,
            "endpoint": self.state.expected_origin,
            "instanceId": self.state.instance_id,
            "certificateSha256": self.certificate_sha256,
        });
        match identity {
            NativeIdentity::ActRealm => {
                self.issue_session(&mut response, NativeSessionKind::Full, None)?;
            }
            NativeIdentity::Display => {
                let mut policy = self.policy.lock().map_err(|_| "unavailable")?;
                let mut companions = self.state.companions.lock().map_err(|_| "unavailable")?;
                let previous = request.previous_token.as_ref().and_then(|token| {
                    let hash = secret_hash(token);
                    companions
                        .registrations
                        .iter()
                        .find(|item| constant_time_eq(&item.token_hash, &hash))
                        .cloned()
                });
                let registered = policy.display.as_ref().and_then(|entry| {
                    companions
                        .registrations
                        .iter()
                        .find(|item| Some(&item.id) == entry.companion_id.as_ref())
                        .cloned()
                });
                // A missing registration after native enrollment means explicit
                // revocation. Losing or removing a Keychain item cannot undo it.
                let revoked = policy
                    .display
                    .as_ref()
                    .is_some_and(|entry| !entry.enabled || registered.is_none())
                    || (policy.display.is_none()
                        && request.previous_token.is_some()
                        && previous.is_none());
                if revoked && !request.enable_access {
                    if policy.display.is_none() {
                        let next = PolicyFile {
                            schema_version: 1,
                            display: Some(DisplayPolicy::default()),
                        };
                        self.persist_policy(&next).map_err(|_| "unavailable")?;
                        *policy = next;
                    }
                    return Err("accessRevoked");
                }
                let existing = registered.or(previous);
                let scopes = existing
                    .as_ref()
                    .map(|item| item.scopes.clone())
                    .or_else(|| {
                        policy
                            .display
                            .as_ref()
                            .map(|item| item.scopes.clone())
                            .filter(|scopes| !scopes.is_empty())
                    })
                    .unwrap_or_else(|| {
                        vec![
                            COMPANION_SCOPE_SNAPSHOT.into(),
                            COMPANION_SCOPE_JUMP.into(),
                            COMPANION_SCOPE_RESPOND.into(),
                        ]
                    });
                let id = existing
                    .as_ref()
                    .map(|item| item.id.clone())
                    .unwrap_or_else(|| Uuid::now_v7().to_string());
                let token = match (&request.previous_token, &existing) {
                    (Some(token), Some(existing))
                        if constant_time_eq(&secret_hash(token), &existing.token_hash) =>
                    {
                        token.clone()
                    }
                    _ => generate_secret().map_err(|_| "unavailable")?,
                };
                let registration = CompanionRegistration {
                    id: id.clone(),
                    client_name: "Display".into(),
                    token_hash: secret_hash(&token),
                    scopes: scopes.clone(),
                    created_at: existing
                        .as_ref()
                        .map_or_else(now_millis, |item| item.created_at),
                };
                let mut registrations = companions.registrations.clone();
                registrations.retain(|item| item.id != id);
                if registrations.len() >= 16 {
                    return Err("registrationLimit");
                }
                registrations.push(registration);
                let next_companions = CompanionState {
                    pairing: companions.pairing.clone(),
                    registrations,
                };
                let next_policy = PolicyFile {
                    schema_version: 1,
                    display: Some(DisplayPolicy {
                        enabled: true,
                        companion_id: Some(id.clone()),
                        scopes: scopes.clone(),
                    }),
                };
                persist_companion_state(&self.state.data_paths.companion_auth, &next_companions)
                    .map_err(|_| "unavailable")?;
                if self.persist_policy(&next_policy).is_err() {
                    let _ =
                        persist_companion_state(&self.state.data_paths.companion_auth, &companions);
                    return Err("unavailable");
                }
                *companions = next_companions;
                *policy = next_policy;
                response["token"] = json!(token);
                response["companionId"] = json!(id);
                response["scopes"] = json!(scopes);
                response["discoveryPath"] = json!(self.state.data_paths.companion_discovery);
                let access = if scopes.iter().any(|scope| scope == COMPANION_SCOPE_RESPOND) {
                    NativeSessionKind::SetupReadWrite
                } else {
                    NativeSessionKind::SetupReadOnly
                };
                drop(companions);
                drop(policy);
                self.issue_session(&mut response, access, Some(id))?;
            }
        }
        Ok(response)
    }

    fn issue_session(
        &self,
        response: &mut Value,
        kind: NativeSessionKind,
        companion_id: Option<String>,
    ) -> Result<(), &'static str> {
        let session_token = generate_secret().map_err(|_| "unavailable")?;
        let csrf_token = generate_secret().map_err(|_| "unavailable")?;
        let mut auth = self.state.auth.lock().map_err(|_| "unavailable")?;
        if auth.native_sessions.len() >= 16 {
            auth.native_sessions.remove(0);
        }
        auth.native_sessions.push(NativeSession {
            token: session_token.clone(),
            csrf: csrf_token.clone(),
            kind,
            companion_id,
        });
        response["sessionToken"] = json!(session_token);
        response["csrfToken"] = json!(csrf_token);
        Ok(())
    }

    fn persist_policy(&self, policy: &PolicyFile) -> io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("missing native policy directory"))?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(io::Error::other("native policy directory must be private"));
        }
        let temporary = self.path.with_extension(format!("tmp-{}", Uuid::now_v7()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)?;
            serde_json::to_writer(&mut file, policy)?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}
