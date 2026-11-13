use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_skills")
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(binary())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run skills")
}

fn help(cwd: &Path, args: &[&str]) -> String {
    let output = run(cwd, args);
    assert!(
        output.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{args:?} wrote stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn documented_commands_are_self_documenting_for_operators_and_agents() {
    let temp = TempDir::new().unwrap();
    let top = help(temp.path(), &["--help"]);
    for phrase in [
        "Bootstrap (once per machine):",
        "mise plugins install skills",
        "release-compatibility.json",
        "bin = \"skills\"",
        "owner/repository",
        "path:DIR",
        "tag:web+frontend",
        "excludes always win",
        "[tools.\"skills:fullmeta-dev/skills\"]",
        "postinstall = [\"skills sync --locked\"]",
        "~/.pi/agent/skills",
        "Ownership and safety:",
        "Lifecycle:",
        "Examples:",
    ] {
        assert!(top.contains(phrase), "top-level help missing {phrase}");
    }
    assert!(
        !top.contains("unavailable inspection commands are reported"),
        "top-level help still claims doctor reports unavailable inspections"
    );
    let use_help = help(temp.path(), &["use", "--help"]);
    assert!(
        !use_help.contains("Explicit Git ref to resolve once"),
        "use help still describes --reference as a one-time resolution"
    );

    for command in [
        "list",
        "use",
        "sync",
        "status",
        "upgrade",
        "install",
        "self-update",
        "doctor",
        "remove",
        "prune",
        "validate",
        "completions",
    ] {
        let text = help(temp.path(), &[command, "--help"]);
        for phrase in ["Normal:", "Example:"] {
            assert!(
                text.contains(phrase),
                "{command} help missing {phrase}: {text}"
            );
        }
    }

    let list = help(temp.path(), &["list", "--help"]);
    for phrase in [
        "SOURCE is required",
        "temporary checkout",
        "immutable",
        "--reference",
        "with none, every discovered skill",
    ] {
        assert!(list.contains(phrase), "list help missing {phrase}");
    }
    let use_help = help(temp.path(), &["use", "--help"]);
    for phrase in [
        "at least one --agent",
        "never installs or activates",
        "--locked requires an existing declaration",
    ] {
        assert!(use_help.contains(phrase), "use help missing {phrase}");
    }
    let sync = help(temp.path(), &["sync", "--help"]);
    for phrase in ["never installs", "does not change behavior"] {
        assert!(sync.contains(phrase), "sync help missing {phrase}");
    }
    let doctor = help(temp.path(), &["doctor", "--help"]);
    assert!(!doctor.contains("unavailable inspection commands are reported"));
    let self_update = help(temp.path(), &["self-update", "--help"]);
    for phrase in ["not a bootstrap command", "release-compatibility.json"] {
        assert!(
            self_update.contains(phrase),
            "self-update help missing {phrase}"
        );
    }
    assert!(help(temp.path(), &["validate", "--help"]).contains("Alias: check"));
    assert!(help(temp.path(), &["completions", "--help"]).contains("Alias: completion"));
}

#[test]
fn generated_completions_match_checked_in_artifacts() {
    let temp = TempDir::new().unwrap();
    for (shell, relative_path) in [
        ("bash", "completions/skills.bash"),
        ("zsh", "completions/_skills"),
        ("fish", "completions/skills.fish"),
        ("powershell", "completions/skills.ps1"),
    ] {
        let output = run(temp.path(), &["completions", shell]);
        assert!(
            output.status.success(),
            "completion generation failed: {shell}"
        );
        let expected = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path))
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
        ("bash", "_skills()"),
        ("zsh", "#compdef skills"),
        ("fish", "complete -c skills"),
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
