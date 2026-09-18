use super::*;
use crate::native_transport::{ConnectRequest, NativeIdentity};

struct Fixture {
    root: PathBuf,
    state: AppState,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("native-client-test-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = super::tests::test_state(store, &root);
        Self { root, state }
    }
    fn registrar(&self) -> native_clients::Registrar {
        native_clients::Registrar::new(self.state.clone()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn connect(previous: Option<&str>, enable: bool) -> ConnectRequest {
    ConnectRequest {
        schema_version: 1,
        previous_token: previous.map(str::to_owned),
        enable_access: enable,
    }
}

fn headers(response: &Value) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "127.0.0.1:43111".parse().unwrap());
    headers.insert(ORIGIN, "http://127.0.0.1:43111".parse().unwrap());
    headers.insert(
        COOKIE,
        format!(
            "actrealm_session={}",
            response["sessionToken"].as_str().unwrap()
        )
        .parse()
        .unwrap(),
    );
    headers.insert(
        CSRF_HEADER,
        response["csrfToken"].as_str().unwrap().parse().unwrap(),
    );
    headers
}

#[test]
fn native_desktop_sessions_coexist_and_csrf_is_bound_to_its_own_session() {
    let fixture = Fixture::new();
    let registrar = fixture.registrar();
    let first = registrar.connect(NativeIdentity::ActRealm, connect(None, false));
    let second = registrar.connect(NativeIdentity::ActRealm, connect(None, false));
    assert_ne!(first["sessionToken"], second["sessionToken"]);
    let first_headers = headers(&first);
    let mut second_headers = headers(&second);
    assert!(authorized_mutation(&fixture.state, &first_headers));
    assert!(authorized_mutation(&fixture.state, &second_headers));
    second_headers.insert(CSRF_HEADER, first_headers[CSRF_HEADER].clone());
    assert!(!authorized_mutation(&fixture.state, &second_headers));
    assert!(authorized(&fixture.state, &first_headers));
}

#[tokio::test]
async fn native_display_is_scoped_and_explicit_revocation_survives_restart_and_keychain_loss() {
    let fixture = Fixture::new();
    let registrar = fixture.registrar();
    let desktop = registrar.connect(NativeIdentity::ActRealm, connect(None, false));
    let display = registrar.connect(NativeIdentity::Display, connect(None, false));
    assert!(display.get("error").is_none(), "{display}");
    let display_headers = headers(&display);
    assert!(!authorized(&fixture.state, &display_headers));
    assert!(!authorized_mutation(&fixture.state, &display_headers));
    assert!(authorized_setup(&fixture.state, &display_headers, false));
    assert!(authorized_setup(&fixture.state, &display_headers, true));
    let revoked = revoke_companion(
        State(fixture.state.clone()),
        Path(display["companionId"].as_str().unwrap().to_owned()),
        headers(&desktop),
    )
    .await;
    assert_eq!(revoked.status(), StatusCode::OK);
    assert!(!authorized_setup(&fixture.state, &display_headers, false));
    assert!(!authorized_setup(&fixture.state, &display_headers, true));
    drop(registrar);
    let registrar = fixture.registrar();
    assert_eq!(
        registrar.connect(NativeIdentity::Display, connect(None, false))["error"],
        "accessRevoked"
    );
    assert_eq!(
        registrar.connect(
            NativeIdentity::Display,
            connect(display["token"].as_str(), false)
        )["error"],
        "accessRevoked"
    );
    let enabled = registrar.connect(NativeIdentity::Display, connect(None, true));
    assert!(enabled.get("error").is_none(), "{enabled}");
    assert_ne!(enabled["companionId"], display["companionId"]);
    assert!(!authorized_setup(&fixture.state, &display_headers, true));
    assert!(authorized_setup(&fixture.state, &headers(&enabled), true));
    let file = fs::read_to_string(&fixture.state.data_paths.companion_auth).unwrap();
    assert!(!file.contains(enabled["token"].as_str().unwrap()));
}

#[test]
fn native_migration_preserves_read_only_grant_and_previous_token() {
    let fixture = Fixture::new();
    let token = "a".repeat(64);
    let id = Uuid::now_v7().to_string();
    fixture
        .state
        .companions
        .lock()
        .unwrap()
        .registrations
        .push(CompanionRegistration {
            id: id.clone(),
            client_name: "Display".into(),
            token_hash: secret_hash(&token),
            scopes: vec![COMPANION_SCOPE_SNAPSHOT.into(), COMPANION_SCOPE_JUMP.into()],
            created_at: 1000,
        });
    let response = fixture
        .registrar()
        .connect(NativeIdentity::Display, connect(Some(&token), false));
    assert_eq!(response["token"], token);
    assert_eq!(response["companionId"], id);
    assert_eq!(
        response["scopes"],
        json!([COMPANION_SCOPE_SNAPSHOT, COMPANION_SCOPE_JUMP])
    );
    assert!(authorized_setup(&fixture.state, &headers(&response), false));
    assert!(!authorized_setup(&fixture.state, &headers(&response), true));
    assert!(!authorized(&fixture.state, &headers(&response)));
    let reloaded = load_companion_state(&fixture.state.data_paths.companion_auth).unwrap();
    assert_eq!(reloaded.registrations.len(), 1);
}

#[test]
fn invalid_legacy_token_does_not_silently_turn_into_a_new_grant() {
    let fixture = Fixture::new();
    let registrar = fixture.registrar();
    assert_eq!(
        registrar.connect(
            NativeIdentity::Display,
            connect(Some(&"b".repeat(64)), false)
        )["error"],
        "accessRevoked"
    );
    assert_eq!(
        fixture
            .registrar()
            .connect(NativeIdentity::Display, connect(None, false))["error"],
        "accessRevoked"
    );
    assert!(fixture
        .state
        .companions
        .lock()
        .unwrap()
        .registrations
        .is_empty());
    let enabled = registrar.connect(NativeIdentity::Display, connect(None, true));
    assert!(enabled.get("error").is_none());
}
