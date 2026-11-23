use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/discovery")
}

fn git(directory: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(directory)
        .args(args)
        .output()
        .expect("run git")
}

#[test]
fn list_json_exposes_selected_artifacts_and_explanation_data() {
    let output = Command::new(env!("CARGO_BIN_EXE_skills"))
        .args([
            "list",
            fixture().to_str().unwrap(),
            "--json",
            "--explain",
            "--include",
            "tag:repository+nested",
        ])
        .output()
        .expect("run skills");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON list output");
    assert_eq!(value["skills"][0]["path"], "skills/nested/deep/review");
    assert_eq!(value["skills"][0]["files"][0], "SKILL.md");
    assert!(value["skills"][0]["description"].is_string());
    assert!(value["skills"][0]["tags"].is_array());
    assert!(value["skills"][0]["aliases"].is_array());
    assert_eq!(value["shadowed"][0]["winner"], "skills/shadow/early");
    assert!(value["explain"]["shadowed"].is_array());
}

#[test]
fn list_tree_reports_missing_selectors_without_v1_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_skills"))
        .args([
            "list",
            fixture().to_str().unwrap(),
            "--tree",
            "--explain",
            "missing",
        ])
        .output()
        .expect("run skills");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("missing include selector"));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("not selected by any include selector")
    );
}

#[test]
fn list_repository_ignores_virtualenv_and_developer_artifacts() {
    let repository = TempDir::new().unwrap();
    let skill = repository.path().join("skills/demo");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    )
    .unwrap();
    fs::create_dir_all(repository.path().join(".mise")).unwrap();
    fs::write(
        repository.path().join(".mise/skills.toml"),
        "version = 1\n[discovery]\nroots = [\"skills\"]\n",
    )
    .unwrap();
    fs::create_dir_all(repository.path().join(".venv/lib")).unwrap();
    fs::write(repository.path().join(".venv/lib/SKILL.md"), "not a skill\n").unwrap();
    fs::create_dir_all(repository.path().join("target/debug")).unwrap();
    fs::write(repository.path().join("target/debug/SKILL.md"), "not a skill\n").unwrap();

    let source = format!("path:{}", repository.path().display());
    let output = Command::new(env!("CARGO_BIN_EXE_skills"))
        .args(["list", &source, "--json", "demo"])
        .output()
        .expect("run repository skills list");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("repository JSON");
    assert_eq!(value["skills"][0]["name"], "demo");
    assert!(value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|diagnostic| { diagnostic["level"] != "error" }));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(".venv"));
    assert!(!stdout.contains("target"));
}

#[test]
fn list_remote_file_git_source_fetches_revision_and_metadata_without_editing_config() {
    let repository = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let skill = repository.path().join("skills/demo");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: remote-demo\ndescription: Remote fixture description.\ntags: [fixture]\n---\n# Demo\n",
    )
    .unwrap();
    fs::create_dir_all(repository.path().join(".mise")).unwrap();
    fs::write(
        repository.path().join(".mise/skills.toml"),
        "version = 1\n[discovery]\nroots = [\"skills\"]\n[tags]\n\"skills/**\" = [\"remote\"]\n[aliases]\nexample = \"skills/demo\"\n",
    )
    .unwrap();
    assert!(git(repository.path(), &["init", "--quiet"])
        .status
        .success());
    assert!(git(repository.path(), &["add", "."]).status.success());
    assert!(git(
        repository.path(),
        &[
            "-c",
            "user.name=skills-test",
            "-c",
            "user.email=skills-test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ]
    )
    .status
    .success());
    let revision = String::from_utf8(git(repository.path(), &["rev-parse", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_string();

    let source = format!("git:file://{}", repository.path().display());
    let output = Command::new(env!("CARGO_BIN_EXE_skills"))
        .env("SKILLS_CACHE_DIR", cache.path())
        .args(["list", &source, "--json", "--explain", "example"])
        .output()
        .expect("run skills");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON list output");
    assert_eq!(value["source"], source);
    assert_eq!(value["revision"], revision);
    assert_eq!(value["track"], "master");
    assert_eq!(value["skills"][0]["name"], "remote-demo");
    let tags = value["skills"][0]["tags"].as_array().unwrap();
    assert!(tags.iter().any(|tag| tag == "remote"));
    assert_eq!(value["skills"][0]["aliases"][0], "example");
    assert_eq!(
        value["skills"][0]["description"],
        "Remote fixture description."
    );
    assert_eq!(value["skills"][0]["tags"][0], "fixture");

    let github_base = TempDir::new().unwrap();
    let github_owner = github_base.path().join("owner");
    fs::create_dir_all(&github_owner).unwrap();
    assert!(git(
        repository.path(),
        &[
            "clone",
            "--bare",
            ".",
            github_owner.join("repo.git").to_str().unwrap()
        ]
    )
    .status
    .success());
    let github = Command::new(env!("CARGO_BIN_EXE_skills"))
        .env("SKILLS_CACHE_DIR", cache.path())
        .env(
            "MISE_SKILLS_GITHUB_BASE_URL",
            format!("file://{}", github_base.path().display()),
        )
        .args(["list", "owner/repo", "--json", "example"])
        .output()
        .expect("run GitHub-seam skills list");
    assert!(
        github.status.success(),
        "{}",
        String::from_utf8_lossy(&github.stderr)
    );
    let github_value: Value = serde_json::from_slice(&github.stdout).expect("GitHub JSON output");
    assert_eq!(github_value["source"], "github:owner/repo");
    assert_eq!(github_value["revision"], revision);
    assert_eq!(github_value["skills"][0]["aliases"][0], "example");

    let tracked = Command::new(env!("CARGO_BIN_EXE_skills"))
        .env("SKILLS_CACHE_DIR", cache.path())
        .env(
            "MISE_SKILLS_GITHUB_BASE_URL",
            format!("file://{}", github_base.path().display()),
        )
        .args(["list", "github:owner/repo", "--track", "master", "--json"])
        .output()
        .expect("run tracked GitHub-seam skills list");
    assert!(tracked.status.success());
    let tracked_value: Value = serde_json::from_slice(&tracked.stdout).unwrap();
    assert_eq!(tracked_value["track"], "master");
    assert_eq!(tracked_value["revision"], revision);
    assert!(!repository.path().join("mise.toml").exists());
}

#[test]
fn list_caches_resolved_refs_and_refresh_bypasses_them() {
    let repository = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let skill = repository.path().join("skills/demo");
    fs::create_dir_all(&skill).unwrap();
    assert!(git(repository.path(), &["init", "--quiet"])
        .status
        .success());
    let commit = |message: &str, body: &str| {
        fs::write(skill.join("SKILL.md"), body).unwrap();
        assert!(git(repository.path(), &["add", "."]).status.success());
        assert!(git(
            repository.path(),
            &[
                "-c",
                "user.name=skills-test",
                "-c",
                "user.email=skills-test@example.invalid",
                "commit",
                "--quiet",
                "-m",
                message,
            ]
        )
        .status
        .success());
        String::from_utf8(git(repository.path(), &["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string()
    };
    let first = commit("first", "---\nname: demo\ndescription: one\n---\n");
    let source = format!("git:file://{}", repository.path().display());
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_skills"))
            .env("SKILLS_CACHE_DIR", cache.path())
            .args(args)
            .output()
            .expect("run skills")
    };

    let listed = run(&["list", &source, "--json"]);
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let value: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(value["revision"], first);

    let second = commit("second", "---\nname: demo\ndescription: two\n---\n");
    assert_ne!(first, second);

    // Within the TTL the cached resolution is reused, so the first revision is
    // still reported even though the default branch moved.
    let cached = run(&["list", &source, "--json"]);
    assert!(cached.status.success());
    let value: Value = serde_json::from_slice(&cached.stdout).unwrap();
    assert_eq!(value["revision"], first);

    // --refresh re-resolves the default branch and fetches the new revision.
    let refreshed = run(&["list", &source, "--json", "--refresh"]);
    assert!(
        refreshed.status.success(),
        "{}",
        String::from_utf8_lossy(&refreshed.stderr)
    );
    let value: Value = serde_json::from_slice(&refreshed.stdout).unwrap();
    assert_eq!(value["revision"], second);
}

#[test]
fn list_handles_broken_pipe_without_panicking() {
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} list {} --tree | head -n 1",
            env!("CARGO_BIN_EXE_skills"),
            fixture().display()
        ))
        .output()
        .expect("run skills through head");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn list_accepts_flags_after_positional_selectors() {
    let output = Command::new(env!("CARGO_BIN_EXE_skills"))
        .args(["list", fixture().to_str().unwrap(), "missing", "--json"])
        .output()
        .expect("run skills");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON list output");
    assert!(value["selection"].is_object());
}

#[test]
fn validate_fetches_a_remote_source() {
    let repository = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let skill = repository.path().join("skills/demo");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: demo\ndescription: demo\n---\n",
    )
    .unwrap();
    assert!(git(repository.path(), &["init", "--quiet"])
        .status
        .success());
    assert!(git(repository.path(), &["add", "."]).status.success());
    assert!(git(
        repository.path(),
        &[
            "-c",
            "user.name=skills-test",
            "-c",
            "user.email=skills-test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ]
    )
    .status
    .success());
    let source = format!("git:file://{}", repository.path().display());
    let output = Command::new(env!("CARGO_BIN_EXE_skills"))
        .env("SKILLS_CACHE_DIR", cache.path())
        .args(["validate", &source, "--json"])
        .output()
        .expect("run skills");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON validate output");
    assert_eq!(value["source"], source);
    assert!(value["diagnostics"].is_array());
}
