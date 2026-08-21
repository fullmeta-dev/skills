use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Path,
    Github,
    Git,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpec {
    pub canonical: String,
    pub key: String,
    pub kind: SourceKind,
    pub value: String,
    pub resolved_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPin {
    pub version: String,
    pub track: String,
}

impl SourceSpec {
    pub fn parse(input: &str, config_root: &Path) -> Result<Self, String> {
        let input = input.trim();
        if input.is_empty() || input.contains('\0') {
            return Err("source must not be empty".into());
        }
        if let Some(value) = input.strip_prefix("path:") {
            return Self::path(value, config_root);
        }
        if let Some(value) = input.strip_prefix("github:") {
            validate_github(value)?;
            return Ok(Self {
                canonical: format!("github:{value}"),
                key: value.to_string(),
                kind: SourceKind::Github,
                value: value.to_string(),
                resolved_path: None,
            });
        }
        if let Some(value) = input.strip_prefix("git:") {
            validate_git(value)?;
            return Ok(Self {
                canonical: format!("git:{value}"),
                key: git_key(value),
                kind: SourceKind::Git,
                value: value.to_string(),
                resolved_path: None,
            });
        }
        if input.starts_with('.') || Path::new(input).is_absolute() {
            return Self::path(input, config_root);
        }
        if looks_like_github(input) {
            validate_github(input)?;
            return Ok(Self {
                canonical: format!("github:{input}"),
                key: input.to_string(),
                kind: SourceKind::Github,
                value: input.to_string(),
                resolved_path: None,
            });
        }
        if input.starts_with("https://")
            || input.starts_with("http://")
            || input.starts_with("ssh://")
            || input.starts_with("file://")
            || looks_like_scp(input)
        {
            validate_git(input)?;
            return Ok(Self {
                canonical: format!("git:{input}"),
                key: git_key(input),
                kind: SourceKind::Git,
                value: input.to_string(),
                resolved_path: None,
            });
        }
        // A filesystem path may be supplied without the explicit `path:`
        // prefix for the common local-project workflow. Only an existing
        // directory is treated this way, so a malformed GitHub shorthand
        // cannot silently become a local declaration.
        if config_root.join(input).is_dir() {
            return Self::path(input, config_root);
        }
        Err("source must be a path, owner/repository, github:owner/repository, or git URL".into())
    }

    fn path(value: &str, config_root: &Path) -> Result<Self, String> {
        if value.is_empty() || value.contains('\0') {
            return Err("path: source cannot be empty".into());
        }
        let path = Path::new(value);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            config_root.join(path)
        };
        let resolved = fs::canonicalize(&resolved).map_err(|error| {
            format!(
                "cannot resolve local source {}: {error}",
                resolved.display()
            )
        })?;
        if !resolved.is_dir() {
            return Err(format!(
                "local source is not a directory: {}",
                resolved.display()
            ));
        }
        let canonical_value = normalize_path(value)?;
        let basename = path
            .file_name()
            .and_then(OsStr::to_str)
            .filter(|value| !value.is_empty() && *value != "." && *value != "..")
            .or_else(|| resolved.file_name().and_then(OsStr::to_str))
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "local source has no usable directory name".to_string())?;
        Ok(Self {
            canonical: format!("path:{canonical_value}"),
            key: format!("local/{basename}"),
            kind: SourceKind::Path,
            value: canonical_value,
            resolved_path: Some(resolved),
        })
    }

    pub fn from_declared(
        source: &str,
        config_root: &Path,
        key: Option<&str>,
    ) -> Result<Self, String> {
        let spec = Self::parse(source, config_root)?;
        if let Some(key) = key {
            let key = key.strip_prefix("skills:").unwrap_or(key);
            if key != spec.key {
                return Err(format!(
                    "tool key 'skills:{key}' does not identify source '{}'",
                    spec.canonical
                ));
            }
        }
        Ok(spec)
    }

    pub fn url(&self) -> String {
        match self.kind {
            SourceKind::Github => {
                let base = std::env::var("MISE_SKILLS_GITHUB_BASE_URL")
                    .unwrap_or_else(|_| "https://github.com".into());
                format!("{}/{}.git", base.trim_end_matches('/'), self.value)
            }
            SourceKind::Git => self.value.clone(),
            SourceKind::Path => unreachable!("local sources do not have a Git URL"),
        }
    }
}

pub fn validate_subpath(value: Option<&str>) -> Result<String, String> {
    let Some(value) = value else {
        return Ok(String::new());
    };
    if value.is_empty() || value.contains('\0') || value.contains('\\') {
        return if value.is_empty() {
            Ok(String::new())
        } else {
            Err("source subpath must be a safe relative path".into())
        };
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err("source subpath must be relative".into());
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::ParentDir => return Err("source subpath may not contain '..'".into()),
            Component::RootDir | Component::Prefix(_) => {
                return Err("source subpath must be relative".into())
            }
        }
    }
    Ok(parts.join("/"))
}

pub fn resolve_git_pin(source: &SourceSpec, reference: Option<&str>) -> Result<GitPin, String> {
    if source.kind == SourceKind::Path {
        return Err("local sources do not have Git revisions".into());
    }
    if let Some(reference) = reference {
        validate_ref(reference)?;
        if is_commit(reference) {
            return Ok(GitPin {
                version: format!("rev:{}", reference.to_ascii_lowercase()),
                track: reference.to_string(),
            });
        }
        let output = run_git(&["ls-remote", &source.url(), reference])?;
        let revision = parse_remote_revision(&output, reference)
            .or_else(|| {
                run_git(&[
                    "ls-remote",
                    &source.url(),
                    &format!("refs/heads/{reference}"),
                ])
                .ok()
                .and_then(|output| parse_remote_revision(&output, reference))
            })
            .ok_or_else(|| {
                format!(
                    "Git ref {reference:?} was not found in {}",
                    source.canonical
                )
            })?;
        return Ok(GitPin {
            version: format!("rev:{revision}"),
            track: reference.to_string(),
        });
    }

    let output = run_git(&["ls-remote", "--symref", &source.url(), "HEAD"])?;
    let track = output
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next() == Some("ref:"))
                .then(|| fields.next())
                .flatten()
                .and_then(|value| value.strip_prefix("refs/heads/"))
        })
        .unwrap_or("main")
        .to_string();
    let revision = output
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let revision = fields.next()?;
            let name = fields.next()?;
            (name == "HEAD" && is_commit(revision)).then_some(revision.to_ascii_lowercase())
        })
        .ok_or_else(|| format!("Git source returned no HEAD revision: {}", source.canonical))?;
    Ok(GitPin {
        version: format!("rev:{revision}"),
        track,
    })
}

pub fn local_pin(source: &SourceSpec, subpath: &str) -> Result<String, String> {
    let root = source
        .resolved_path
        .as_deref()
        .ok_or_else(|| "local source path is unavailable".to_string())?;
    let selected = if subpath.is_empty() {
        root.to_path_buf()
    } else {
        let selected = root.join(subpath);
        let selected = fs::canonicalize(&selected)
            .map_err(|error| format!("cannot resolve source subpath {subpath:?}: {error}"))?;
        if !selected.starts_with(root) {
            return Err("source subpath escapes the source root".into());
        }
        selected
    };
    if !selected.is_dir() {
        return Err(format!("source subpath is not a directory: {subpath}"));
    }
    Ok(format!("tree-{}", tree_digest(&selected)?))
}

pub fn canonical_version(value: &str, kind: SourceKind) -> Result<String, String> {
    match kind {
        SourceKind::Path => {
            let digest = value.strip_prefix("tree-").ok_or_else(|| {
                "local sources require version tree-<64 hexadecimal characters>".to_string()
            })?;
            if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(
                    "local sources require version tree-<64 hexadecimal characters>".into(),
                );
            }
            Ok(format!("tree-{}", digest.to_ascii_lowercase()))
        }
        SourceKind::Github | SourceKind::Git => {
            let revision = value.strip_prefix("rev:").unwrap_or(value);
            if !is_commit(revision) {
                return Err("Git sources require version rev:<full 40-character commit>".into());
            }
            Ok(format!("rev:{}", revision.to_ascii_lowercase()))
        }
    }
}

fn tree_digest(root: &Path) -> Result<String, String> {
    validate_links(root)?;
    let mut entries = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| !should_skip(entry.path()));
    for item in walker {
        let entry = item.map_err(|error| format!("cannot inspect local source: {error}"))?;
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let file_type = entry.file_type();
        let (kind, executable, payload) = if file_type.is_symlink() {
            let target = fs::read_link(entry.path()).map_err(|error| {
                format!("cannot read symlink {}: {error}", entry.path().display())
            })?;
            // The backend's portable shell digest feeds readlink's newline
            // terminated output into the hash; keep local pins byte-for-byte
            // compatible with installation verification.
            let mut payload = target.to_string_lossy().as_bytes().to_vec();
            payload.push(b'\n');
            ("symlink", false, payload)
        } else if file_type.is_dir() {
            ("directory", executable(entry.path())?, Vec::new())
        } else if file_type.is_file() {
            (
                "file",
                executable(entry.path())?,
                fs::read(entry.path()).map_err(|error| {
                    format!(
                        "cannot read local source {}: {error}",
                        entry.path().display()
                    )
                })?,
            )
        } else {
            ("other", executable(entry.path())?, Vec::new())
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
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_links(root: &Path) -> Result<(), String> {
    let root =
        fs::canonicalize(root).map_err(|error| format!("cannot resolve source root: {error}"))?;
    let walker = WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip(entry.path()));
    for item in walker {
        let entry = item.map_err(|error| format!("cannot inspect source links: {error}"))?;
        if !entry.file_type().is_symlink() {
            continue;
        }
        let target = fs::canonicalize(entry.path()).map_err(|error| {
            format!(
                "unsafe or broken source link {}: {error}",
                entry.path().display()
            )
        })?;
        if !target.starts_with(&root) {
            return Err(format!(
                "source link escapes source root: {}",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn executable(path: &Path) -> Result<bool, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(fs::metadata(path)
            .map_err(|error| error.to_string())?
            .permissions()
            .mode()
            & 0o111
            != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(false)
    }
}

fn should_skip(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        matches!(
            name.to_str(),
            Some(".git") | Some(".mise-skills-manifest.json") | Some(".skills-backend")
        )
    })
}

fn run_git(args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            detail
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_remote_revision(output: &str, reference: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let revision = fields.next()?;
        let name = fields.next()?;
        (is_commit(revision)
            && (name == reference
                || name == format!("refs/heads/{reference}")
                || name == format!("refs/tags/{reference}")
                || (reference == "HEAD" && name == "HEAD")))
            .then_some(revision.to_ascii_lowercase())
    })
}

fn validate_ref(reference: &str) -> Result<(), String> {
    if reference.is_empty() || reference.starts_with('-') || reference.contains(['\0', '\n', '\r'])
    {
        return Err("Git ref must be a non-empty safe ref name".into());
    }
    Ok(())
}

fn is_commit(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn looks_like_github(value: &str) -> bool {
    let mut parts = value.split('/');
    parts.next().is_some_and(|part| !part.is_empty())
        && parts.next().is_some_and(|part| !part.is_empty())
        && parts.next().is_none()
        && !value.contains([':', '@', '\\'])
}

fn looks_like_scp(value: &str) -> bool {
    value.split_once(':').is_some_and(|(host, path)| {
        !host.is_empty() && !path.is_empty() && !host.contains('/') && !host.contains(' ')
    })
}

fn validate_github(value: &str) -> Result<(), String> {
    let mut parts = value.split('/');
    let valid_part = |part: Option<&str>| {
        part.is_some_and(|part| {
            !part.is_empty()
                && part.len() <= 100
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    };
    if !valid_part(parts.next()) || !valid_part(parts.next()) || parts.next().is_some() {
        return Err(format!("invalid GitHub source github:{value}"));
    }
    Ok(())
}

fn validate_git(value: &str) -> Result<(), String> {
    if value.is_empty() || value.contains(['\0', '\n', '\r', '#']) || value.starts_with('-') {
        return Err(
            "Git source must be a URL or SCP-style SSH address without a ref fragment".into(),
        );
    }
    let is_url = value
        .split_once("://")
        .is_some_and(|(scheme, rest)| !scheme.is_empty() && !rest.is_empty());
    if !is_url && !looks_like_scp(value) {
        return Err(format!(
            "Git source is not a URL or SCP-style SSH address: {value}"
        ));
    }
    Ok(())
}

fn git_key(value: &str) -> String {
    if let Some(path) = value.strip_prefix("file://") {
        return format!(
            "local/{}",
            Path::new(path)
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("source")
                .trim_end_matches(".git")
        );
    }
    let mut key = value
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(value)
        .to_string();
    if let Some((_, rest)) = key.split_once('@') {
        key = rest.to_string();
    }
    if let Some((host, path)) = key.split_once(':') {
        key = format!("{host}/{path}");
    }
    key = key
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string();
    key
}

fn normalize_path(value: &str) -> Result<String, String> {
    if value.contains('\0') || value.contains('\\') {
        return Err("path: source contains an unsupported path character".into());
    }
    let path = Path::new(value);
    let absolute = path.is_absolute();
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir | Component::RootDir => {}
            Component::Normal(part) => components.push(part.to_string_lossy().to_string()),
            Component::ParentDir => components.push("..".into()),
            Component::Prefix(prefix) => {
                components.push(prefix.as_os_str().to_string_lossy().to_string())
            }
        }
    }
    let normalized = if absolute {
        format!("/{}", components.join("/"))
    } else {
        components.join("/")
    };
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn canonicalizes_shorthand_and_safe_subpaths() {
        let source = SourceSpec::parse("owner/repo", Path::new("/tmp")).unwrap();
        assert_eq!(source.canonical, "github:owner/repo");
        assert_eq!(source.key, "owner/repo");
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("local")).unwrap();
        let local = SourceSpec::parse("./local", temp.path()).unwrap();
        assert_eq!(local.kind, SourceKind::Path);
        assert_eq!(
            validate_subpath(Some("skills/engineering")),
            Ok("skills/engineering".into())
        );
        assert!(validate_subpath(Some("../escape")).is_err());
    }

    #[test]
    fn local_digest_changes_with_content_but_ignores_git() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join(".git")).unwrap();
        fs::write(temp.path().join("skill.txt"), "one").unwrap();
        let source =
            SourceSpec::parse(&format!("path:{}", temp.path().display()), temp.path()).unwrap();
        let first = local_pin(&source, "").unwrap();
        fs::write(temp.path().join(".git").join("ignored"), "ignored").unwrap();
        assert_eq!(first, local_pin(&source, "").unwrap());
        fs::write(temp.path().join("skill.txt"), "two").unwrap();
        assert_ne!(first, local_pin(&source, "").unwrap());
    }

    #[test]
    fn git_key_matches_backend_identity() {
        assert_eq!(
            git_key("https://host.example/org/repo.git"),
            "host.example/org/repo"
        );
        assert_eq!(
            git_key("git@host.example:org/repo.git"),
            "host.example/org/repo"
        );
    }
}
