use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore = "explicit release-candidate resource gate"]
fn two_minute_release_candidate_resource_gate() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = workspace.join("target/release/actrealm");
    assert!(binary.is_file(), "release binary must be built first");
    let report = std::env::temp_dir().join("actrealm-m6-m8-resource.json");
    let output = Command::new(workspace.join("scripts/m5-resource-check.sh"))
        .arg(&binary)
        .current_dir(&workspace)
        .env("ACTREALM_RESOURCE_DURATION_SECONDS", "120")
        .env("ACTREALM_RESOURCE_REPORT", &report)
        .output()
        .expect("resource checker must start");
    assert!(
        output.status.success(),
        "resource gate failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence = std::fs::read_to_string(&report).expect("resource report must exist");
    assert!(evidence.contains("\"durationSeconds\":120"));
    println!("{}", evidence.trim());
}
