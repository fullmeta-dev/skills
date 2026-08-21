use crate::config::{Config, ToolInfo};
use crate::source::{canonical_version, local_pin, SourceKind, SourceSpec};
use mise_skills::discover_with_subpath;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnedLink {
    #[serde(default)]
    pub tool: String,
    pub adapter: String,
    pub skill: String,
    pub target: String,
    pub link: String,
    pub source: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnershipManifest {
    schema_version: u32,
    config_root: String,
    #[serde(default)]
    links: Vec<OwnedLink>,
}

impl Default for OwnershipManifest {
    fn default() -> Self {
        Self {
            schema_version: 1,
            config_root: String::new(),
            links: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkStatus {
    pub adapter: String,
    pub skill: String,
    pub target: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub linked: usize,
    pub removed: usize,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub links: Vec<LinkStatus>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LinkKey {
    adapter: String,
    target: PathBuf,
}

#[derive(Debug, Clone)]
struct DesiredLink {
    tool: String,
    adapter: Adapter,
    skill: String,
    target: PathBuf,
    link: PathBuf,
    source: String,
    version: String,
}

#[derive(Debug, Clone)]
struct Artifact {
    path: PathBuf,
    has_manifest: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Adapter {
    Agents,
    Pi,
    Claude,
}

impl Adapter {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "agents" | "codex" | "cursor" => Ok(Self::Agents),
            "pi" => Ok(Self::Pi),
            "claude" => Ok(Self::Claude),
            _ => Err(format!(
                "unsupported agent {value:?}; expected agents, codex, cursor, pi, or claude"
            )),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Agents => "agents",
            Self::Pi => "pi",
            Self::Claude => "claude",
        }
    }

    fn root(self, global: bool, config_root: &Path) -> Result<PathBuf, String> {
        if global {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| "cannot determine HOME for global skill links".to_string())?;
            Ok(match self {
                Self::Agents => home.join(".agents").join("skills"),
                Self::Pi => home.join(".pi").join("agent").join("skills"),
                Self::Claude => home.join(".claude").join("skills"),
            })
        } else {
            Ok(match self {
                Self::Agents => config_root.join(".agents").join("skills"),
                Self::Pi => config_root.join(".pi").join("skills"),
                Self::Claude => config_root.join(".claude").join("skills"),
            })
        }
    }
}

pub fn validate_agent(value: &str) -> Result<(), String> {
    Adapter::parse(value).map(|_| ())
}

pub fn sync(config: &Config, global: bool) -> Result<SyncReport, String> {
    let mut manifest = load_manifest(&config.root)?;
    let desired = desired_links(config, global)?;
    let mut report = SyncReport::default();
    let old_links: BTreeMap<LinkKey, OwnedLink> = manifest
        .links
        .iter()
        .cloned()
        .map(|link| {
            (
                LinkKey {
                    adapter: link.adapter.clone(),
                    target: PathBuf::from(&link.target),
                },
                link,
            )
        })
        .collect();
    let mut next_links = BTreeMap::new();

    for (key, old) in &old_links {
        if desired.contains_key(key) {
            continue;
        }
        match remove_owned(old) {
            Ok(true) => report.removed += 1,
            Ok(false) => {}
            Err(conflict) => {
                report.conflicts.push(conflict);
                next_links.insert(key.clone(), old.clone());
            }
        }
    }

    for (key, desired_link) in &desired {
        let old = old_links.get(key);
        match converge_link(desired_link, old) {
            Ok(ConvergeResult::Created) => report.linked += 1,
            Ok(ConvergeResult::Updated) => {
                report.removed += 1;
                report.linked += 1;
            }
            Ok(ConvergeResult::Unchanged) => {}
            Ok(ConvergeResult::Conflict(message)) => {
                report.conflicts.push(message);
                if let Some(old) = old {
                    next_links.insert(key.clone(), old.clone());
                }
                continue;
            }
            Err(error) => return Err(error),
        }
        next_links.insert(key.clone(), desired_link.owned());
    }

    manifest.config_root = config.root.display().to_string();
    manifest.links = next_links.into_values().collect();
    manifest.links.sort_by(|left, right| {
        left.adapter
            .cmp(&right.adapter)
            .then_with(|| left.target.cmp(&right.target))
    });
    save_manifest(&config.root, &manifest).map(|_| report)
}

pub fn prune(config: &Config, global: bool) -> Result<SyncReport, String> {
    let mut manifest = load_manifest(&config.root)?;
    let desired_result = desired_links_report(config, global);
    let declared_tools = crate::config::skill_tools(&config.document)?;
    let desired_keys: BTreeSet<LinkKey> = desired_result.links.keys().cloned().collect();
    let mut report = SyncReport::default();
    let mut next_links = BTreeMap::new();
    for link in manifest.links {
        let key = LinkKey {
            adapter: link.adapter.clone(),
            target: PathBuf::from(&link.target),
        };
        let is_declared =
            !link.tool.is_empty() && declared_tools.iter().any(|tool| tool.key == link.tool);
        let keep_for_unavailable_declaration =
            !desired_result.diagnostics.is_empty() && is_declared;
        if desired_keys.contains(&key) || keep_for_unavailable_declaration {
            next_links.insert(key, link);
            continue;
        }
        match remove_owned(&link) {
            Ok(true) => report.removed += 1,
            Ok(false) => {}
            Err(conflict) => {
                report.conflicts.push(conflict);
                next_links.insert(key, link);
            }
        }
    }
    manifest.links = next_links.into_values().collect();
    manifest.links.sort_by(|left, right| {
        left.adapter
            .cmp(&right.adapter)
            .then_with(|| left.target.cmp(&right.target))
    });
    save_manifest(&config.root, &manifest).map(|_| report)
}

pub fn status(config: &Config, global: bool) -> Result<StatusReport, String> {
    let manifest = load_manifest(&config.root)?;
    let desired_result = desired_links_report(config, global);
    let mut links = Vec::new();
    let mut diagnostics = desired_result.diagnostics;
    let mut seen = BTreeSet::new();

    for link in manifest.links {
        let target = PathBuf::from(&link.target);
        let state = match symlink_target(&target) {
            TargetState::Missing => "missing",
            TargetState::Other => "conflict",
            TargetState::Symlink(actual) => {
                if actual == link.link {
                    if target.exists() {
                        "active"
                    } else {
                        "dangling"
                    }
                } else {
                    "conflict"
                }
            }
        };
        if state == "dangling" {
            diagnostics.push(format!("dangling link: {}", target.display()));
        } else if state == "missing" {
            diagnostics.push(format!("missing link: {}", target.display()));
        } else if state == "conflict" {
            diagnostics.push(format!("conflicting link: {}", target.display()));
        }
        seen.insert(LinkKey {
            adapter: link.adapter.clone(),
            target: target.clone(),
        });
        links.push(LinkStatus {
            adapter: link.adapter,
            skill: link.skill,
            target: link.target,
            state: state.to_string(),
            detail: Some(link.link),
        });
    }

    for (key, desired) in desired_result.links {
        if seen.contains(&key) {
            continue;
        }
        let (state, detail) = match symlink_target(&desired.target) {
            TargetState::Missing => ("missing", None),
            TargetState::Other => (
                "conflict",
                Some("unmanaged target already exists".to_string()),
            ),
            TargetState::Symlink(actual) if actual == desired.link => (
                "unowned",
                Some("link was not recorded in this config root".to_string()),
            ),
            TargetState::Symlink(_) => ("conflict", Some("unmanaged symlink".to_string())),
        };
        if state == "missing" {
            diagnostics.push(format!("missing link: {}", desired.target.display()));
        } else if state == "conflict" {
            diagnostics.push(format!("conflicting target: {}", desired.target.display()));
        }
        links.push(LinkStatus {
            adapter: desired.adapter.id().to_string(),
            skill: desired.skill,
            target: desired.target.display().to_string(),
            state: state.to_string(),
            detail,
        });
    }
    links.sort_by(|left, right| {
        left.adapter
            .cmp(&right.adapter)
            .then_with(|| left.target.cmp(&right.target))
    });
    Ok(StatusReport { links, diagnostics })
}

fn desired_links(config: &Config, global: bool) -> Result<BTreeMap<LinkKey, DesiredLink>, String> {
    let report = desired_links_report(config, global);
    if !report.diagnostics.is_empty() {
        return Err(report.diagnostics.join("; "));
    }
    Ok(report.links)
}

struct DesiredReport {
    links: BTreeMap<LinkKey, DesiredLink>,
    diagnostics: Vec<String>,
}

fn desired_links_report(config: &Config, global: bool) -> DesiredReport {
    let mut links = BTreeMap::new();
    let mut diagnostics = Vec::new();
    let tools = match crate::config::skill_tools(&config.document) {
        Ok(tools) => tools,
        Err(error) => {
            diagnostics.push(error);
            return DesiredReport { links, diagnostics };
        }
    };
    for tool in tools {
        let result = desired_tool_links(config, global, &tool);
        match result {
            Ok(tool_links) => {
                for desired in tool_links {
                    let key = LinkKey {
                        adapter: desired.adapter.id().to_string(),
                        target: desired.target.clone(),
                    };
                    if let Some(existing) = links.get(&key) {
                        if existing.link != desired.link {
                            diagnostics.push(format!(
                                "conflicting declarations for target {}",
                                desired.target.display()
                            ));
                        }
                        continue;
                    }
                    links.insert(key, desired);
                }
            }
            Err(error) => diagnostics.push(error),
        }
    }
    DesiredReport { links, diagnostics }
}

fn desired_tool_links(
    config: &Config,
    global: bool,
    tool: &ToolInfo,
) -> Result<Vec<DesiredLink>, String> {
    let source = SourceSpec::from_declared(&tool.source, &config.root, Some(&tool.key))?;
    let version = canonical_version(&tool.version, source.kind)?;
    if source.kind == SourceKind::Path {
        let actual = local_pin(&source, &tool.subpath)?;
        if actual != version {
            return Err(format!(
                "local source changed for {}: declared {} but found {}; run use or upgrade",
                tool.key, version, actual
            ));
        }
    }
    let artifact = find_artifact(&tool.key, &version)?.ok_or_else(|| {
        format!(
            "missing installed artifact for {}; run mise install",
            tool.key
        )
    })?;
    verify_artifact(&artifact.path, &version, &source)?;
    let subpath =
        (!artifact.has_manifest && !tool.subpath.is_empty()).then(|| Path::new(&tool.subpath));
    let discovery = discover_with_subpath(&artifact.path, subpath)
        .map_err(|error| format!("cannot discover {}: {error}", tool.key))?;
    if discovery.has_errors() {
        return Err(format!(
            "cannot discover {}: source contains errors",
            tool.key
        ));
    }
    let selection = discovery.select(&tool.include, &tool.exclude);
    let mut adapters = BTreeSet::new();
    for agent in &tool.agents {
        adapters.insert(Adapter::parse(agent)?);
    }
    if adapters.is_empty() {
        return Err(format!("skill tool {} has no agents", tool.key));
    }
    let mut result = Vec::new();
    for skill in selection.selected {
        validate_skill_target(&skill.name)?;
        let source_skill = artifact.path.join(&skill.path);
        if !source_skill.is_dir() {
            return Err(format!(
                "selected skill artifact is not a directory: {}",
                skill.path
            ));
        }
        for adapter in &adapters {
            let root = adapter.root(global, &config.root)?;
            let target = root.join(&skill.name);
            let link = relative_path(root.as_path(), &source_skill)?;
            result.push(DesiredLink {
                tool: tool.key.clone(),
                adapter: *adapter,
                skill: skill.name.clone(),
                target,
                link,
                source: source_skill.display().to_string(),
                version: version.clone(),
            });
        }
    }
    Ok(result)
}

fn validate_skill_target(name: &str) -> Result<(), String> {
    let path = Path::new(name);
    if name.is_empty()
        || path.is_absolute()
        || path.components().count() != 1
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "skill name {name:?} cannot be materialized as an adapter directory"
        ));
    }
    Ok(())
}

impl DesiredLink {
    fn owned(&self) -> OwnedLink {
        OwnedLink {
            tool: self.tool.clone(),
            adapter: self.adapter.id().to_string(),
            skill: self.skill.clone(),
            target: self.target.display().to_string(),
            link: self.link.display().to_string(),
            source: self.source.clone(),
            version: self.version.clone(),
        }
    }
}

enum ConvergeResult {
    Created,
    Updated,
    Unchanged,
    Conflict(String),
}

fn converge_link(desired: &DesiredLink, old: Option<&OwnedLink>) -> Result<ConvergeResult, String> {
    let existing = symlink_target(&desired.target);
    match existing {
        TargetState::Missing => {
            create_link(desired)?;
            Ok(if old.is_some() {
                ConvergeResult::Updated
            } else {
                ConvergeResult::Created
            })
        }
        TargetState::Other => Ok(ConvergeResult::Conflict(format!(
            "conflict at {}: existing unmanaged target was left untouched",
            desired.target.display()
        ))),
        TargetState::Symlink(actual) => {
            if actual == desired.link {
                if old.is_some() {
                    Ok(ConvergeResult::Unchanged)
                } else {
                    Ok(ConvergeResult::Conflict(format!(
                        "conflict at {}: existing unmanaged symlink was left untouched",
                        desired.target.display()
                    )))
                }
            } else if let Some(old) = old {
                if actual == old.link {
                    remove_link(&desired.target)?;
                    create_link(desired)?;
                    Ok(ConvergeResult::Updated)
                } else {
                    Ok(ConvergeResult::Conflict(format!(
                        "conflict at {}: managed link was changed and was left untouched",
                        desired.target.display()
                    )))
                }
            } else {
                Ok(ConvergeResult::Conflict(format!(
                    "conflict at {}: existing unmanaged symlink was left untouched",
                    desired.target.display()
                )))
            }
        }
    }
}

fn remove_owned(link: &OwnedLink) -> Result<bool, String> {
    let target = Path::new(&link.target);
    match symlink_target(target) {
        TargetState::Missing => Ok(true),
        TargetState::Other => Err(format!(
            "conflict at {}: refusing to remove unmanaged target",
            target.display()
        )),
        TargetState::Symlink(actual) if actual == link.link => {
            remove_link(target)?;
            Ok(true)
        }
        TargetState::Symlink(_) => Err(format!(
            "conflict at {}: refusing to remove changed symlink",
            target.display()
        )),
    }
}

enum TargetState {
    Missing,
    Other,
    Symlink(PathBuf),
}

fn symlink_target(path: &Path) -> TargetState {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => match fs::read_link(path) {
            Ok(link) => TargetState::Symlink(link),
            Err(_) => TargetState::Other,
        },
        Ok(_) => TargetState::Other,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => TargetState::Missing,
        Err(_) => TargetState::Other,
    }
}

fn create_link(desired: &DesiredLink) -> Result<(), String> {
    if let Some(parent) = desired.target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create adapter root {}: {error}", parent.display()))?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&desired.link, &desired.target).map_err(|error| {
            format!(
                "cannot create relative skill link {}: {error}",
                desired.target.display()
            )
        })
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(&desired.link, &desired.target).map_err(|error| {
            format!(
                "cannot create relative skill link {}: {error}",
                desired.target.display()
            )
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = desired;
        Err("skill links are unsupported on this platform".to_string())
    }
}

fn remove_link(path: &Path) -> Result<(), String> {
    fs::remove_file(path)
        .map_err(|error| format!("cannot remove skill link {}: {error}", path.display()))
}

fn relative_path(from: &Path, to: &Path) -> Result<PathBuf, String> {
    let from = absolute_path(from)?;
    let to = absolute_path(to)?;
    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    if from_components.first() != to_components.first() {
        return Err(format!(
            "cannot create relative link from {} to {}",
            from.display(),
            to.display()
        ));
    }
    let mut common = 0;
    while common < from_components.len()
        && common < to_components.len()
        && from_components[common] == to_components[common]
    {
        common += 1;
    }
    let mut result = PathBuf::new();
    for component in &from_components[common..] {
        if matches!(component, Component::Normal(_)) {
            result.push("..");
        }
    }
    for component in &to_components[common..] {
        result.push(component.as_os_str());
    }
    if result.as_os_str().is_empty() {
        return Err("cannot create empty relative skill link".to_string());
    }
    Ok(result)
}

fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| error.to_string())
    }
}

fn verify_artifact(path: &Path, version: &str, source: &SourceSpec) -> Result<(), String> {
    let manifest_path = path.join(".mise-skills-manifest.json");
    if !manifest_path.is_file() {
        return Ok(());
    }
    let contents = fs::read_to_string(&manifest_path).map_err(|error| {
        format!(
            "cannot read artifact manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    let manifest: serde_json::Value = serde_json::from_str(&contents).map_err(|error| {
        format!(
            "cannot parse artifact manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    let actual = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .or_else(|| manifest.get("revision").and_then(serde_json::Value::as_str))
        .ok_or_else(|| {
            format!(
                "artifact manifest has no version: {}",
                manifest_path.display()
            )
        })?;
    let actual_version = canonical_version(actual, source.kind)?;
    if actual_version != version {
        return Err(format!(
            "installed artifact version mismatch: declared {version} but found {actual_version}"
        ));
    }
    if let Some(actual_source) = manifest.get("source").and_then(serde_json::Value::as_str) {
        let source_matches = match source.kind {
            SourceKind::Path => {
                let actual_path = actual_source.strip_prefix("path:").unwrap_or(actual_source);
                fs::canonicalize(actual_path)
                    .ok()
                    .zip(source.resolved_path.as_ref())
                    .is_some_and(|(actual, expected)| actual == *expected)
            }
            SourceKind::Github | SourceKind::Git => actual_source == source.canonical,
        };
        if !source_matches {
            return Err(format!(
                "installed artifact source mismatch: declared {} but found {}",
                source.canonical, actual_source
            ));
        }
    }
    Ok(())
}

fn find_artifact(key: &str, version: &str) -> Result<Option<Artifact>, String> {
    let installs = std::env::var_os("MISE_INSTALLS_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("MISE_DATA_DIR").map(|path| PathBuf::from(path).join("installs"))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|path| PathBuf::from(path).join(".local/share/mise/installs"))
        })
        .ok_or_else(|| "cannot determine mise installs directory".to_string())?;
    let tool_name = format!(
        "skills-{}",
        key.strip_prefix("skills:")
            .unwrap_or(key)
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>()
    );
    let mut candidates = Vec::new();
    candidates.push(installs.join(&tool_name).join(version));
    if let Some(raw) = version.strip_prefix("rev:") {
        candidates.push(installs.join(&tool_name).join(raw));
    }
    for candidate in candidates {
        if candidate.is_dir() {
            return Ok(Some(Artifact {
                has_manifest: candidate.join(".mise-skills-manifest.json").is_file(),
                path: fs::canonicalize(candidate).map_err(|error| error.to_string())?,
            }));
        }
    }
    let tool_root = installs.join(&tool_name);
    if tool_root.is_dir() {
        for entry in fs::read_dir(&tool_root).map_err(|error| {
            format!("cannot read mise installs {}: {error}", tool_root.display())
        })? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            if !path.is_dir()
                || !path.join("SKILL.md").is_file()
                    && !path.join(".mise-skills-manifest.json").is_file()
            {
                continue;
            }
            if path.join(".mise-skills-manifest.json").is_file() {
                let contents = fs::read_to_string(path.join(".mise-skills-manifest.json"))
                    .map_err(|error| error.to_string())?;
                let manifest: serde_json::Value =
                    serde_json::from_str(&contents).map_err(|error| error.to_string())?;
                let actual = manifest
                    .get("version")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| manifest.get("revision").and_then(serde_json::Value::as_str));
                if actual.is_some_and(|value| {
                    value == version || value == version.strip_prefix("rev:").unwrap_or(version)
                }) {
                    return Ok(Some(Artifact {
                        path: fs::canonicalize(&path).map_err(|error| error.to_string())?,
                        has_manifest: true,
                    }));
                }
            }
        }
    }
    Ok(None)
}

fn state_manifest_path(config_root: &Path) -> Result<PathBuf, String> {
    let state = std::env::var_os("MISE_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("MISE_DATA_DIR").map(|path| PathBuf::from(path).join("state")))
        .or_else(|| {
            std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/state/mise"))
        })
        .ok_or_else(|| "cannot determine mise state directory".to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(config_root.to_string_lossy().as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    Ok(state.join("skills").join(format!("{hash}.json")))
}

fn load_manifest(config_root: &Path) -> Result<OwnershipManifest, String> {
    let path = state_manifest_path(config_root)?;
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|error| {
            format!(
                "cannot parse ownership manifest {}: {error}",
                path.display()
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(OwnershipManifest {
            config_root: config_root.display().to_string(),
            ..OwnershipManifest::default()
        }),
        Err(error) => Err(format!(
            "cannot read ownership manifest {}: {error}",
            path.display()
        )),
    }
}

fn save_manifest(config_root: &Path, manifest: &OwnershipManifest) -> Result<(), String> {
    let path = state_manifest_path(config_root)?;
    if manifest.links.is_empty() {
        match fs::remove_file(&path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(format!(
                    "cannot remove ownership manifest {}: {error}",
                    path.display()
                ))
            }
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| "ownership manifest has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "cannot create state directory {}: {error}",
            parent.display()
        )
    })?;
    let temporary = path.with_extension("json.tmp");
    let contents = serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
    fs::write(&temporary, format!("{contents}\n")).map_err(|error| {
        format!(
            "cannot write ownership manifest {}: {error}",
            temporary.display()
        )
    })?;
    fs::rename(&temporary, &path).map_err(|error| {
        format!(
            "cannot replace ownership manifest {}: {error}",
            path.display()
        )
    })
}
