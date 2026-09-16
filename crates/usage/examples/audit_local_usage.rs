//! Read-only source audit with an isolated numeric checkpoint directory.
use actrealm_usage::{UsageCollector, UsagePaths};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn main() {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("provide an isolated audit directory"),
    );
    let mut paths = UsagePaths::discover();
    let live = paths
        .actrealm_home
        .canonicalize()
        .unwrap_or_else(|_| paths.actrealm_home.clone());
    std::fs::create_dir_all(&output).expect("create audit directory");
    let output = output.canonicalize().expect("resolve audit directory");
    assert_ne!(
        output, live,
        "audit must not replace the live Runtime cache"
    );
    std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o700)).unwrap();
    paths.actrealm_home = output.clone();
    let mut collector = UsageCollector::new(paths);
    let started = std::time::Instant::now();
    let records = loop {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let records = collector.collect(now);
        if collector.is_caught_up() {
            break records;
        }
        assert!(
            started.elapsed().as_secs() < 180,
            "audit did not reach a stable source boundary"
        );
    };
    collector.flush_scan_checkpoint();
    let encoded = serde_json::to_vec_pretty(&records).expect("encode numeric records");
    std::fs::write(output.join("records.json"), encoded).expect("save numeric audit");
    std::fs::set_permissions(
        output.join("records.json"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    println!(
        "{} records; history complete: {}; elapsed: {:?}",
        records.len(),
        collector.is_history_complete(),
        started.elapsed()
    );
}
