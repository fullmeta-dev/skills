use crate::config::{Config, ToolInfo};
use crate::source::{canonical_version, local_pin, validate_subpath, SourceKind, SourceSpec};
use fslock::LockFile;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use skills::{discover_with_subpath, Diagnostic, DiagnosticLevel, Skill};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    links: Vec<OwnedLink>,
    /// Canonical git-exclude roots this config has touched. Recorded so a sync
    /// whose exclude cleanup failed can clear the block on a later run, even
    /// after every link has been removed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    git_exclude_roots: Vec<String>,
}

impl Default for OwnershipManifest {
    fn default() -> Self {
        Self {
            schema_version: 1,
            config_root: String::new(),
            links: Vec::new(),
            git_exclude_roots: Vec::new(),
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
    pub replaced: Vec<String>,
    pub conflicts: Vec<String>,
    pub warnings: Vec<String>,
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
    root: PathBuf,
    boundary: PathBuf,
    source: String,
    version: String,
}

#[derive(Debug, Clone)]
struct Artifact {
    path: PathBuf,
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

    fn boundary(self, global: bool, config_root: &Path) -> Result<PathBuf, String> {
        if global {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| "cannot determine HOME for global skill links".to_string())?;
            fs::canonicalize(&home).map_err(|error| {
                format!(
                    "cannot resolve HOME for global skill links {}: {error}",
                    home.display()
                )
            })
        } else {
            Ok(config_root.to_path_buf())
        }
    }

    fn root(self, global: bool, config_root: &Path) -> Result<PathBuf, String> {
        let base = self.boundary(global, config_root)?;
        Ok(match self {
            Self::Agents => base.join(".agents").join("skills"),
            Self::Pi if global => base.join(".pi").join("agent").join("skills"),
            Self::Pi => base.join(".pi").join("skills"),
            Self::Claude => base.join(".claude").join("skills"),
        })
    }
}

pub fn validate_agent(value: &str) -> Result<(), String> {
    Adapter::parse(value).map(|_| ())
}

pub fn sync(config: &Config, global: bool, force: bool) -> Result<SyncReport, String> {
    // Serialize this config root's lifecycle so concurrent syncs cannot
    // interleave link, manifest, and git-exclude writes. Released on return.
    let _lock = lock_config_root(&config.root)?;
    let manifest = load_manifest(&config.root, global)?;
    let (desired, warnings) = desired_links(config, global)?;
    let snapshots = link_snapshots(&manifest, &desired);
    let manifest_snapshot = snapshot_manifest_file(&config.root)?;
    match sync_changes(config, global, manifest, desired, force, warnings) {
        Ok(report) => Ok(report),
        Err(error) => Err(rollback_error(error, &snapshots, &manifest_snapshot)),
    }
}

fn sync_changes(
    config: &Config,
    global: bool,
    mut manifest: OwnershipManifest,
    desired: BTreeMap<LinkKey, DesiredLink>,
    force: bool,
    warnings: Vec<String>,
) -> Result<SyncReport, String> {
    let mut report = SyncReport::default();
    report.warnings = warnings;
    let previous = manifest.links.clone();
    let previous_roots: Vec<PathBuf> = manifest
        .git_exclude_roots
        .iter()
        .map(PathBuf::from)
        .collect();
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
        match converge_link(desired_link, old, force) {
            Ok(ConvergeResult::Created) => report.linked += 1,
            Ok(ConvergeResult::Updated) => {
                report.removed += 1;
                report.linked += 1;
            }
            Ok(ConvergeResult::Replaced(message)) => {
                report.linked += 1;
                report.replaced.push(message);
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
    save_manifest(&config.root, &manifest)?;
    let next: Vec<OwnedLink> = manifest.links.clone();
    let roots = exclude_roots(global, &previous_roots, &previous, &next);
    manifest.git_exclude_roots = roots
        .iter()
        .map(|root| root.display().to_string())
        .collect();
    save_manifest(&config.root, &manifest)?;
    match update_git_excludes(global, &roots, &next) {
        Ok(()) => {
            if !manifest.git_exclude_roots.is_empty() {
                manifest.git_exclude_roots.clear();
                if let Err(error) = save_manifest(&config.root, &manifest) {
                    report.warnings.push(error);
                }
            }
        }
        Err(error) => report.warnings.push(error),
    }
    Ok(report)
}

pub fn prune(config: &Config, global: bool) -> Result<SyncReport, String> {
    // Serialize this config root's lifecycle; see `sync`.
    let _lock = lock_config_root(&config.root)?;
    let manifest = load_manifest(&config.root, global)?;
    let desired_result = desired_links_report(config, global);
    let declared_tools = crate::config::skill_tools(&config.document)?;
    let desired_keys: BTreeSet<LinkKey> = desired_result.links.keys().cloned().collect();
    let snapshots = link_snapshots(&manifest, &desired_result.links);
    let manifest_snapshot = snapshot_manifest_file(&config.root)?;
    match prune_changes(
        config,
        global,
        manifest,
        desired_result,
        declared_tools,
        desired_keys,
    ) {
        Ok(report) => Ok(report),
        Err(error) => Err(rollback_error(error, &snapshots, &manifest_snapshot)),
    }
}

fn prune_changes(
    config: &Config,
    global: bool,
    mut manifest: OwnershipManifest,
    desired_result: DesiredReport,
    declared_tools: Vec<ToolInfo>,
    desired_keys: BTreeSet<LinkKey>,
) -> Result<SyncReport, String> {
    let mut report = SyncReport::default();
    report.warnings = desired_result.warnings.clone();
    let previous = manifest.links.clone();
    let previous_roots: Vec<PathBuf> = manifest
        .git_exclude_roots
        .iter()
        .map(PathBuf::from)
        .collect();
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
    save_manifest(&config.root, &manifest)?;
    let next: Vec<OwnedLink> = manifest.links.clone();
    let roots = exclude_roots(global, &previous_roots, &previous, &next);
    manifest.git_exclude_roots = roots
        .iter()
        .map(|root| root.display().to_string())
        .collect();
    save_manifest(&config.root, &manifest)?;
    match update_git_excludes(global, &roots, &next) {
        Ok(()) => {
            if !manifest.git_exclude_roots.is_empty() {
                manifest.git_exclude_roots.clear();
                if let Err(error) = save_manifest(&config.root, &manifest) {
                    report.warnings.push(error);
                }
            }
        }
        Err(error) => report.warnings.push(error),
    }
    Ok(report)
}

pub fn status(config: &Config, global: bool) -> Result<StatusReport, String> {
    let manifest = load_manifest(&config.root, global)?;
    let desired_result = desired_links_report(config, global);
    let mut links = Vec::new();
    let mut diagnostics = desired_result.diagnostics;
    diagnostics.extend(desired_result.warnings);
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

#[derive(Debug)]
struct LinkSnapshot {
    target: PathBuf,
    state: SnapshotState,
}

#[derive(Debug)]
enum SnapshotState {
    Missing,
    Symlink(PathBuf),
    Other,
}

#[derive(Debug)]
struct ManifestFileSnapshot {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

fn link_snapshots(
    manifest: &OwnershipManifest,
    desired: &BTreeMap<LinkKey, DesiredLink>,
) -> Vec<LinkSnapshot> {
    let mut targets = BTreeSet::new();
    targets.extend(
        manifest
            .links
            .iter()
            .map(|link| PathBuf::from(&link.target)),
    );
    targets.extend(desired.values().map(|link| link.target.clone()));
    targets
        .into_iter()
        .map(|target| {
            let state = match symlink_target(&target) {
                TargetState::Missing => SnapshotState::Missing,
                TargetState::Other => SnapshotState::Other,
                TargetState::Symlink(link) => SnapshotState::Symlink(link),
            };
            LinkSnapshot { target, state }
        })
        .collect()
}

fn snapshot_manifest_file(config_root: &Path) -> Result<ManifestFileSnapshot, String> {
    let path = state_manifest_path(config_root)?;
    let contents = match fs::read(&path) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "cannot snapshot ownership manifest {}: {error}",
                path.display()
            ))
        }
    };
    Ok(ManifestFileSnapshot { path, contents })
}

fn rollback_error(
    error: String,
    snapshots: &[LinkSnapshot],
    manifest_snapshot: &ManifestFileSnapshot,
) -> String {
    let mut rollback_errors = Vec::new();
    if let Err(rollback) = restore_links(snapshots) {
        rollback_errors.push(rollback);
    }
    if let Err(rollback) = restore_manifest_file(manifest_snapshot) {
        rollback_errors.push(rollback);
    }
    if rollback_errors.is_empty() {
        error
    } else {
        format!("{error}; rollback failed: {}", rollback_errors.join("; "))
    }
}

fn restore_links(snapshots: &[LinkSnapshot]) -> Result<(), String> {
    let mut errors = Vec::new();
    for snapshot in snapshots {
        let result = match &snapshot.state {
            SnapshotState::Missing => match symlink_target(&snapshot.target) {
                TargetState::Missing => Ok(()),
                TargetState::Symlink(_) => remove_link(&snapshot.target),
                TargetState::Other => Err(format!(
                    "cannot restore missing link {}; target became unmanaged",
                    snapshot.target.display()
                )),
            },
            SnapshotState::Symlink(link) => match symlink_target(&snapshot.target) {
                TargetState::Missing => create_symlink(link, &snapshot.target),
                TargetState::Symlink(actual) if actual == *link => Ok(()),
                TargetState::Symlink(_) => {
                    remove_link(&snapshot.target)?;
                    create_symlink(link, &snapshot.target)
                }
                TargetState::Other => Err(format!(
                    "cannot restore managed link {}; target became unmanaged",
                    snapshot.target.display()
                )),
            },
            SnapshotState::Other => Ok(()),
        };
        if let Err(error) = result {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn restore_manifest_file(snapshot: &ManifestFileSnapshot) -> Result<(), String> {
    match &snapshot.contents {
        Some(contents) => {
            let parent = snapshot
                .path
                .parent()
                .ok_or_else(|| "ownership manifest has no parent".to_string())?;
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "cannot restore state directory {}: {error}",
                    parent.display()
                )
            })?;
            fs::write(&snapshot.path, contents).map_err(|error| {
                format!(
                    "cannot restore ownership manifest {}: {error}",
                    snapshot.path.display()
                )
            })
        }
        None => match fs::remove_file(&snapshot.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "cannot remove failed ownership manifest {}: {error}",
                snapshot.path.display()
            )),
        },
    }
}

/// The canonical adapter roots whose git-exclude blocks this config may need to
/// write or clear: previously recorded roots, plus the parents of every link it
/// owned before and owns now. Global links live under HOME and are never
/// excluded.
fn exclude_roots(
    global: bool,
    recorded: &[PathBuf],
    previous: &[OwnedLink],
    next: &[OwnedLink],
) -> BTreeSet<PathBuf> {
    if global {
        return BTreeSet::new();
    }
    let mut roots: BTreeSet<PathBuf> = recorded.iter().cloned().collect();
    for link in previous.iter().chain(next.iter()) {
        if let Some(root) = Path::new(&link.target).parent() {
            roots.insert(canonical_root(root));
        }
    }
    roots
}

/// Rewrite the clone-local git-exclude blocks for `roots`. A name is excluded
/// only while its target is still the owned symlink, so a link replaced by
/// handwritten content stays visible. Roots with no owned names get an empty
/// set, which clears their block.
fn update_git_excludes(
    global: bool,
    roots: &BTreeSet<PathBuf>,
    next: &[OwnedLink],
) -> Result<(), String> {
    if global {
        return Ok(());
    }
    let mut names: BTreeMap<PathBuf, BTreeSet<String>> = roots
        .iter()
        .cloned()
        .map(|root| (root, BTreeSet::new()))
        .collect();
    for link in next {
        let target = Path::new(&link.target);
        let ours = matches!(
            symlink_target(target),
            TargetState::Symlink(actual) if actual == Path::new(&link.link)
        );
        if !ours {
            continue;
        }
        if let Some(root) = target.parent() {
            if let Some(name) = target.file_name().and_then(|name| name.to_str()) {
                names
                    .entry(canonical_root(root))
                    .or_default()
                    .insert(name.to_string());
            }
        }
    }
    let mut errors = Vec::new();
    for (root, names) in names {
        let names: Vec<String> = names.into_iter().collect();
        if let Err(error) = crate::git_exclude::exclude_links(&root, &names) {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "could not update git excludes: {}",
            errors.join("; ")
        ))
    }
}

fn canonical_root(root: &Path) -> PathBuf {
    fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

fn desired_links(
    config: &Config,
    global: bool,
) -> Result<(BTreeMap<LinkKey, DesiredLink>, Vec<String>), String> {
    let report = desired_links_report(config, global);
    if !report.diagnostics.is_empty() {
        return Err(report.diagnostics.join("; "));
    }
    Ok((report.links, report.warnings))
}

struct DesiredReport {
    links: BTreeMap<LinkKey, DesiredLink>,
    diagnostics: Vec<String>,
    warnings: Vec<String>,
}

fn desired_links_report(config: &Config, global: bool) -> DesiredReport {
    let mut links = BTreeMap::new();
    let mut diagnostics = Vec::new();
    let mut warnings = Vec::new();
    let tools = match crate::config::skill_tools(&config.document) {
        Ok(tools) => tools,
        Err(error) => {
            diagnostics.push(error);
            return DesiredReport {
                links,
                diagnostics,
                warnings,
            };
        }
    };
    for tool in tools {
        let result = desired_tool_links(config, global, &tool);
        match result {
            Ok(tool_links) => {
                warnings.extend(tool_links.warnings);
                for desired in tool_links.links {
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
    DesiredReport {
        links,
        diagnostics,
        warnings,
    }
}

fn desired_tool_links(config: &Config, global: bool, tool: &ToolInfo) -> Result<ToolLinks, String> {
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
    let subpath = validate_subpath(Some(&tool.subpath))?;
    let artifact = find_artifact(&tool.key, &version)?.ok_or_else(|| {
        format!(
            "missing installed artifact for {}; run mise install",
            tool.key
        )
    })?;
    verify_artifact(&artifact.path, &version, &source)?;
    // Artifacts are complete source snapshots. Apply each declaration's safe
    // source_subpath only while selecting skills, never while locating or
    // materializing the shared source+version artifact.
    let selection_root = (!subpath.is_empty()).then(|| Path::new(subpath.as_str()));
    let discovery = discover_with_subpath(&artifact.path, selection_root)
        .map_err(|error| format!("cannot discover {}: {error}", tool.key))?;
    let selection = discovery.select(&tool.include, &tool.exclude);
    // A malformed skill elsewhere in a third-party source must not block
    // activating the selected skills. Only errors that touch the selection, or
    // the source as a whole, are fatal; the rest are reported as warnings.
    let mut warnings = Vec::new();
    for diagnostic in &discovery.diagnostics {
        if diagnostic.level != DiagnosticLevel::Error {
            continue;
        }
        if diagnostic_touches_selection(diagnostic, &selection.selected) {
            return Err(format!(
                "cannot discover {}: selected skills contain errors",
                tool.key
            ));
        }
        warnings.push(format!(
            "{}: unselected source error: {}",
            tool.key,
            describe_diagnostic(diagnostic)
        ));
    }
    let mut adapters = BTreeSet::new();
    for agent in &tool.agents {
        adapters.insert(Adapter::parse(agent)?);
    }
    if adapters.is_empty() {
        return Err(format!("skill tool {} has no agents", tool.key));
    }
    let boundaries = adapters
        .iter()
        .map(|adapter| {
            let root = adapter.root(global, &config.root)?;
            let boundary = adapter.boundary(global, &config.root)?;
            validate_adapter_root(&root, &boundary)?;
            Ok((*adapter, root, boundary))
        })
        .collect::<Result<Vec<_>, String>>()?;
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
        for (adapter, root, boundary) in &boundaries {
            let target = root.join(&skill.name);
            let link = relative_path(root.as_path(), &source_skill)?;
            result.push(DesiredLink {
                tool: tool.key.clone(),
                adapter: *adapter,
                skill: skill.name.clone(),
                target,
                link,
                root: root.clone(),
                boundary: boundary.clone(),
                source: source_skill.display().to_string(),
                version: version.clone(),
            });
        }
    }
    Ok(ToolLinks {
        links: result,
        warnings,
    })
}

struct ToolLinks {
    links: Vec<DesiredLink>,
    warnings: Vec<String>,
}

/// Whether an error-level diagnostic should block activating this selection.
/// A diagnostic without a path, or one inside a selected skill directory,
/// affects what we are about to activate; anything else is only reported.
fn diagnostic_touches_selection(diagnostic: &Diagnostic, selected: &[Skill]) -> bool {
    let Some(path) = diagnostic.path.as_deref() else {
        return true;
    };
    selected
        .iter()
        .any(|skill| path == skill.path || path.starts_with(&format!("{}/", skill.path)))
}

fn describe_diagnostic(diagnostic: &Diagnostic) -> String {
    match &diagnostic.path {
        Some(path) => format!("{path}: {}", diagnostic.message),
        None => diagnostic.message.clone(),
    }
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
    Replaced(String),
    Conflict(String),
}

fn converge_link(
    desired: &DesiredLink,
    old: Option<&OwnedLink>,
    force: bool,
) -> Result<ConvergeResult, String> {
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
        TargetState::Other => {
            if force {
                let backup = back_up_target(&desired.target)?;
                create_link(desired)?;
                Ok(ConvergeResult::Replaced(format!(
                    "replaced existing target {}; previous contents moved to {}",
                    desired.target.display(),
                    backup.display()
                )))
            } else {
                Ok(ConvergeResult::Conflict(format!(
                    "conflict at {}: existing unmanaged target was left untouched",
                    desired.target.display()
                )))
            }
        }
        TargetState::Symlink(actual) => {
            if actual == desired.link {
                if old.is_some() {
                    Ok(ConvergeResult::Unchanged)
                } else if force {
                    Ok(ConvergeResult::Replaced(format!(
                        "adopted existing unmanaged link at {}",
                        desired.target.display()
                    )))
                } else {
                    Ok(ConvergeResult::Conflict(format!(
                        "conflict at {}: existing unmanaged symlink was left untouched",
                        desired.target.display()
                    )))
                }
            } else if let Some(old) = old {
                if actual == old.link || force {
                    remove_link(&desired.target)?;
                    create_link(desired)?;
                    Ok(ConvergeResult::Updated)
                } else {
                    Ok(ConvergeResult::Conflict(format!(
                        "conflict at {}: managed link was changed and was left untouched",
                        desired.target.display()
                    )))
                }
            } else if force {
                remove_link(&desired.target)?;
                create_link(desired)?;
                Ok(ConvergeResult::Replaced(format!(
                    "replaced existing unmanaged symlink at {}",
                    desired.target.display()
                )))
            } else {
                Ok(ConvergeResult::Conflict(format!(
                    "conflict at {}: existing unmanaged symlink was left untouched",
                    desired.target.display()
                )))
            }
        }
    }
}

/// Move a conflicting file or directory aside instead of deleting it. The
/// backup is hidden and timestamped so it cannot be mistaken for a skill, and
/// a numeric suffix is added if the same target is replaced twice in one
/// second.
fn back_up_target(target: &Path) -> Result<PathBuf, String> {
    let parent = target
        .parent()
        .ok_or_else(|| format!("cannot back up {}", target.display()))?;
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("target");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let mut candidate = parent.join(format!(".skills-replaced-{name}-{stamp}"));
    let mut counter = 1;
    while candidate.exists() {
        candidate = parent.join(format!(".skills-replaced-{name}-{stamp}.{counter}"));
        counter += 1;
    }
    fs::rename(target, &candidate)
        .map_err(|error| format!("cannot move {} aside: {error}", target.display()))?;
    Ok(candidate)
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
    validate_adapter_root(&desired.root, &desired.boundary)?;
    create_symlink(&desired.link, &desired.target)
}

fn create_symlink(link: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create adapter root {}: {error}", parent.display()))?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(link, target).map_err(|error| {
            format!(
                "cannot create relative skill link {}: {error}",
                target.display()
            )
        })
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(link, target).map_err(|error| {
            format!(
                "cannot create relative skill link {}: {error}",
                target.display()
            )
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (link, target);
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
    let contents = fs::read_to_string(&manifest_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!(
                "installed artifact is missing its backend manifest: {}",
                manifest_path.display()
            )
        } else {
            format!(
                "cannot read artifact manifest {}: {error}",
                manifest_path.display()
            )
        }
    })?;
    let manifest: serde_json::Value = serde_json::from_str(&contents).map_err(|error| {
        format!(
            "cannot parse artifact manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    let schema = manifest
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            format!(
                "artifact manifest has no schema_version: {}",
                manifest_path.display()
            )
        })?;
    if schema != 1 {
        return Err(format!(
            "unsupported artifact manifest schema {schema}: {}",
            manifest_path.display()
        ));
    }

    let actual_source = manifest
        .get("source")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "artifact manifest has no source: {}",
                manifest_path.display()
            )
        })?;
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

    let actual = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
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
    if let Some(revision) = manifest.get("revision").and_then(serde_json::Value::as_str) {
        let revision = canonical_version(revision, source.kind)?;
        if revision != actual_version {
            return Err(format!(
                "installed artifact revision mismatch: version is {actual_version} but revision is {revision}"
            ));
        }
    }

    // A full snapshot is rooted at the artifact itself. A manifest that
    // records a non-empty subpath is an older partial artifact and must not be
    // selected, because reusing it for another declaration could activate a
    // different source tree at the same source+version key.
    let artifact_subpath = manifest
        .get("subpath")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "artifact manifest has no subpath: {}",
                manifest_path.display()
            )
        })?;
    if !validate_subpath(Some(artifact_subpath))?.is_empty() {
        return Err(format!(
            "artifact manifest is not a complete source snapshot: subpath {artifact_subpath:?}"
        ));
    }
    let actual_tree = manifest
        .get("tree")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "artifact manifest has no snapshot tree: {}",
                manifest_path.display()
            )
        })?;
    if actual_tree.len() != 64
        || !actual_tree
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(format!(
            "artifact manifest has invalid snapshot tree: {}",
            manifest_path.display()
        ));
    }
    let expected_tree = crate::source::tree_digest(path)?;
    if actual_tree.to_ascii_lowercase() != expected_tree {
        return Err(format!(
            "artifact snapshot tree mismatch: declared {actual_tree} but found {expected_tree}"
        ));
    }

    let inventory = manifest
        .get("skills")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            format!(
                "artifact manifest has no skills inventory: {}",
                manifest_path.display()
            )
        })?;
    let mut declared = BTreeMap::new();
    for entry in inventory {
        let object = entry.as_object().ok_or_else(|| {
            format!(
                "artifact skills inventory contains a non-object: {}",
                manifest_path.display()
            )
        })?;
        let raw_path = object
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                format!(
                    "artifact skills inventory entry has no path: {}",
                    manifest_path.display()
                )
            })?;
        let path_value = validate_inventory_path(raw_path)?;
        let tree = object
            .get("tree")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("artifact skills inventory entry has no tree: {raw_path}"))?;
        if tree.len() != 64 || !tree.chars().all(|character| character.is_ascii_hexdigit()) {
            return Err(format!(
                "artifact skills inventory has invalid tree for {raw_path}"
            ));
        }
        if declared
            .insert(path_value.clone(), tree.to_ascii_lowercase())
            .is_some()
        {
            return Err(format!(
                "artifact skills inventory repeats path {raw_path:?}"
            ));
        }
        let skill_root = if path_value == "." {
            path.to_path_buf()
        } else {
            path.join(&path_value)
        };
        if !skill_root.is_dir() || !skill_root.join("SKILL.md").is_file() {
            return Err(format!(
                "artifact skills inventory path is missing: {raw_path}"
            ));
        }
        let actual_tree = crate::source::tree_digest(&skill_root)?;
        if actual_tree != tree.to_ascii_lowercase() {
            return Err(format!(
                "artifact skills inventory tree mismatch for {raw_path}"
            ));
        }
    }

    let actual_paths = artifact_skill_paths(path)?;
    let declared_paths = declared.keys().cloned().collect::<BTreeSet<_>>();
    if actual_paths != declared_paths {
        return Err(format!(
            "artifact skills inventory does not match snapshot (declared {:?}, found {:?})",
            declared_paths, actual_paths
        ));
    }
    Ok(())
}

fn validate_inventory_path(path: &str) -> Result<String, String> {
    if path == "." {
        return Ok(path.to_string());
    }
    let normalized = validate_subpath(Some(path))?;
    if normalized.is_empty() {
        return Err("artifact skills inventory path must name a skill directory".into());
    }
    Ok(normalized)
}

fn artifact_skill_paths(path: &Path) -> Result<BTreeSet<String>, String> {
    let mut paths = BTreeSet::new();
    let walker = WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !crate::source::TREE_DIGEST_EXCLUDED_DIRECTORIES
                    .iter()
                    .any(|name| entry.file_name().to_str() == Some(*name))
        });
    for entry in walker {
        let entry = entry.map_err(|error| format!("cannot inspect artifact inventory: {error}"))?;
        if entry.file_name() != "SKILL.md" {
            continue;
        }
        let parent = entry
            .path()
            .parent()
            .ok_or_else(|| "artifact inventory has a SKILL.md without a parent".to_string())?;
        let relative = parent
            .strip_prefix(path)
            .map_err(|error| error.to_string())?;
        let relative = if relative.as_os_str().is_empty() {
            ".".to_string()
        } else {
            relative
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/")
        };
        paths.insert(relative);
    }
    Ok(paths)
}

/// Reproduce mise's install-directory slug: lowercase, every run of
/// non-alphanumeric characters collapsed to one `-`, with leading and
/// trailing separators trimmed. mise applies this to `<backend>-<tool>`, so
/// `git:` keys containing `.` or `_` only match when this is exact.
fn mise_install_name(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            separator = false;
        } else if !separator && !slug.is_empty() {
            slug.push('-');
            separator = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
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
    let tool_name = mise_install_name(&format!(
        "skills-{}",
        key.strip_prefix("skills:").unwrap_or(key)
    ));
    let mut candidates = Vec::new();
    // mise stores a Git version containing `:` as a filesystem-safe `-` (the
    // declared `rev:<commit>` becomes `rev-<commit>`) and slugs the version
    // with the same rules as the tool name.
    for candidate in [
        version.to_string(),
        version.replace(':', "-"),
        mise_install_name(version),
    ] {
        candidates.push(installs.join(&tool_name).join(candidate));
    }
    if let Some(raw) = version.strip_prefix("rev:") {
        candidates.push(installs.join(&tool_name).join(raw));
    }
    for candidate in candidates {
        if candidate.is_dir() {
            return Ok(Some(Artifact {
                path: fs::canonicalize(candidate).map_err(|error| error.to_string())?,
            }));
        }
    }
    // Do not fall back to scanning another version directory. The install
    // path is the source+version identity; selecting an artifact by version
    // alone can activate a different source when revisions happen to match.
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

fn lock_config_root(config_root: &Path) -> Result<LockFile, String> {
    // Keep locks beside, not inside, the ownership manifests so tools that
    // enumerate `state/skills` see only manifests.
    let manifest = state_manifest_path(config_root)?;
    let state = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "cannot determine skills state directory".to_string())?;
    let hash = manifest
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| "cannot determine skills state hash".to_string())?;
    let path = state.join("skills-locks").join(format!("{hash}.lock"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "cannot create state directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let mut lock = LockFile::open(&path)
        .map_err(|error| format!("cannot open skills state lock {}: {error}", path.display()))?;
    lock.lock()
        .map_err(|error| format!("cannot lock skills state {}: {error}", path.display()))?;
    Ok(lock)
}

fn load_manifest(config_root: &Path, global: bool) -> Result<OwnershipManifest, String> {
    let path = state_manifest_path(config_root)?;
    let manifest = match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|error| {
            format!(
                "cannot parse ownership manifest {}: {error}",
                path.display()
            )
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => OwnershipManifest {
            config_root: config_root.display().to_string(),
            ..OwnershipManifest::default()
        },
        Err(error) => {
            return Err(format!(
                "cannot read ownership manifest {}: {error}",
                path.display()
            ))
        }
    };
    validate_ownership_manifest(&manifest, config_root, global)?;
    Ok(manifest)
}

fn validate_ownership_manifest(
    manifest: &OwnershipManifest,
    config_root: &Path,
    global: bool,
) -> Result<(), String> {
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported ownership manifest schema {}",
            manifest.schema_version
        ));
    }
    let expected_root = normalize_absolute_path(config_root)?;
    let recorded_root =
        normalize_absolute_path(Path::new(&manifest.config_root)).map_err(|_| {
            format!(
                "ownership manifest config_root is not an absolute path: {}",
                manifest.config_root
            )
        })?;
    if recorded_root != expected_root {
        return Err(format!(
            "ownership manifest config_root {} does not match {}",
            manifest.config_root,
            config_root.display()
        ));
    }

    let mut seen = BTreeSet::new();
    for link in &manifest.links {
        let adapter = Adapter::parse(&link.adapter)?;
        let adapter_root = adapter.root(global, config_root)?;
        let boundary = adapter.boundary(global, config_root)?;
        validate_adapter_root(&adapter_root, &boundary)?;
        let target = validate_owned_target(&link.target, &adapter_root).map_err(|error| {
            format!(
                "ownership manifest link {} has unsafe target: {error}",
                link.target
            )
        })?;
        let key = (adapter.id().to_string(), target);
        if !seen.insert(key) {
            return Err(format!(
                "ownership manifest contains duplicate target {}",
                link.target
            ));
        }
    }
    Ok(())
}

fn validate_adapter_root(adapter_root: &Path, boundary: &Path) -> Result<(), String> {
    let adapter_root = normalize_absolute_path(adapter_root)?;
    let boundary = normalize_absolute_path(boundary)?;
    if !adapter_root.starts_with(&boundary) {
        return Err(format!(
            "adapter root {} is outside expected root {}",
            adapter_root.display(),
            boundary.display()
        ));
    }

    let relative = adapter_root
        .strip_prefix(&boundary)
        .map_err(|error| error.to_string())?;
    let mut current = boundary.clone();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(format!(
                    "cannot inspect adapter path {}: {error}",
                    current.display()
                ))
            }
        };
        if metadata.file_type().is_symlink() {
            let resolved = fs::canonicalize(&current).map_err(|error| {
                format!(
                    "adapter path symlink {} cannot be resolved safely: {error}",
                    current.display()
                )
            })?;
            if !resolved.starts_with(&boundary) {
                return Err(format!(
                    "adapter path symlink {} escapes expected root {}",
                    current.display(),
                    boundary.display()
                ));
            }
        } else if !metadata.is_dir() {
            return Err(format!(
                "adapter path {} is not a directory",
                current.display()
            ));
        }
    }
    Ok(())
}

fn normalize_absolute_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("path is not absolute: {}", path.display()));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(format!("path escapes its root: {}", path.display()));
                }
            }
        }
    }
    Ok(normalized)
}

fn validate_owned_target(target: &str, adapter_root: &Path) -> Result<PathBuf, String> {
    let target = normalize_absolute_path(Path::new(target))?;
    let adapter_root = normalize_absolute_path(adapter_root)?;
    if target == adapter_root || !target.starts_with(&adapter_root) {
        return Err(format!(
            "target {} is outside adapter root {}",
            target.display(),
            adapter_root.display()
        ));
    }

    // A symlinked parent can make a lexically safe path point outside the
    // adapter root. Resolve the nearest existing ancestor before allowing
    // deletion of this persisted target.
    let canonical_root = fs::canonicalize(&adapter_root).unwrap_or_else(|_| adapter_root.clone());
    let mut ancestor = target.parent();
    while let Some(path) = ancestor {
        if path == adapter_root {
            // The adapter root itself may have been removed after the last
            // link was pruned; no existing parent remains to resolve.
            break;
        }
        if path.exists() {
            let canonical = fs::canonicalize(path).map_err(|error| {
                format!("cannot resolve target parent {}: {error}", path.display())
            })?;
            if !canonical.starts_with(&canonical_root) {
                return Err(format!(
                    "target parent {} escapes adapter root {}",
                    path.display(),
                    adapter_root.display()
                ));
            }
            break;
        }
        ancestor = path.parent();
    }
    Ok(target)
}

fn save_manifest(config_root: &Path, manifest: &OwnershipManifest) -> Result<(), String> {
    if std::env::var("MISE_SKILLS_FAIL_MANIFEST_WRITE")
        .ok()
        .as_deref()
        == Some("1")
    {
        return Err("injected ownership manifest persistence failure".into());
    }
    let path = state_manifest_path(config_root)?;
    if manifest.links.is_empty() && manifest.git_exclude_roots.is_empty() {
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
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "cannot replace ownership manifest {}: {error}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn manifest_fixture() -> (TempDir, PathBuf, SourceSpec, String, String) {
        let temp = TempDir::new().unwrap();
        let source_root = temp.path().join("source");
        let artifact = temp.path().join("artifact");
        fs::create_dir_all(source_root.join("skills/demo")).unwrap();
        fs::create_dir_all(artifact.join("skills/demo")).unwrap();
        let skill = "---\nname: demo\ndescription: demo\n---\n";
        fs::write(source_root.join("skills/demo/SKILL.md"), skill).unwrap();
        fs::write(artifact.join("skills/demo/SKILL.md"), skill).unwrap();
        fs::write(source_root.join("skills/demo/extra.txt"), "extra").unwrap();
        fs::write(artifact.join("skills/demo/extra.txt"), "extra").unwrap();
        let source =
            SourceSpec::parse(&format!("path:{}", source_root.display()), temp.path()).unwrap();
        let version = local_pin(&source, "").unwrap();
        let tree = crate::source::tree_digest(&artifact.join("skills/demo")).unwrap();
        (temp, artifact, source, version, tree)
    }

    #[test]
    fn verifies_complete_manifest_source_version_and_inventory() {
        let (_temp, artifact, source, version, tree) = manifest_fixture();
        let manifest = json!({
            "schema_version": 1,
            "source": format!("path:{}", source.resolved_path.as_ref().unwrap().display()),
            "subpath": ".",
            "version": version,
            "revision": version,
            "tree": crate::source::tree_digest(&artifact).unwrap(),
            "skills": [{"path": "skills/demo", "tree": tree}],
        });
        fs::write(
            artifact.join(".mise-skills-manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        verify_artifact(&artifact, &version, &source).unwrap();
    }

    #[test]
    fn rejects_manifestless_installed_artifact() {
        let (_temp, artifact, source, version, _tree) = manifest_fixture();
        let error = verify_artifact(&artifact, &version, &source).unwrap_err();
        assert!(error.contains("missing its backend manifest"), "{error}");
    }

    #[test]
    fn slugs_like_mise_install_directories() {
        assert_eq!(
            mise_install_name("skills-github.com/owner/repo"),
            "skills-github-com-owner-repo"
        );
        assert_eq!(mise_install_name("skills-a.b_c-d+e"), "skills-a-b-c-d-e");
        assert_eq!(mise_install_name("skills-a..b"), "skills-a-b");
        assert_eq!(mise_install_name("skills-.a."), "skills-a");
        assert_eq!(mise_install_name("skills-A.B"), "skills-a-b");
    }

    #[test]
    fn finds_git_artifacts_under_mise_slugged_names() {
        let temp = TempDir::new().unwrap();
        let installs = temp.path().join("installs");
        let revision = "abcdef0123456789abcdef0123456789abcdef01";
        let directory = installs
            .join("skills-github-com-owner-repo")
            .join(format!("rev-{revision}"));
        fs::create_dir_all(&directory).unwrap();
        std::env::set_var("MISE_INSTALLS_DIR", &installs);
        let found = find_artifact("github.com/owner/repo", &format!("rev:{revision}"));
        std::env::remove_var("MISE_INSTALLS_DIR");
        assert_eq!(
            found.unwrap().unwrap().path,
            fs::canonicalize(&directory).unwrap()
        );
    }

    #[test]
    fn rejects_untrusted_ownership_manifest_metadata_before_deletion() {
        let temp = TempDir::new().unwrap();
        let config_root = temp.path().join("project");
        fs::create_dir_all(&config_root).unwrap();
        let target = config_root.join(".pi/skills/demo");
        let valid = OwnershipManifest {
            schema_version: 1,
            config_root: config_root.display().to_string(),
            links: vec![OwnedLink {
                tool: "skills:local/source".into(),
                adapter: "pi".into(),
                skill: "demo".into(),
                target: target.display().to_string(),
                link: "../../artifact/demo".into(),
                source: "artifact/demo".into(),
                version: "tree-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .into(),
            }],
            git_exclude_roots: Vec::new(),
        };
        validate_ownership_manifest(&valid, &config_root, false).unwrap();

        let mut invalid = valid.clone();
        invalid.schema_version = 2;
        assert!(validate_ownership_manifest(&invalid, &config_root, false)
            .unwrap_err()
            .contains("unsupported ownership manifest schema"));

        let mut invalid = valid.clone();
        invalid.config_root = temp.path().join("other").display().to_string();
        assert!(validate_ownership_manifest(&invalid, &config_root, false)
            .unwrap_err()
            .contains("config_root"));

        let mut invalid = valid.clone();
        invalid.links[0].adapter = "unknown".into();
        assert!(validate_ownership_manifest(&invalid, &config_root, false)
            .unwrap_err()
            .contains("unsupported agent"));

        let mut invalid = valid;
        invalid.links[0].target = temp.path().join("outside").display().to_string();
        assert!(validate_ownership_manifest(&invalid, &config_root, false)
            .unwrap_err()
            .contains("outside adapter root"));
    }

    #[test]
    fn rejects_partial_manifest_snapshot_and_inventory_mismatch() {
        let (_temp, artifact, source, version, tree) = manifest_fixture();
        let mut manifest = json!({
            "schema_version": 1,
            "source": format!("path:{}", source.resolved_path.as_ref().unwrap().display()),
            "subpath": "skills",
            "version": version,
            "revision": version,
            "tree": crate::source::tree_digest(&artifact).unwrap(),
            "skills": [{"path": "skills/demo", "tree": tree}],
        });
        fs::write(
            artifact.join(".mise-skills-manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        assert!(verify_artifact(&artifact, &version, &source)
            .unwrap_err()
            .contains("complete source snapshot"));

        manifest["subpath"] = json!(".");
        manifest["skills"][0]["path"] = json!("missing");
        fs::write(
            artifact.join(".mise-skills-manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
        assert!(verify_artifact(&artifact, &version, &source)
            .unwrap_err()
            .contains("inventory path is missing"));
    }
}
