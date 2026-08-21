use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_skills")
}

fn skill_source(root: &Path, name: &str) -> PathBuf {
    let source = root.join(name);
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: test\n---\n"),
    )
    .unwrap();
    source
}

fn run(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run skills")
}

fn run_with_env(cwd: &Path, args: &[&str], key: &str, value: &Path) -> std::process::Output {
    Command::new(binary())
        .current_dir(cwd)
        .args(args)
        .env(key, value)
        .output()
        .expect("run skills")
}

#[test]
fn use_edits_project_and_explicit_paths_without_damaging_hooks() {
    let temp = TempDir::new().unwrap();
    let source = skill_source(temp.path(), "project-source");
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(
        project.join("mise.toml"),
        "# retain\n[tools]\npython = \"3\"\n\n[hooks]\npostinstall = \"echo old\"\n\n[tasks.bootstrap]\nrun = \"echo bootstrap\"\n",
    )
    .unwrap();

    let source_arg = format!("path:{}", source.display());
    let output = run(
        &project,
        &["use", &source_arg, "project-source", "--agent", "pi"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = fs::read_to_string(project.join("mise.toml")).unwrap();
    assert!(config.contains("# retain"));
    assert!(config.contains("python = \"3\""));
    assert!(config.contains("skills sync --locked"));
    assert!(config.contains("echo old"));
    assert!(config.contains("[tasks.bootstrap]"));

    let before_repeat = config.clone();
    let output = run(
        &project,
        &["use", &source_arg, "project-source", "--agent", "pi"],
    );
    assert!(output.status.success());
    assert_eq!(
        before_repeat,
        fs::read_to_string(project.join("mise.toml")).unwrap()
    );

    let explicit = temp.path().join("explicit.toml");
    let output = run(
        &project,
        &[
            "use",
            "--path",
            explicit.to_str().unwrap(),
            &source_arg,
            "project-source",
            "--agent",
            "claude",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fs::read_to_string(explicit)
        .unwrap()
        .contains("[tools.\"skills:local/project-source\"]"));

    let global_dir = temp.path().join("global");
    fs::create_dir(&global_dir).unwrap();
    let output = run_with_env(
        &project,
        &[
            "use",
            "--global",
            &source_arg,
            "project-source",
            "--agent",
            "agents",
        ],
        "MISE_CONFIG_DIR",
        &global_dir,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(global_dir.join("config.toml").is_file());
}

#[test]
fn locked_use_preserves_a_pin_and_upgrade_changes_only_version() {
    let temp = TempDir::new().unwrap();
    let source = skill_source(temp.path(), "source");
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    let source_arg = format!("path:{}", source.display());
    let output = run(&project, &["use", &source_arg, "source", "--agent", "pi"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config_path = project.join("mise.toml");
    let initial = fs::read_to_string(&config_path).unwrap();
    let initial_version = initial
        .lines()
        .find(|line| line.starts_with("version = "))
        .unwrap()
        .to_string();

    fs::write(source.join("new.txt"), "new content").unwrap();
    let output = run(
        &project,
        &["use", "--locked", &source_arg, "source", "--agent", "pi"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let locked = fs::read_to_string(&config_path).unwrap();
    assert!(locked.contains(&initial_version));

    let output = run(&project, &["upgrade"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let upgraded = fs::read_to_string(&config_path).unwrap();
    assert!(!upgraded.contains(&initial_version));
    assert!(upgraded.contains("source = "));
    assert!(upgraded.contains("include = [\"source\"]"));
    assert!(upgraded.contains("agents = [\"pi\"]"));
    assert!(upgraded.contains("skills sync --locked"));
}

#[test]
fn git_sources_default_to_the_remote_branch_and_upgrade_explicitly() {
    let temp = TempDir::new().unwrap();
    let repository = temp.path().join("repo");
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    Command::new("git")
        .args(["init", "--quiet", repository.to_str().unwrap()])
        .status()
        .unwrap();
    for args in [
        [
            "-C",
            repository.to_str().unwrap(),
            "config",
            "user.email",
            "test@example.invalid",
        ],
        [
            "-C",
            repository.to_str().unwrap(),
            "config",
            "user.name",
            "test",
        ],
    ] {
        assert!(Command::new("git").args(args).status().unwrap().success());
    }
    fs::write(repository.join("one.txt"), "one").unwrap();
    assert!(Command::new("git")
        .args(["-C", repository.to_str().unwrap(), "add", "."])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args([
            "-C",
            repository.to_str().unwrap(),
            "commit",
            "--quiet",
            "-m",
            "one"
        ])
        .status()
        .unwrap()
        .success());
    let first_revision = String::from_utf8(
        Command::new("git")
            .args(["-C", repository.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    let source = format!("git:file://{}", repository.display());
    let output = run(&project, &["use", &source, "one", "--agent", "pi"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config_path = project.join("mise.toml");
    let initial = fs::read_to_string(&config_path).unwrap();
    assert!(initial.contains(&format!("version = \"rev:{first_revision}\"")));
    assert!(initial.contains("track = \"master\""));

    fs::write(repository.join("two.txt"), "two").unwrap();
    assert!(Command::new("git")
        .args(["-C", repository.to_str().unwrap(), "add", "."])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args([
            "-C",
            repository.to_str().unwrap(),
            "commit",
            "--quiet",
            "-m",
            "two"
        ])
        .status()
        .unwrap()
        .success());
    let output = run(&project, &["upgrade"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let upgraded = fs::read_to_string(config_path).unwrap();
    assert!(!upgraded.contains(&format!("version = \"rev:{first_revision}\"")));
    assert!(upgraded.contains("track = \"master\""));
}

#[test]
fn annotated_git_tags_pin_the_peeled_commit() {
    let temp = TempDir::new().unwrap();
    let repository = temp.path().join("repo");
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    Command::new("git")
        .args(["init", "--quiet", repository.to_str().unwrap()])
        .status()
        .unwrap();
    for args in [
        [
            "-C",
            repository.to_str().unwrap(),
            "config",
            "user.email",
            "test@example.invalid",
        ],
        [
            "-C",
            repository.to_str().unwrap(),
            "config",
            "user.name",
            "test",
        ],
    ] {
        assert!(Command::new("git").args(args).status().unwrap().success());
    }
    fs::write(repository.join("one.txt"), "one").unwrap();
    assert!(Command::new("git")
        .args(["-C", repository.to_str().unwrap(), "add", "."])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args([
            "-C",
            repository.to_str().unwrap(),
            "commit",
            "--quiet",
            "-m",
            "one",
        ])
        .status()
        .unwrap()
        .success());
    let commit = String::from_utf8(
        Command::new("git")
            .args(["-C", repository.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    assert!(Command::new("git")
        .args([
            "-C",
            repository.to_str().unwrap(),
            "tag",
            "--annotate",
            "v1",
            "--message",
            "v1",
        ])
        .status()
        .unwrap()
        .success());
    let tag_object = String::from_utf8(
        Command::new("git")
            .args([
                "-C",
                repository.to_str().unwrap(),
                "rev-parse",
                "refs/tags/v1",
            ])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    let source = format!("git:file://{}", repository.display());
    let output = run(
        &project,
        &["use", &source, "one", "--agent", "pi", "--reference", "v1"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = fs::read_to_string(project.join("mise.toml")).unwrap();
    assert!(config.contains(&format!("version = \"rev:{commit}\"")));
    assert!(!config.contains(&format!("version = \"rev:{tag_object}\"")));
    assert!(config.contains("track = \"v1\""));
}

#[test]
fn a_derived_key_cannot_be_repointed_to_a_different_source() {
    let temp = TempDir::new().unwrap();
    let first_root = temp.path().join("one");
    let second_root = temp.path().join("two");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let first = skill_source(&first_root, "same");
    let second = skill_source(&second_root, "same");
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    let first_arg = format!("path:{}", first.display());
    let second_arg = format!("path:{}", second.display());
    assert!(run(&project, &["use", &first_arg, "same", "--agent", "pi"])
        .status
        .success());
    let before = fs::read_to_string(project.join("mise.toml")).unwrap();
    let output = run(&project, &["use", &second_arg, "same", "--agent", "pi"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing to repoint"));
    assert_eq!(
        before,
        fs::read_to_string(project.join("mise.toml")).unwrap()
    );
}
