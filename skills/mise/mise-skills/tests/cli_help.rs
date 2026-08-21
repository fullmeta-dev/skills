use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_mise-skills")
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(binary())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run mise-skills")
}

#[test]
fn documented_commands_have_scope_failure_and_example_help() {
    let temp = TempDir::new().unwrap();
    let top = run(temp.path(), &["--help"]);
    assert!(top.status.success());
    let top_text = String::from_utf8_lossy(&top.stdout);
    for phrase in ["Normal behavior:", "Scope:", "Examples:", "completions"] {
        assert!(top_text.contains(phrase), "top-level help missing {phrase}");
    }

    for command in [
        "list",
        "use",
        "sync",
        "status",
        "upgrade",
        "remove",
        "prune",
        "validate",
        "completions",
    ] {
        let output = run(temp.path(), &[command, "--help"]);
        assert!(output.status.success(), "{command} --help failed");
        let text = String::from_utf8_lossy(&output.stdout);
        for phrase in ["Normal:", "Failure:", "Scope:", "Example:"] {
            assert!(
                text.contains(phrase),
                "{command} help missing {phrase}: {text}"
            );
        }
    }
}

#[test]
fn generated_completions_match_checked_in_artifacts() {
    let temp = TempDir::new().unwrap();
    for (shell, relative_path) in [
        ("bash", "completions/mise-skills.bash"),
        ("zsh", "completions/_mise-skills"),
        ("fish", "completions/mise-skills.fish"),
        ("powershell", "completions/mise-skills.ps1"),
    ] {
        let output = run(temp.path(), &["completions", shell]);
        assert!(
            output.status.success(),
            "completion generation failed: {shell}"
        );
        let expected = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path),
        )
        .expect("checked-in completion artifact");
        assert_eq!(
            output.stdout, expected,
            "checked-in completion is stale: {shell}"
        );
    }
    assert!(!temp.path().join("mise.toml").exists());
}

#[test]
fn shipped_completion_shells_are_generated_without_side_effects() {
    let temp = TempDir::new().unwrap();
    for (shell, marker) in [
        ("bash", "_mise__skills()"),
        ("zsh", "#compdef mise-skills"),
        ("fish", "complete -c mise-skills"),
        ("powershell", "Register-ArgumentCompleter"),
    ] {
        let output = run(temp.path(), &["completions", shell]);
        assert!(
            output.status.success(),
            "completion generation failed: {shell}"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(marker),
            "completion marker missing for {shell}"
        );
    }
    assert!(!temp.path().join("mise.toml").exists());
}

#[test]
fn bootstrap_artifact_declares_the_complete_global_flow() {
    let artifact = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../docs/mise-skills-bootstrap.toml"),
    )
    .expect("global mise bootstrap artifact");
    for required in [
        "[plugins]",
        "vfox-backend:skills",
        "#348a5b2021d1e8b6d398c1da068fee90bb1c3653",
        "[tools.\"github:fullmeta-dev/skills\"]",
        "bin = \"mise-skills\"",
        "[tasks.skills]",
        "usage_arguments",
        "[bootstrap.hooks.pre-tools]",
        "mise-skills-bootstrap-plugin.sh",
    ] {
        assert!(artifact.contains(required), "bootstrap artifact missing {required}");
    }
}

#[test]
fn strict_validation_reports_tags_as_a_nonblocking_diagnostic() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source").join("skills").join("demo");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\ndescription: Demo\ntags: [web]\n---\n",
    )
    .unwrap();
    let source_root = source.parent().unwrap().parent().unwrap();
    let output = run(
        temp.path(),
        &["validate", source_root.to_str().unwrap(), "--json"],
    );
    assert!(output.status.success(), "validation failed: {output:?}");
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["diagnostics"].as_array().unwrap().iter().any(|item| {
        item["message"]
            .as_str()
            .unwrap()
            .contains("tags` is an intentional extension")
    }));
    assert!(!temp.path().join("mise.toml").exists());
}
