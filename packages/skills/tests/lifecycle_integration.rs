use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;
use walkdir::WalkDir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_skills")
}

fn run(cwd: &Path, args: &[&str], envs: &[(&str, &Path)]) -> Output {
    run_with(cwd, args, envs, &[])
}

fn run_with(
    cwd: &Path,
    args: &[&str],
    envs: &[(&str, &Path)],
    extra_envs: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(binary());
    command.current_dir(cwd).args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    for (key, value) in extra_envs {
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
    write_manifest(&install, &format!("path:{}", source.display()), version);
    install
}

fn digest_tree(root: &Path) -> String {
    let mut entries = Vec::new();
    for item in WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some(".git")
                    | Some("node_modules")
                    | Some("dist")
                    | Some("build")
                    | Some("__pycache__")
                    | Some(".mise-skills-manifest.json")
                    | Some(".skills-backend")
            )
        })
    {
        let entry = item.unwrap();
        let relative = entry
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let executable = if entry.file_type().is_symlink() {
            false
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                entry.metadata().unwrap().permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                false
            }
        };
        let (kind, payload) = if entry.file_type().is_dir() {
            ("directory", Vec::new())
        } else if entry.file_type().is_symlink() {
            let mut payload = fs::read_link(entry.path())
                .unwrap()
                .to_string_lossy()
                .as_bytes()
                .to_vec();
            payload.push(b'\n');
            ("symlink", payload)
        } else {
            ("file", fs::read(entry.path()).unwrap())
        };
        entries.push((relative, kind, executable, payload));
    }
    entries.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let mut hasher = Sha256::new();
    for (relative, kind, executable, payload) in entries {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(kind.as_bytes());
        hasher.update([0]);
        hasher.update(if executable { b"1" } else { b"0" });
        hasher.update([0]);
        hasher.update(payload.len().to_string().as_bytes());
        hasher.update([0]);
        hasher.update(payload);
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn write_manifest(artifact: &Path, source: &str, version: &str) {
    let mut skills = Vec::new();
    for item in WalkDir::new(artifact)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != ".mise-skills-manifest.json")
    {
        let entry = item.unwrap();
        if entry.file_name() != "SKILL.md" {
            continue;
        }
        let parent = entry.path().parent().unwrap();
        let path = parent.strip_prefix(artifact).unwrap();
        let path = if path.as_os_str().is_empty() {
            ".".to_string()
        } else {
            path.to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/")
        };
        skills.push(json!({"path": path, "tree": digest_tree(parent)}));
    }
    skills.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    let manifest = json!({
        "schema_version": 1,
        "source": source,
        "subpath": ".",
        "version": version,
        "revision": version,
        "tree": digest_tree(artifact),
        "skills": skills,
    });
    fs::write(
        artifact.join(".mise-skills-manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
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

    // --force replaces the unmanaged file and moves the original aside.
    let forced = run(
        &project,
        &[
            "sync",
            "--path",
            project_config.to_str().unwrap(),
            "--force",
        ],
        &envs,
    );
    assert!(
        forced.status.success(),
        "{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(fs::symlink_metadata(&pi_target)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(String::from_utf8_lossy(&forced.stdout).contains("replaced:"));
    let backups: Vec<PathBuf> = fs::read_dir(project.join(".pi/skills"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".skills-replaced-demo"))
        })
        .collect();
    assert_eq!(
        backups.len(),
        1,
        "force must move the conflicting file aside"
    );
    assert_eq!(fs::read_to_string(&backups[0]).unwrap(), "unmanaged");

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
#[cfg(unix)]
fn refuses_adapter_roots_and_intermediate_paths_that_escape_project_root() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(source.join("demo")).unwrap();
    fs::write(
        source.join("demo/SKILL.md"),
        "---\nname: demo\ndescription: test\n---\n",
    )
    .unwrap();
    let envs = [
        ("HOME", temp.path().join("home")),
        ("MISE_DATA_DIR", temp.path().join("data")),
        ("MISE_STATE_DIR", temp.path().join("state")),
    ];
    for (name, root_kind) in [("intermediate", "intermediate"), ("adapter-root", "root")] {
        let project = temp.path().join(name);
        let outside = temp.path().join(format!("{name}-outside"));
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&outside).unwrap();
        if root_kind == "intermediate" {
            symlink(&outside, project.join(".pi")).unwrap();
        } else {
            fs::create_dir_all(project.join(".pi")).unwrap();
            symlink(&outside, project.join(".pi/skills")).unwrap();
        }
        let config = project.join("mise.toml");
        let source_arg = format!("path:{}", source.display());
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
            &envs
                .iter()
                .map(|(key, value)| (*key, value.as_path()))
                .collect::<Vec<_>>(),
        );
        assert!(output.status.success());
        let pinned = version(&config);
        install_snapshot(&envs[1].1, &pinned, &source);
        let output = run(
            &project,
            &["sync", "--path", config.to_str().unwrap(), "--locked"],
            &envs
                .iter()
                .map(|(key, value)| (*key, value.as_path()))
                .collect::<Vec<_>>(),
        );
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("escapes expected root"), "{error}");
        assert!(!outside.join("demo").exists());
    }
}

#[test]
#[cfg(unix)]
fn refuses_global_adapter_path_symlinks_that_escape_home() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let home = temp.path().join("home");
    let config_dir = temp.path().join("config");
    let outside = temp.path().join("outside");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    for directory in [&source, &home, &config_dir, &outside, &data, &state] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::create_dir_all(source.join("demo")).unwrap();
    fs::write(
        source.join("demo/SKILL.md"),
        "---\nname: demo\ndescription: test\n---\n",
    )
    .unwrap();
    symlink(&outside, home.join(".agents")).unwrap();
    let config = config_dir.join("config.toml");
    let source_arg = format!("path:{}", source.display());
    let envs = [
        ("HOME", home.as_path()),
        ("MISE_CONFIG_DIR", config_dir.as_path()),
        ("MISE_DATA_DIR", data.as_path()),
        ("MISE_STATE_DIR", state.as_path()),
    ];
    let output = run(
        temp.path(),
        &["use", "--global", &source_arg, "demo", "--agent", "agents"],
        &envs,
    );
    assert!(output.status.success());
    install_snapshot(&data, &version(&config), &source);
    let output = run(temp.path(), &["sync", "--global", "--locked"], &envs);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("escapes expected root"), "{error}");
    assert!(!outside.join("skills/demo").exists());
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
        .join(format!("rev-{revision}"));
    copy_directory(&repository, &artifact);
    write_manifest(
        &artifact,
        &format!("git:file://{}", repository.display()),
        &format!("rev:{revision}"),
    );

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
fn manifest_persistence_failure_rolls_back_sync_and_prune() {
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
    let first_version = version(&config);
    let first_artifact = install_snapshot(&data, &first_version, &source);
    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(output.status.success());
    let demo_target = project.join(".pi/skills/demo");
    let manifest_path = fs::read_dir(state.join("skills"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let original_manifest = fs::read(&manifest_path).unwrap();

    skill(&source, "two");
    let output = run(
        &project,
        &[
            "use",
            "--path",
            config.to_str().unwrap(),
            &source_arg,
            "demo",
            "two",
            "--agent",
            "pi",
        ],
        &envs,
    );
    assert!(output.status.success());
    let second_version = version(&config);
    let second_artifact = install_snapshot(&data, &second_version, &source);
    let output = run_with(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
        &[("MISE_SKILLS_FAIL_MANIFEST_WRITE", "1")],
    );
    assert!(!output.status.success());
    assert_eq!(
        fs::canonicalize(&demo_target).unwrap(),
        fs::canonicalize(first_artifact.join("demo")).unwrap()
    );
    assert!(!project.join(".pi/skills/two").exists());
    assert_eq!(fs::read(&manifest_path).unwrap(), original_manifest);
    assert!(!fs::canonicalize(&demo_target)
        .unwrap()
        .starts_with(&second_artifact));

    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(output.status.success());
    fs::write(&config, "").unwrap();
    let manifest_after_sync = fs::read(&manifest_path).unwrap();
    let output = run_with(
        &project,
        &["prune", "--path", config.to_str().unwrap()],
        &envs,
        &[("MISE_SKILLS_FAIL_MANIFEST_WRITE", "1")],
    );
    assert!(!output.status.success());
    assert!(demo_target.is_symlink());
    assert!(project.join(".pi/skills/two").is_symlink());
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest_after_sync);

    let output = run(
        &project,
        &["prune", "--path", config.to_str().unwrap()],
        &envs,
    );
    assert!(output.status.success());
    assert!(!demo_target.exists());
    assert!(!project.join(".pi/skills/two").exists());
    assert!(!manifest_path.exists());
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

fn git_init(root: &Path) {
    let run_git = |args: &[&str]| {
        let status = Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    };
    run_git(&["init", "-q"]);
    run_git(&["config", "user.email", "test@example.invalid"]);
    run_git(&["config", "user.name", "Test"]);
}

fn git_status(root: &Path) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(["status", "--porcelain"])
        .output()
        .expect("run git status");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn project_sync_hides_generated_links_from_git_status() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    git_init(&project);
    fs::write(project.join("tracked.txt"), "keep\n").unwrap();
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
            "agents",
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
    let pinned = version(&config);
    install_snapshot(&data, &pinned, &source);
    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let exclude_path = project.join(".git/info/exclude");
    let exclude = fs::read_to_string(&exclude_path).unwrap();
    for entry in [
        "/.agents/skills/demo",
        "/.pi/skills/demo",
        "/.claude/skills/demo",
    ] {
        assert!(
            exclude.contains(entry),
            "exclude missing {entry}: {exclude}"
        );
    }
    let status = git_status(&project);
    for name in [".agents/skills", ".pi/skills", ".claude/skills"] {
        assert!(!status.contains(name), "git status lists {name}: {status}");
    }

    let repeat = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(repeat.status.success());
    assert_eq!(
        fs::read_to_string(&exclude_path).unwrap(),
        exclude,
        "the exclude block must be idempotent"
    );

    let remove = run(
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
        remove.status.success(),
        "{}",
        String::from_utf8_lossy(&remove.stderr)
    );
    let exclude = fs::read_to_string(&exclude_path).unwrap_or_default();
    for name in ["/.agents/skills", "/.pi/skills", "/.claude/skills"] {
        assert!(
            !exclude.contains(name),
            "exclude block not cleared for {name}: {exclude}"
        );
    }
}

#[test]
fn sync_leaves_unmanaged_conflicts_visible_in_git_status() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    git_init(&project);
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
    let pinned = version(&config);
    install_snapshot(&data, &pinned, &source);
    fs::create_dir_all(project.join(".pi/skills")).unwrap();
    fs::write(project.join(".pi/skills/demo"), "mine\n").unwrap();

    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflict"));
    let exclude = fs::read_to_string(project.join(".git/info/exclude")).unwrap_or_default();
    assert!(
        !exclude.contains("/.pi/skills/demo"),
        "an unmanaged conflict must not be ignored: {exclude}"
    );
    assert!(
        git_status(&project).contains(".pi/"),
        "an unmanaged conflict must stay visible to git"
    );
}

#[test]
#[cfg(unix)]
fn global_sync_does_not_touch_a_home_repository_exclude() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let config_dir = temp.path().join("config");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &config_dir, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    git_init(&home);
    let config = config_dir.join("config.toml");
    let source_arg = format!("path:{}", source.display());
    let envs = [
        ("HOME", home.as_path()),
        ("MISE_DATA_DIR", data.as_path()),
        ("MISE_STATE_DIR", state.as_path()),
        ("MISE_CONFIG_DIR", config_dir.as_path()),
    ];
    let output = run(
        &home,
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
        &envs,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pinned = version(&config);
    install_snapshot(&data, &pinned, &source);
    let output = run(&home, &["sync", "--global"], &envs);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(home.join(".agents/skills/demo").is_symlink());
    let exclude = fs::read_to_string(home.join(".git/info/exclude")).unwrap_or_default();
    assert!(
        !exclude.contains("# >>> skills"),
        "global sync must not write a home repository exclude: {exclude}"
    );
}

#[test]
fn manifest_persistence_failure_leaves_git_exclude_unchanged() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    git_init(&project);
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
    let first_version = version(&config);
    install_snapshot(&data, &first_version, &source);
    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(output.status.success());
    let exclude_path = project.join(".git/info/exclude");
    let original_exclude = fs::read(&exclude_path).unwrap();

    skill(&source, "two");
    let output = run(
        &project,
        &[
            "use",
            "--path",
            config.to_str().unwrap(),
            &source_arg,
            "demo",
            "two",
            "--agent",
            "pi",
        ],
        &envs,
    );
    assert!(output.status.success());
    let second_version = version(&config);
    install_snapshot(&data, &second_version, &source);
    let output = run_with(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
        &[("MISE_SKILLS_FAIL_MANIFEST_WRITE", "1")],
    );
    assert!(!output.status.success());
    assert_eq!(
        fs::read(&exclude_path).unwrap(),
        original_exclude,
        "a failed sync must leave the git exclude unchanged"
    );
}

#[test]
fn replacing_a_managed_link_with_handwritten_content_restores_visibility() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    git_init(&project);
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
    let pinned = version(&config);
    install_snapshot(&data, &pinned, &source);
    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(output.status.success());
    let target = project.join(".pi/skills/demo");
    assert!(target.is_symlink());

    // Replace the managed link with handwritten content and sync again.
    fs::remove_file(&target).unwrap();
    fs::write(&target, "handwritten\n").unwrap();
    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflict"));
    assert_eq!(fs::read_to_string(&target).unwrap(), "handwritten\n");
    let exclude = fs::read_to_string(project.join(".git/info/exclude")).unwrap_or_default();
    assert!(
        !exclude.contains("/.pi/skills/demo"),
        "handwritten content must not stay ignored: {exclude}"
    );
    assert!(
        git_status(&project).contains(".pi/"),
        "handwritten content must be visible to git"
    );
}

#[test]
fn a_failed_git_exclude_cleanup_is_retried_on_the_next_sync() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    git_init(&project);
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
    let pinned = version(&config);
    install_snapshot(&data, &pinned, &source);
    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--locked"],
        &envs,
    );
    assert!(output.status.success());
    let exclude_path = project.join(".git/info/exclude");
    assert!(fs::read_to_string(&exclude_path)
        .unwrap()
        .contains("/.pi/skills/demo"));

    // Remove the declaration while exclude cleanup fails.
    let output = run_with(
        &project,
        &[
            "remove",
            "--path",
            config.to_str().unwrap(),
            "skills:local/source",
        ],
        &envs,
        &[("MISE_SKILLS_FAIL_GIT_EXCLUDE", "1")],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("git exclude"));
    assert!(
        fs::read_to_string(&exclude_path)
            .unwrap()
            .contains("/.pi/skills/demo"),
        "the block persists after a failed cleanup"
    );

    // A later sync retries the recorded root and clears the stale block.
    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap()],
        &envs,
    );
    assert!(output.status.success());
    let exclude = fs::read_to_string(&exclude_path).unwrap_or_default();
    assert!(
        !exclude.contains("/.pi/skills/demo"),
        "the retried cleanup must clear the stale block: {exclude}"
    );
}

#[test]
fn use_accepts_flags_before_the_source_argument() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    let config = project.join("mise.toml");
    let envs = [("HOME", home.as_path())];
    let output = run(
        &project,
        &[
            "use",
            "--path",
            config.to_str().unwrap(),
            "--agent",
            "agents",
            &format!("path:{}", source.display()),
        ],
        &envs,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fs::read_to_string(&config)
        .unwrap()
        .contains("skills:local/source"));
}

#[test]
fn unselected_source_errors_warn_without_blocking_activation() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "good");
    let broken = source.join("broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("SKILL.md"), "---\nname: [\n---\n").unwrap();
    let config = project.join("mise.toml");
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
            &format!("path:{}", source.display()),
            "good",
            "--agent",
            "agents",
            "--allow-source-errors",
        ],
        &envs,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pinned = version(&config);
    install_snapshot(&data, &pinned, &source);

    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap()],
        &envs,
    );
    assert!(
        output.status.success(),
        "unselected errors must not block activation: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unselected source error"),
        "the tolerated error must still be reported: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.join(".agents/skills/good").is_symlink());
}

#[test]
fn force_replaces_an_unmanaged_directory_and_reports_the_backup() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    let config = project.join("mise.toml");
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
            &format!("path:{}", source.display()),
            "demo",
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
    install_snapshot(&data, &pinned, &source);

    let target = project.join(".pi/skills/demo");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("SKILL.md"), "foreign").unwrap();

    let conflict = run(
        &project,
        &["sync", "--path", config.to_str().unwrap()],
        &envs,
    );
    assert!(conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("conflict"));
    assert!(target.is_dir() && !target.is_symlink());

    let forced = run(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--force"],
        &envs,
    );
    assert!(
        forced.status.success(),
        "{}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(fs::symlink_metadata(&target)
        .unwrap()
        .file_type()
        .is_symlink());
    let backups: Vec<PathBuf> = fs::read_dir(project.join(".pi/skills"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".skills-replaced-demo"))
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        fs::read_to_string(backups[0].join("SKILL.md")).unwrap(),
        "foreign"
    );
}

#[test]
fn selected_source_errors_still_block_activation() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    let broken = source.join("broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("SKILL.md"), "---\nname: [\n---\n").unwrap();
    let config = project.join("mise.toml");
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
            &format!("path:{}", source.display()),
            "broken",
            "--agent",
            "agents",
        ],
        &envs,
    );
    assert!(output.status.success());
    let pinned = version(&config);
    install_snapshot(&data, &pinned, &source);

    let output = run(
        &project,
        &["sync", "--path", config.to_str().unwrap()],
        &envs,
    );
    assert!(
        !output.status.success(),
        "a selected malformed skill must still block activation"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("source contains errors"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn forced_replacement_is_rolled_back_when_sync_fails() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    let config = project.join("mise.toml");
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
            &format!("path:{}", source.display()),
            "demo",
            "--agent",
            "pi",
        ],
        &envs,
    );
    assert!(output.status.success());
    let pinned = version(&config);
    install_snapshot(&data, &pinned, &source);
    let target = project.join(".pi/skills/demo");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("SKILL.md"), "foreign").unwrap();

    let output = run_with(
        &project,
        &["sync", "--path", config.to_str().unwrap(), "--force"],
        &envs,
        &[("MISE_SKILLS_FAIL_MANIFEST_WRITE", "1")],
    );
    assert!(!output.status.success());
    assert!(
        target.is_dir() && !target.is_symlink(),
        "a failed forced sync must restore the displaced directory"
    );
    assert_eq!(
        fs::read_to_string(target.join("SKILL.md")).unwrap(),
        "foreign"
    );
}

#[test]
fn ambient_mise_env_does_not_retarget_explicit_scope() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let state = temp.path().join("state");
    let project = temp.path().join("project");
    let source = temp.path().join("source");
    for directory in [&home, &data, &state, &project, &source] {
        fs::create_dir_all(directory).unwrap();
    }
    skill(&source, "demo");
    let config = project.join("mise.toml");
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
            &format!("path:{}", source.display()),
            "demo",
            "--agent",
            "pi",
        ],
        &envs,
    );
    assert!(output.status.success());

    // An ambient MISE_ENV must not make --path read mise.<env>.toml.
    let output = run_with(
        &project,
        &["status", "--path", config.to_str().unwrap(), "--json"],
        &envs,
        &[("MISE_ENV", "tailscale")],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("skills:local/source"),
        "an ambient MISE_ENV retargeted --path: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    // An explicit --env still selects the environment-specific file.
    let output = run_with(
        &project,
        &[
            "status",
            "--path",
            config.to_str().unwrap(),
            "--env",
            "dev",
            "--json",
        ],
        &envs,
        &[("MISE_ENV", "tailscale")],
    );
    assert!(output.status.success());
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("skills:local/source"),
        "an explicit --env must select mise.dev.toml"
    );

    // The same holds for --global: an ambient MISE_ENV must not redirect it to
    // config.<env>.toml, which is not where global declarations live.
    let output = run(
        &project,
        &[
            "use",
            "--global",
            &format!("path:{}", source.display()),
            "demo",
            "--agent",
            "pi",
        ],
        &envs,
    );
    assert!(output.status.success());
    let output = run_with(
        &project,
        &["status", "--global", "--json"],
        &envs,
        &[("MISE_ENV", "tailscale")],
    );
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("skills:local/source"),
        "an ambient MISE_ENV retargeted --global: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
