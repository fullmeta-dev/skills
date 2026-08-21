use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_skills")
}

fn run(cwd: &Path, args: &[&str], envs: &[(&str, &Path)]) -> Output {
    let mut command = Command::new(binary());
    command.current_dir(cwd).args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run skills")
}

fn skill(source: &Path, name: &str) {
    let directory = source.join(name);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: test\n---\n"),
    )
    .unwrap();
}

fn version(config: &Path) -> String {
    fs::read_to_string(config)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("version = \"")?.strip_suffix('"'))
        .unwrap()
        .to_string()
}

fn install_snapshot(data: &Path, version: &str, source: &Path) -> PathBuf {
    let install = data
        .join("installs")
        .join("skills-local-source")
        .join(version);
    fs::create_dir_all(&install).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = install.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
    install
}

fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn syncs_all_adapters_with_isolated_ownership_and_recovers_stale_links() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    let global_config = temp.path().join("global-config");
    for directory in [&home, &data, &state, &project, &source, &global_config] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    let project_config = project.join("mise.toml");
    let source_arg = format!("path:{}", source.display());
    let envs = [
        ("HOME", home.as_path()),
        ("MISE_DATA_DIR", data.as_path()),
        ("MISE_STATE_DIR", state.as_path()),
    ];

    let output = run(
        &project,
        &[
            "use",
            "--path",
            project_config.to_str().unwrap(),
            &source_arg,
            "demo",
            "--agent",
            "agents",
            "--agent",
            "codex",
            "--agent",
            "cursor",
            "--agent",
            "pi",
            "--agent",
            "claude",
        ],
        &envs,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pinned = version(&project_config);
    let install = install_snapshot(&data, &pinned, &source);

    let output = run(
        &project,
        &[
            "sync",
            "--path",
            project_config.to_str().unwrap(),
            "--locked",
        ],
        &envs,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for target in [
        project.join(".agents/skills/demo"),
        project.join(".pi/skills/demo"),
        project.join(".claude/skills/demo"),
    ] {
        let link = fs::read_link(&target).unwrap();
        assert!(
            link.is_relative(),
            "activation must use a relative link: {link:?}"
        );
        assert_eq!(
            fs::canonicalize(target).unwrap(),
            fs::canonicalize(install.join("demo")).unwrap()
        );
    }
    let manifest_count = fs::read_dir(state.join("skills")).unwrap().count();
    assert_eq!(manifest_count, 1, "codex and cursor aliases must coalesce");

    let repeat = run(
        &project,
        &["sync", "--path", project_config.to_str().unwrap()],
        &envs,
    );
    assert!(repeat.status.success());
    assert!(String::from_utf8_lossy(&repeat.stdout).contains("synced 0 link(s)"));

    let pi_target = project.join(".pi/skills/demo");
    fs::remove_file(&pi_target).unwrap();
    fs::write(&pi_target, "unmanaged").unwrap();
    let collision = run(
        &project,
        &["sync", "--path", project_config.to_str().unwrap()],
        &envs,
    );
    assert!(collision.status.success());
    assert!(String::from_utf8_lossy(&collision.stderr).contains("conflict"));
    assert_eq!(fs::read_to_string(&pi_target).unwrap(), "unmanaged");
    let status = run(
        &project,
        &[
            "status",
            "--path",
            project_config.to_str().unwrap(),
            "--json",
        ],
        &envs,
    );
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("conflict"));

    fs::remove_file(&pi_target).unwrap();
    let prune_without_sync = run(
        &project,
        &["prune", "--path", project_config.to_str().unwrap()],
        &envs,
    );
    assert!(prune_without_sync.status.success());
    assert!(
        !pi_target.exists(),
        "prune must not materialize missing desired links"
    );

    let remove = run(
        &project,
        &[
            "remove",
            "--path",
            project_config.to_str().unwrap(),
            "skills:local/source",
        ],
        &envs,
    );
    assert!(
        remove.status.success(),
        "{}",
        String::from_utf8_lossy(&remove.stderr)
    );
    assert!(!fs::read_to_string(&project_config)
        .unwrap()
        .contains("skills:local/source"));
    assert!(!project.join(".agents/skills/demo").exists());
    assert!(!project.join(".pi/skills/demo").exists());
    assert!(!project.join(".claude/skills/demo").exists());

    let global_config_path = global_config.join("config.toml");
    let global_envs = [
        ("HOME", home.as_path()),
        ("MISE_DATA_DIR", data.as_path()),
        ("MISE_STATE_DIR", state.as_path()),
        ("MISE_CONFIG_DIR", global_config.as_path()),
    ];
    let global = run(
        &project,
        &[
            "use",
            "--global",
            &source_arg,
            "demo",
            "--agent",
            "agents",
            "--agent",
            "pi",
            "--agent",
            "claude",
        ],
        &global_envs,
    );
    assert!(
        global.status.success(),
        "{}",
        String::from_utf8_lossy(&global.stderr)
    );
    let global_version = version(&global_config_path);
    install_snapshot(&data, &global_version, &source);
    let global_sync = run(&project, &["sync", "--global"], &global_envs);
    assert!(
        global_sync.status.success(),
        "{}",
        String::from_utf8_lossy(&global_sync.stderr)
    );
    assert!(home.join(".agents/skills/demo").is_symlink());
    assert!(home.join(".pi/agent/skills/demo").is_symlink());
    assert!(home.join(".claude/skills/demo").is_symlink());

    // Direct mise unuse removes the declaration and artifact without invoking
    // the skill CLI. Status reports the dangling owned link and prune repairs it.
    fs::write(&global_config_path, "").unwrap();
    fs::remove_dir_all(data.join("installs/skills-local-source")).unwrap();
    let dangling = run(&project, &["status", "--global", "--json"], &global_envs);
    assert!(dangling.status.success());
    assert!(String::from_utf8_lossy(&dangling.stdout).contains("dangling"));
    let prune = run(&project, &["prune", "--global"], &global_envs);
    assert!(
        prune.status.success(),
        "{}",
        String::from_utf8_lossy(&prune.stderr)
    );
    assert!(!home.join(".agents/skills/demo").exists());
    assert!(!home.join(".pi/agent/skills/demo").exists());
    assert!(!home.join(".claude/skills/demo").exists());
    assert!(!project.join(".agents/skills/demo").exists());
}

#[test]
fn same_revision_subpaths_use_one_complete_artifact_without_cross_activation() {
    let temp = TempDir::new().unwrap();
    let repository = temp.path().join("repository");
    let first_project = temp.path().join("first-project");
    let second_project = temp.path().join("second-project");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let home = temp.path().join("home");
    for directory in [
        &repository,
        &first_project,
        &second_project,
        &data,
        &state,
        &home,
    ] {
        fs::create_dir_all(directory).unwrap();
    }
    for (name, marker) in [("one", "first"), ("two", "second")] {
        let directory = repository.join("skills").join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name}\n---\n"),
        )
        .unwrap();
        fs::write(directory.join("marker.txt"), marker).unwrap();
    }
    Command::new("git")
        .args(["init", "--quiet", repository.to_str().unwrap()])
        .status()
        .unwrap();
    for (key, value) in [
        ("user.email", "test@example.invalid"),
        ("user.name", "test"),
    ] {
        assert!(Command::new("git")
            .args(["-C", repository.to_str().unwrap(), "config", key, value])
            .status()
            .unwrap()
            .success());
    }
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
            "fixture"
        ])
        .status()
        .unwrap()
        .success());
    let revision = String::from_utf8(
        Command::new("git")
            .args(["-C", repository.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    let first_config = first_project.join("mise.toml");
    let second_config = second_project.join("mise.toml");
    let source = format!("git:file://{}", repository.display());
    let envs = [
        ("HOME", home.as_path()),
        ("MISE_DATA_DIR", data.as_path()),
        ("MISE_STATE_DIR", state.as_path()),
    ];
    for (project, config, subpath, selector) in [
        (&first_project, &first_config, "skills/one", "one"),
        (&second_project, &second_config, "skills/two", "two"),
    ] {
        let output = run(
            project,
            &[
                "use",
                "--path",
                config.to_str().unwrap(),
                &source,
                selector,
                "--subpath",
                subpath,
                "--agent",
                "pi",
            ],
            &envs,
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(version(&first_config), format!("rev:{revision}"));
    assert_eq!(version(&second_config), format!("rev:{revision}"));

    // Simulate mise's source+version artifact: it contains both subtrees,
    // regardless of which declaration requested installation first.
    let artifact = data
        .join("installs")
        .join("skills-local-repository")
        .join(&revision);
    copy_directory(&repository, &artifact);

    for (project, config, skill) in [
        (&first_project, &first_config, "one"),
        (&second_project, &second_config, "two"),
    ] {
        let output = run(
            project,
            &["sync", "--path", config.to_str().unwrap(), "--locked"],
            &envs,
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let target = project.join(format!(".pi/skills/{skill}"));
        assert_eq!(
            fs::canonicalize(&target).unwrap(),
            fs::canonicalize(artifact.join("skills").join(skill)).unwrap()
        );
        assert_eq!(
            fs::read_to_string(target.join("marker.txt")).unwrap(),
            if skill == "one" { "first" } else { "second" }
        );
    }
}

#[test]
fn remove_uninstalls_only_after_the_last_declaration_reference() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let project = temp.path().join("project");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let home = temp.path().join("home");
    let global_config = temp.path().join("global-config");
    for directory in [&source, &project, &data, &state, &home, &global_config] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "one");
    skill(&source, "two");
    let config = project.join("mise.toml");
    let source_arg = format!("path:{}", source.display());
    let envs = [
        ("HOME", home.as_path()),
        ("MISE_DATA_DIR", data.as_path()),
        ("MISE_STATE_DIR", state.as_path()),
        ("MISE_CONFIG_DIR", global_config.as_path()),
    ];
    let output = run(
        &project,
        &[
            "use",
            "--path",
            config.to_str().unwrap(),
            &source_arg,
            "one",
            "two",
            "--agent",
            "pi",
        ],
        &envs,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pinned = version(&config);
    let artifact = install_snapshot(&data, &pinned, &source);
    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(output.status.success());

    let output = run(
        &project,
        &[
            "remove",
            "--path",
            config.to_str().unwrap(),
            "skills:local/source",
            "one",
        ],
        &envs,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        artifact.is_dir(),
        "partial removal must retain the artifact"
    );
    assert!(project.join(".pi/skills/two").is_symlink());
    assert!(!project.join(".pi/skills/one").exists());

    let global_output = run(
        &project,
        &["use", "--global", &source_arg, "two", "--agent", "pi"],
        &envs,
    );
    assert!(
        global_output.status.success(),
        "{}",
        String::from_utf8_lossy(&global_output.stderr)
    );
    let output = run(
        &project,
        &[
            "remove",
            "--path",
            config.to_str().unwrap(),
            "skills:local/source",
        ],
        &envs,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        artifact.exists(),
        "a global declaration must retain the shared artifact"
    );

    let output = run(
        &project,
        &["remove", "--global", "skills:local/source"],
        &envs,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !artifact.exists(),
        "the final scoped removal should delegate artifact cleanup to mise"
    );
}

#[test]
fn locked_sync_rejects_local_source_drift() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let project = temp.path().join("project");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let home = temp.path().join("home");
    for directory in [&source, &project, &data, &state, &home] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    let config = project.join("mise.toml");
    let source_arg = format!("path:{}", source.display());
    let envs = [
        ("HOME", home.as_path()),
        ("MISE_DATA_DIR", data.as_path()),
        ("MISE_STATE_DIR", state.as_path()),
    ];
    let output = run(
        &project,
        &[
            "use",
            "--path",
            config.to_str().unwrap(),
            &source_arg,
            "demo",
            "--agent",
            "pi",
        ],
        &envs,
    );
    assert!(output.status.success());
    fs::write(source.join("new.txt"), "drift").unwrap();
    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("local source changed"));
}
