use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../tests/fixtures/discovery")
}

#[test]
fn list_json_exposes_selected_artifacts_and_explanation_data() {
    let output = Command::new(env!("CARGO_BIN_EXE_mise-skills"))
        .args([
            "list",
            fixture().to_str().unwrap(),
            "--json",
            "--explain",
            "--include",
            "tag:repository+nested",
        ])
        .output()
        .expect("run mise-skills");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON list output");
    assert_eq!(value["skills"][0]["path"], "skills/nested/deep/review");
    assert_eq!(value["skills"][0]["files"][0], "SKILL.md");
    assert_eq!(value["shadowed"][0]["winner"], "skills/shadow/early");
    assert!(value["explain"]["shadowed"].is_array());
}

#[test]
fn list_tree_reports_missing_selectors_without_v1_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_mise-skills"))
        .args([
            "list",
            fixture().to_str().unwrap(),
            "--tree",
            "--explain",
            "missing",
        ])
        .output()
        .expect("run mise-skills");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("missing include selector"));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("not selected by any include selector")
    );
}
