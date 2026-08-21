use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_skills")
}

#[cfg(unix)]
fn fake_mise(root: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join("mise");
    fs::write(
        &path,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_MISE_LOG"
case "$1" in
  --version) echo '2026.6.14 linux';;
  which) exit 1;;
  install) test "${FAKE_MISE_FAIL_INSTALL:-0}" = 1 && exit 9; exit 0;;
  plugins) exit 0;;
  *) exit 0;;
esac
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

#[test]
#[cfg(unix)]
fn install_targets_the_selected_environment_without_unscoped_arguments() {
    let temp = TempDir::new().unwrap();
    let bin_dir = temp.path().join("bin");
    let project = temp.path().join("project");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&project).unwrap();
    let mise = fake_mise(&bin_dir);
    let config = project.join("mise.toml");
    fs::write(project.join("mise.dev.toml"), "[tools]\n").unwrap();
    let log = temp.path().join("mise.log");
    let output = Command::new(binary())
        .current_dir(&project)
        .args([
            "install",
            "--path",
            config.to_str().unwrap(),
            "--env",
            "dev",
        ])
        .env("MISE_BIN", &mise)
        .env("FAKE_MISE_LOG", &log)
        .env("PATH", &bin_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let invocation = fs::read_to_string(log).unwrap();
    assert!(invocation.contains("install --yes --jobs 1 --env dev"));
    assert!(!invocation.contains("skills@"));
    assert!(project.join("mise.dev.toml").is_file());
}

#[test]
#[cfg(unix)]
fn self_update_keeps_old_config_when_normal_mise_install_fails() {
    let temp = TempDir::new().unwrap();
    let bin_dir = temp.path().join("bin");
    let config_dir = temp.path().join("config");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&config_dir).unwrap();
    let mise = fake_mise(&bin_dir);
    let config = config_dir.join("config.toml");
    let old = "# preserve this config\n[tools.\"github:fullmeta-dev/skills\"]\nversion = \"0.0.1\"\nbin = \"skills\"\n";
    fs::write(&config, old).unwrap();
    let manifest = temp.path().join("compat.json");
    fs::write(
        &manifest,
        r#"{"schema_version":1,"latest":"0.2.0","releases":{"0.2.0":{"minimum_mise":"2026.6.14","plugin_url":"https://new/plugin.git","plugin_revision":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}}"#,
    )
    .unwrap();
    let output = Command::new(binary())
        .current_dir(temp.path())
        .args(["self-update"])
        .env("MISE_BIN", &mise)
        .env("MISE_CONFIG_DIR", &config_dir)
        .env("HOME", temp.path().join("home"))
        .env("PATH", &bin_dir)
        .env("FAKE_MISE_LOG", temp.path().join("mise.log"))
        .env("FAKE_MISE_FAIL_INSTALL", "1")
        .env("SKILLS_COMPATIBILITY_MANIFEST", &manifest)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(&config).unwrap(), old);
    assert!(!fs::read_to_string(temp.path().join("mise.log"))
        .unwrap()
        .contains("plugins install"));

    let output = Command::new(binary())
        .current_dir(temp.path())
        .args(["self-update"])
        .env("MISE_BIN", &mise)
        .env("MISE_CONFIG_DIR", &config_dir)
        .env("HOME", temp.path().join("home"))
        .env("PATH", &bin_dir)
        .env("FAKE_MISE_LOG", temp.path().join("mise.log"))
        .env("SKILLS_COMPATIBILITY_MANIFEST", &manifest)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let updated = fs::read_to_string(&config).unwrap();
    assert!(updated.contains("version = \"0.2.0\""));
    assert!(updated.contains("# preserve this config"));
    assert!(!updated.contains("vfox-backend:skills"));
    assert!(!updated.contains("https://new/plugin.git"));
}

#[test]
#[cfg(unix)]
fn detects_foreign_windows_executable_on_portable_path() {
    let temp = TempDir::new().unwrap();
    let bin_dir = temp.path().join("bin");
    let project = temp.path().join("project");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&project).unwrap();
    let mise = fake_mise(&bin_dir);
    fs::write(bin_dir.join("skills.exe"), "foreign").unwrap();
    let config = project.join("mise.toml");
    let output = Command::new(binary())
        .current_dir(&project)
        .args(["install", "--path", config.to_str().unwrap()])
        .env("MISE_BIN", &mise)
        .env("HOME", temp.path().join("home"))
        .env("PATH", &bin_dir)
        .env("FAKE_MISE_LOG", temp.path().join("mise.log"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("PATH owner:"), "{stderr}");
    assert!(stderr.contains("skills.exe"), "{stderr}");
}
