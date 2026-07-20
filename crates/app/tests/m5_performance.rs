#![cfg(unix)]

use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

fn percentile_95(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    let index = ((samples.len() * 95).div_ceil(100)).saturating_sub(1);
    samples[index]
}

fn wait_for(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(started.elapsed() < Duration::from_secs(3));
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn nonblocking_hook_process_p95_is_below_50_ms() {
    let root = PathBuf::from("/tmp").join(format!(
        "actrealm-m5-hook-perf-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    fs::create_dir_all(&root).unwrap();
    let socket = root.join("bridge.sock");
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args([
            "serve",
            "--approval",
            "pass-through",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(&socket);

    let payload = json!({
        "hook_event_name":"SessionStart",
        "session_id":"performance-session"
    })
    .to_string();
    let run = || {
        let started = Instant::now();
        let mut hook = Command::new(env!("CARGO_BIN_EXE_actrealm"))
            .args([
                "hook",
                "--provider",
                "claude",
                "--socket",
                socket.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        hook.stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        assert!(hook.wait().unwrap().success());
        started.elapsed()
    };
    for _ in 0..5 {
        let _ = run();
    }
    let mut samples = (0..40).map(|_| run()).collect::<Vec<_>>();
    let p95 = percentile_95(&mut samples);
    eprintln!("nonblocking_hook_p95_ms={:.3}", p95.as_secs_f64() * 1_000.0);
    runtime.kill().unwrap();
    runtime.wait().unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(
        p95 < Duration::from_millis(50),
        "nonblocking hook p95 was {p95:?}"
    );
}
