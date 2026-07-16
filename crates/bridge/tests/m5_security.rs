#![cfg(unix)]

use flow_agent_bridge::{BridgeListener, MAX_BRIDGE_FRAME_BYTES};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread;
use uuid::Uuid;

fn socket(name: &str) -> PathBuf {
    PathBuf::from("/tmp").join(format!(
        "fa-m5-bridge-{name}-{}-{}/run/bridge.sock",
        std::process::id(),
        Uuid::now_v7()
    ))
}

#[test]
fn socket_and_created_parent_are_private_and_oversize_frame_is_rejected() {
    let path = socket("oversize");
    let listener = BridgeListener::bind(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let path_for_client = path.clone();
    let server = thread::spawn(move || {
        let mut stream = listener.incoming().next().unwrap().unwrap();
        BridgeListener::read_request(&mut stream)
    });
    let mut client = UnixStream::connect(path_for_client).unwrap();
    client
        .write_all(&vec![b'x'; MAX_BRIDGE_FRAME_BYTES + 1])
        .unwrap();
    client.write_all(b"\n").unwrap();
    drop(client);
    assert!(server.join().unwrap().is_err());
    let root = path.parent().unwrap().parent().unwrap();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deeply_nested_bridge_json_is_rejected_without_panicking() {
    let path = socket("deep");
    let listener = BridgeListener::bind(&path).unwrap();
    let path_for_client = path.clone();
    let server = thread::spawn(move || {
        let mut stream = listener.incoming().next().unwrap().unwrap();
        BridgeListener::read_request(&mut stream)
    });
    let mut frame = format!(
        "{{\"v\":1,\"id\":\"{}\",\"requestId\":null,\"provider\":\"claude\",\"providerSessionId\":null,\"providerTurnId\":null,\"promptId\":null,\"role\":\"primary\",\"receivedAt\":1,\"deadlineAt\":null,\"needsReply\":false,\"term\":null,\"raw\":",
        Uuid::now_v7()
    );
    frame.push_str(&"[".repeat(200));
    frame.push('0');
    frame.push_str(&"]".repeat(200));
    frame.push_str("}\n");
    let mut client = UnixStream::connect(path_for_client).unwrap();
    client.write_all(frame.as_bytes()).unwrap();
    drop(client);
    assert!(server.join().unwrap().is_err());
    let root = path.parent().unwrap().parent().unwrap();
    let _ = fs::remove_dir_all(root);
}
