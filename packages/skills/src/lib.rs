//! Deterministic Agent Skills discovery, catalog overlays, and selectors.
//!
//! The library deliberately deals in repository-relative paths.  This keeps
//! output stable across checkouts and makes the same selection reproducible for
//! a local snapshot and an immutable Git installation.

use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

const SKIPPED_DIRECTORIES: &[&str] = &[".git", "node_modules", "dist", "build", "__pycache__"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub path: Option<String>,
    pub message: String,
}

impl Diagnostic {
    fn error(path: Option<String>, message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            path,
            message: message.into(),
        }
    }

    fn warning(path: Option<String>, message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            path,
            message: message.into(),
        }
    }
}

#[derive(Debug)]
pub enum DiscoveryError {
    Source { path: PathBuf, message: String },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source { path, message } => write!(f, "{}: {}", path.display(), message),
        }
    }
}

impl std::error::Error for DiscoveryError {}

#[derive(Debug, Clone, Serialize)]
pub struct Skill {
    /// Repository-relative directory containing the exact `SKILL.md` file.
    pub path: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    /// Every file and symlink in this skill directory, relative to `path`.
    /// Entries are sorted lexically and are intentionally included in list
    /// output so consumers can materialize the complete artifact.
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShadowedSkill {
    pub name: String,
    pub winner: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Selection {
    pub selected: Vec<Skill>,
    pub excluded: Vec<SelectionDecision>,
    pub decisions: Vec<SelectionDecision>,
    pub missing_includes: Vec<String>,
    pub missing_excludes: Vec<String>,
    pub selector_resolutions: Vec<SelectorResolution>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelectionDecision {
    pub path: String,
    pub name: String,
    pub included_by: Vec<String>,
    pub excluded_by: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelectorResolution {
    pub selector: String,
    pub winner: String,
    pub shadowed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Explain {
    pub shadowed: Vec<ShadowedSkill>,
    pub decisions: Vec<SelectionDecision>,
    pub missing_includes: Vec<String>,
    pub missing_excludes: Vec<String>,
    pub selector_resolutions: Vec<SelectorResolution>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogSummary {
    pub path: Option<String>,
    pub roots: Vec<String>,
    pub exclude: Vec<String>,
    pub tags: BTreeMap<String, Vec<String>>,
    pub aliases: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Discovery {
    source: PathBuf,
    pub catalog: CatalogSummary,
    /// All valid candidates before duplicate-name shadowing is applied.
    pub candidates: Vec<Skill>,
    /// Effective candidates after duplicate-name shadowing.
    pub skills: Vec<Skill>,
    pub shadowed: Vec<ShadowedSkill>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Discovery {
    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.level == DiagnosticLevel::Error)
    }

    /// Apply the v1 selector grammar. Missing selectors are represented in the
    /// result and are deliberately not errors.
    pub fn select(&self, includes: &[String], excludes: &[String]) -> Selection {
        let includes = includes.to_vec();
        let excludes = excludes.to_vec();
        let mut decisions = Vec::new();
        let mut selected = Vec::new();
        let mut excluded = Vec::new();
        let mut missing_includes = Vec::new();
        let mut missing_excludes = Vec::new();
        let mut selector_resolutions = Vec::new();

        let include_all = includes.is_empty();
        let mut include_matches: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for selector in &includes {
            let all_matches = self.selector_candidates(selector);
            let matches = self.match_selector(selector);
            if all_matches.len() > 1 && is_bare_selector(selector) {
                selector_resolutions.push(SelectorResolution {
                    selector: selector.clone(),
                    winner: matches
                        .first()
                        .map(|skill| skill.path.clone())
                        .unwrap_or_default(),
                    shadowed: all_matches
                        .iter()
                        .skip(1)
                        .map(|skill| skill.path.clone())
                        .collect(),
                });
            }
            if matches.is_empty() {
                missing_includes.push(selector.clone());
            }
            for skill in matches {
                include_matches
                    .entry(skill.path.clone())
                    .or_default()
                    .push(selector.clone());
            }
        }

        let mut exclude_matches: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for selector in &excludes {
            let all_matches = self.selector_candidates(selector);
            let matches = self.match_selector(selector);
            if all_matches.len() > 1 && is_bare_selector(selector) {
                selector_resolutions.push(SelectorResolution {
                    selector: selector.clone(),
                    winner: matches
                        .first()
                        .map(|skill| skill.path.clone())
                        .unwrap_or_default(),
                    shadowed: all_matches
                        .iter()
                        .skip(1)
                        .map(|skill| skill.path.clone())
                        .collect(),
                });
            }
            if matches.is_empty() {
                missing_excludes.push(selector.clone());
            }
            for skill in matches {
                exclude_matches
                    .entry(skill.path.clone())
                    .or_default()
                    .push(selector.clone());
            }
        }

        for skill in &self.skills {
            let included_by = include_matches
                .get(&skill.path)
                .cloned()
                .unwrap_or_default();
            let excluded_by = exclude_matches
                .get(&skill.path)
                .cloned()
                .unwrap_or_default();
            let included = include_all || !included_by.is_empty();
            let is_excluded = !excluded_by.is_empty();
            if included && !is_excluded {
                selected.push(skill.clone());
            }
            let reason = if !included {
                "not selected by any include selector".to_string()
            } else if is_excluded {
                format!("excluded by {}", excluded_by.join(", "))
            } else if include_all {
                "included by default".to_string()
            } else {
                format!("included by {}", included_by.join(", "))
            };
            let decision = SelectionDecision {
                path: skill.path.clone(),
                name: skill.name.clone(),
                included_by,
                excluded_by,
                reason,
            };
            if is_excluded && included {
                excluded.push(decision.clone());
            }
            decisions.push(decision);
        }

        Selection {
            selected,
            excluded,
            decisions,
            missing_includes,
            missing_excludes,
            selector_resolutions,
        }
    }

    pub fn explain(&self, selection: &Selection) -> Explain {
        Explain {
            shadowed: self.shadowed.clone(),
            decisions: selection.decisions.clone(),
            missing_includes: selection.missing_includes.clone(),
            missing_excludes: selection.missing_excludes.clone(),
            selector_resolutions: selection.selector_resolutions.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }

    fn match_selector(&self, selector: &str) -> Vec<Skill> {
        let matches = self.selector_candidates(selector);
        if is_bare_selector(selector) {
            // A bare selector denotes one stable skill, not a set of unrelated
            // basename matches. `self.skills` is already path sorted, so
            // retaining the first item is the deterministic winner.
            matches.into_iter().take(1).collect()
        } else {
            matches
        }
    }

    /// Return non-blocking diagnostics for the strict Agent Skills format.
    ///
    /// `tags` is intentionally accepted by normal discovery as a portable
    /// extension, so this check is kept separate from `has_errors` and never
    /// changes installation or selection behavior.
    pub fn strict_diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for skill in &self.candidates {
            let path = self.source.join(&skill.path).join("SKILL.md");
            diagnostics.extend(strict_skill_diagnostics(
                &path,
                &skill.path,
                &skill.name,
                &skill.description,
            ));
        }
        diagnostics.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.message.cmp(&right.message))
        });
        diagnostics
    }

    fn selector_candidates(&self, selector: &str) -> Vec<Skill> {
        if let Some(tag_expr) = selector.strip_prefix("tag:") {
            let tags = tag_expr.split('+').collect::<Vec<_>>();
            if tags.is_empty() || tags.iter().any(|tag| tag.is_empty()) {
                return Vec::new();
            }
            return self
                .skills
                .iter()
                .filter(|skill| {
                    tags.iter()
                        .all(|tag| skill.tags.iter().any(|value| value == tag))
                })
                .cloned()
                .collect();
        }

        if selector.contains('/') || has_glob_marker(selector) {
            let Ok(glob) = Glob::new(selector) else {
                return Vec::new();
            };
            let matcher = glob.compile_matcher();
            return self
                .skills
                .iter()
                .filter(|skill| matcher.is_match(&skill.path))
                .cloned()
                .collect();
        }

        self.skills
            .iter()
            .filter(|skill| {
                let basename = Path::new(&skill.path)
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default();
                basename == selector
                    || skill.name == selector
                    || self
                        .catalog
                        .aliases
                        .get(selector)
                        .is_some_and(|path| path == &skill.path)
            })
            .cloned()
            .collect()
    }
}

#[derive(Debug, Deserialize, Default)]
struct RawCatalog {
    version: Option<u64>,
    discovery: Option<RawDiscovery>,
    #[serde(default)]
    tags: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawDiscovery {
    #[serde(default)]
    roots: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Debug)]
struct LoadedCatalog {
    summary: CatalogSummary,
    root_patterns: Vec<String>,
    tag_patterns: Vec<(GlobMatcher, Vec<String>)>,
    exclude_matchers: Vec<GlobMatcher>,
    diagnostics: Vec<Diagnostic>,
}

pub fn discover(source: impl AsRef<Path>) -> Result<Discovery, DiscoveryError> {
    discover_with_subpath(source, None)
}

/// Discover a source, optionally replacing its default root with a relative
/// source subpath. Catalog roots remain relative to the source checkout.
pub fn discover_with_subpath(
    source: impl AsRef<Path>,
    subpath: Option<&Path>,
) -> Result<Discovery, DiscoveryError> {
    let source_arg = source.as_ref();
    let source = fs::canonicalize(source_arg).map_err(|error| DiscoveryError::Source {
        path: source_arg.to_path_buf(),
        message: format!("cannot read source: {error}"),
    })?;
    if !source.is_dir() {
        return Err(DiscoveryError::Source {
            path: source,
            message: "source is not a directory".into(),
        });
    }

    let mut catalog = load_catalog(&source);
    let source_root = if let Some(subpath) = subpath {
        let path = validate_relative_path(subpath).map_err(|message| DiscoveryError::Source {
            path: subpath.to_path_buf(),
            message,
        })?;
        let root = source.join(path);
        if !root.is_dir() {
            return Err(DiscoveryError::Source {
                path: root,
                message: "source subpath is not a directory".into(),
            });
        }
        root
    } else {
        source.clone()
    };

    let roots = match discovery_roots(
        &source,
        &source_root,
        &catalog.root_patterns,
        &mut catalog.diagnostics,
    ) {
        Ok(roots) => roots,
        Err(message) => {
            catalog.diagnostics.push(Diagnostic::error(None, message));
            Vec::new()
        }
    };
    let mut candidates = Vec::new();
    let mut seen_roots = HashSet::new();
    for root in roots {
        let root = match fs::canonicalize(&root) {
            Ok(root) => root,
            Err(error) => {
                catalog.diagnostics.push(Diagnostic::error(
                    relative_path(&source, &root),
                    format!("cannot read catalog discovery root: {error}"),
                ));
                continue;
            }
        };
        if !root.starts_with(&source) {
            catalog.diagnostics.push(Diagnostic::error(
                relative_path(&source, &root),
                "catalog discovery root escapes the source",
            ));
            continue;
        }
        if !seen_roots.insert(root.clone()) {
            continue;
        }
        discover_root(&source, &root, &mut catalog, &mut candidates);
    }

    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    candidates.dedup_by(|a, b| a.path == b.path);
    let mut by_name: BTreeMap<String, Vec<Skill>> = BTreeMap::new();
    for candidate in &candidates {
        by_name
            .entry(candidate.name.clone())
            .or_default()
            .push(candidate.clone());
    }
    let mut skills = Vec::new();
    let mut shadowed = Vec::new();
    for (name, mut same_name) in by_name {
        same_name.sort_by(|a, b| a.path.cmp(&b.path));
        let winner = same_name.first().expect("group cannot be empty").clone();
        skills.push(winner.clone());
        for candidate in same_name.into_iter().skip(1) {
            catalog.diagnostics.push(Diagnostic::warning(
                Some(candidate.path.clone()),
                format!("name {name:?} is shadowed by {}", winner.path),
            ));
            shadowed.push(ShadowedSkill {
                name: name.clone(),
                winner: winner.path.clone(),
                path: candidate.path,
            });
        }
    }
    skills.sort_by(|a, b| a.path.cmp(&b.path));
    shadowed.sort_by(|a, b| a.path.cmp(&b.path));
    catalog.diagnostics.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.message.cmp(&b.message))
            .then_with(|| format!("{:?}", a.level).cmp(&format!("{:?}", b.level)))
    });

    Ok(Discovery {
        source,
        catalog: catalog.summary,
        candidates,
        skills,
        shadowed,
        diagnostics: catalog.diagnostics,
    })
}

fn load_catalog(source: &Path) -> LoadedCatalog {
    let path = source.join(".mise").join("skills.toml");
    let path_for_output =
        relative_path(source, &path).unwrap_or_else(|| ".mise/skills.toml".to_string());
    let raw = match fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<RawCatalog>(&content) {
            Ok(raw) => raw,
            Err(error) => {
                return LoadedCatalog {
                    summary: CatalogSummary {
                        path: Some(path_for_output),
                        roots: Vec::new(),
                        exclude: Vec::new(),
                        tags: BTreeMap::new(),
                        aliases: BTreeMap::new(),
                    },
                    root_patterns: Vec::new(),
                    tag_patterns: Vec::new(),
                    exclude_matchers: Vec::new(),
                    diagnostics: vec![Diagnostic::error(
                        None,
                        format!("invalid .mise/skills.toml: {error}"),
                    )],
                }
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LoadedCatalog {
                summary: CatalogSummary {
                    path: None,
                    roots: Vec::new(),
                    exclude: Vec::new(),
                    tags: BTreeMap::new(),
                    aliases: BTreeMap::new(),
                },
                root_patterns: Vec::new(),
                tag_patterns: Vec::new(),
                exclude_matchers: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
        Err(error) => {
            return LoadedCatalog {
                summary: CatalogSummary {
                    path: Some(path_for_output),
                    roots: Vec::new(),
                    exclude: Vec::new(),
                    tags: BTreeMap::new(),
                    aliases: BTreeMap::new(),
                },
                root_patterns: Vec::new(),
                tag_patterns: Vec::new(),
                exclude_matchers: Vec::new(),
                diagnostics: vec![Diagnostic::error(
                    None,
                    format!("cannot read .mise/skills.toml: {error}"),
                )],
            }
        }
    };

    let mut diagnostics = Vec::new();
    if raw.version != Some(1) {
        diagnostics.push(Diagnostic::error(
            Some(".mise/skills.toml".into()),
            "catalog version must be 1",
        ));
    }
    let discovery = raw.discovery.unwrap_or_default();
    let roots = discovery.roots;
    let exclude = discovery.exclude;
    let root_patterns = roots.clone();
    let mut tag_patterns = Vec::new();
    for (pattern, tags) in &raw.tags {
        if validate_relative_string(pattern).is_err() {
            diagnostics.push(Diagnostic::error(
                Some(pattern.clone()),
                "catalog tag path glob must be relative and cannot contain '..'",
            ));
            continue;
        }
        match Glob::new(pattern) {
            Ok(glob) => {
                if let Err(error) = validate_tags(tags) {
                    diagnostics.push(Diagnostic::error(Some(pattern.clone()), error));
                }
                let valid_tags = tags.iter().filter(|tag| valid_tag(tag)).cloned().collect();
                tag_patterns.push((glob.compile_matcher(), valid_tags));
            }
            Err(error) => diagnostics.push(Diagnostic::error(
                Some(pattern.clone()),
                format!("invalid catalog tag path glob: {error}"),
            )),
        }
    }
    let mut exclude_matchers = Vec::new();
    for pattern in &exclude {
        if validate_relative_string(pattern).is_err() {
            diagnostics.push(Diagnostic::error(
                Some(pattern.clone()),
                "catalog exclusion glob must be relative and cannot contain '..'",
            ));
            continue;
        }
        match Glob::new(pattern) {
            Ok(glob) => exclude_matchers.push(glob.compile_matcher()),
            Err(error) => diagnostics.push(Diagnostic::error(
                Some(pattern.clone()),
                format!("invalid catalog exclusion glob: {error}"),
            )),
        }
    }
    let aliases = raw
        .aliases
        .iter()
        .map(|(alias, path)| {
            (
                alias.clone(),
                normalized_relative_string(path).unwrap_or_else(|_| path.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (alias, path) in &aliases {
        if alias.is_empty() || has_glob_marker(alias) || alias.contains('/') {
            diagnostics.push(Diagnostic::error(
                Some(alias.clone()),
                "catalog aliases must be non-empty bare selectors",
            ));
        }
        if validate_relative_string(path).is_err() {
            diagnostics.push(Diagnostic::error(
                Some(alias.clone()),
                "catalog alias target must be a relative skill path",
            ));
        }
    }

    LoadedCatalog {
        summary: CatalogSummary {
            path: Some(path_for_output),
            roots,
            exclude,
            tags: raw.tags,
            aliases,
        },
        root_patterns,
        tag_patterns,
        exclude_matchers,
        diagnostics,
    }
}

fn discovery_roots(
    source: &Path,
    default_root: &Path,
    patterns: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<PathBuf>, String> {
    if patterns.is_empty() {
        return Ok(vec![default_root.to_path_buf()]);
    }
    let mut roots = Vec::new();
    for pattern in patterns {
        if validate_relative_string(pattern).is_err() {
            diagnostics.push(Diagnostic::error(
                Some(pattern.clone()),
                "discovery root must be relative and cannot contain '..'",
            ));
            continue;
        }
        if has_glob_marker(pattern) {
            let matcher = match Glob::new(pattern) {
                Ok(glob) => glob.compile_matcher(),
                Err(error) => {
                    diagnostics.push(Diagnostic::error(
                        Some(pattern.clone()),
                        format!("invalid discovery root glob: {error}"),
                    ));
                    continue;
                }
            };
            let mut found = false;
            for item in WalkDir::new(source).follow_links(false).into_iter() {
                let entry = match item {
                    Ok(entry) => entry,
                    Err(error) => {
                        diagnostics.push(traversal_diagnostic(source, source, error));
                        continue;
                    }
                };
                if !entry.file_type().is_dir() {
                    continue;
                }
                let rel = relative_path(source, entry.path()).unwrap_or_default();
                if matcher.is_match(&rel) {
                    roots.push(entry.path().to_path_buf());
                    found = true;
                }
            }
            if !found {
                diagnostics.push(Diagnostic::warning(
                    Some(pattern.clone()),
                    "discovery root glob matched no directories",
                ));
            }
        } else {
            let root = source.join(pattern);
            if root.is_dir() {
                roots.push(root);
            } else {
                diagnostics.push(Diagnostic::warning(
                    Some(pattern.clone()),
                    "discovery root does not exist",
                ));
            }
        }
    }
    Ok(roots)
}

fn traversal_diagnostic(source: &Path, fallback: &Path, error: walkdir::Error) -> Diagnostic {
    let path = error
        .path()
        .and_then(|path| relative_path(source, path))
        .or_else(|| relative_path(source, fallback))
        .map(|path| if path.is_empty() { ".".into() } else { path });
    let message = if let Some(io_error) = error.io_error() {
        format!(
            "cannot traverse directory: {:?}: {io_error}",
            io_error.kind()
        )
    } else {
        "cannot traverse directory: filesystem loop detected".into()
    };
    Diagnostic::error(path, message)
}

fn discover_root(
    source: &Path,
    root: &Path,
    catalog: &mut LoadedCatalog,
    candidates: &mut Vec<Skill>,
) {
    if has_skipped_ancestor(source, root) {
        return;
    }
    let canonical_root = match fs::canonicalize(root) {
        Ok(path) => path,
        Err(error) => {
            catalog.diagnostics.push(Diagnostic::error(
                relative_path(source, root),
                format!("cannot read discovery root: {error}"),
            ));
            return;
        }
    };
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            if entry.file_type().is_dir() {
                let name = entry.file_name().to_string_lossy();
                if SKIPPED_DIRECTORIES.iter().any(|skipped| *skipped == name) {
                    return false;
                }
            }
            let rel = relative_path(source, entry.path()).unwrap_or_default();
            !catalog_path_excluded(&rel, &catalog.exclude_matchers)
        });

    for item in walker {
        let entry = match item {
            Ok(entry) => entry,
            Err(error) => {
                catalog
                    .diagnostics
                    .push(traversal_diagnostic(source, root, error));
                continue;
            }
        };
        if entry.file_name() != "SKILL.md" {
            continue;
        }
        let Some(skill_root) = entry.path().parent() else {
            continue;
        };
        let relative_root = relative_path(source, skill_root).unwrap_or_default();
        if catalog_path_excluded(&relative_root, &catalog.exclude_matchers) {
            continue;
        }
        match parse_skill(source, skill_root, &canonical_root, catalog) {
            Ok(skill) => candidates.push(skill),
            Err(diagnostic) => catalog.diagnostics.push(diagnostic),
        }
    }
}

fn parse_skill(
    source: &Path,
    skill_root: &Path,
    walker_root: &Path,
    catalog: &LoadedCatalog,
) -> Result<Skill, Diagnostic> {
    let relative_root = relative_path(source, skill_root).unwrap_or_default();
    let canonical_skill_root = fs::canonicalize(skill_root).map_err(|error| {
        Diagnostic::error(
            Some(relative_root.clone()),
            format!("cannot read skill directory: {error}"),
        )
    })?;
    if !canonical_skill_root.starts_with(walker_root) {
        return Err(Diagnostic::error(
            Some(relative_root.clone()),
            "skill directory escapes its discovery root",
        ));
    }
    check_links(skill_root, &canonical_skill_root)
        .map_err(|message| Diagnostic::error(Some(relative_root.clone()), message))?;
    let frontmatter_path = skill_root.join("SKILL.md");
    let (name, description, front_tags) =
        parse_frontmatter(&frontmatter_path).map_err(|message| {
            Diagnostic::error(
                Some(relative_path(source, &frontmatter_path).unwrap_or(relative_root.clone())),
                message,
            )
        })?;
    let mut tags = BTreeSet::new();
    for tag in front_tags {
        tags.insert(tag);
    }
    for (matcher, values) in &catalog.tag_patterns {
        if matcher.is_match(&relative_root) {
            for tag in values {
                tags.insert(tag.clone());
            }
        }
    }
    let files = artifact_files(skill_root)
        .map_err(|message| Diagnostic::error(Some(relative_root.clone()), message))?;
    Ok(Skill {
        path: relative_root,
        name,
        description,
        tags: tags.into_iter().collect(),
        files,
    })
}

fn check_links(skill_root: &Path, canonical_skill_root: &Path) -> Result<(), String> {
    for item in WalkDir::new(skill_root).follow_links(false).into_iter() {
        let entry = item.map_err(|error| format!("cannot inspect skill artifact: {error}"))?;
        if !entry.file_type().is_symlink() {
            continue;
        }
        let target = fs::canonicalize(entry.path()).map_err(|error| {
            format!("unsafe or broken link {}: {error}", entry.path().display())
        })?;
        if !target.starts_with(canonical_skill_root) {
            return Err(format!(
                "unsafe link {} escapes skill root",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn artifact_files(skill_root: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    for item in WalkDir::new(skill_root).follow_links(false).into_iter() {
        let entry = item.map_err(|error| format!("cannot inspect skill artifact: {error}"))?;
        if entry.depth() == 0 || entry.file_type().is_dir() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(skill_root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        files.push(relative);
    }
    files.sort();
    Ok(files)
}

#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: Option<serde_yaml::Value>,
    description: Option<serde_yaml::Value>,
    tags: Option<serde_yaml::Value>,
}

fn parse_frontmatter(path: &Path) -> Result<(String, String, Vec<String>), String> {
    let content =
        fs::read_to_string(path).map_err(|error| format!("cannot read frontmatter: {error}"))?;
    let mut lines = content.lines();
    let first = lines
        .next()
        .unwrap_or_default()
        .trim_start_matches('\u{feff}')
        .trim();
    if first != "---" {
        return Err("SKILL.md must begin with YAML frontmatter (---)".into());
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" || line.trim() == "..." {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err("unterminated YAML frontmatter".into());
    }
    let frontmatter: Frontmatter = serde_yaml::from_str(&yaml)
        .map_err(|error| format!("malformed YAML frontmatter: {error}"))?;
    let name = required_string(frontmatter.name, "name")?;
    let description = required_string(frontmatter.description, "description")?;
    let tags = parse_tags(frontmatter.tags)?;
    Ok((name, description, tags))
}

fn strict_skill_diagnostics(
    path: &Path,
    relative_root: &str,
    name: &str,
    description: &str,
) -> Vec<Diagnostic> {
    let relative_file = if relative_root.is_empty() {
        "SKILL.md".to_string()
    } else {
        format!("{relative_root}/SKILL.md")
    };
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            return vec![Diagnostic::warning(
                Some(relative_file),
                format!("strict Agent Skills validation could not read file: {error}"),
            )]
        }
    };
    let mut lines = content.lines();
    let first = lines
        .next()
        .unwrap_or_default()
        .trim_start_matches('\u{feff}')
        .trim();
    if first != "---" {
        return Vec::new();
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" || line.trim() == "..." {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Vec::new();
    }
    let value = match serde_yaml::from_str::<serde_yaml::Value>(&yaml) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let Some(mapping) = value.as_mapping() else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    for key in mapping.keys() {
        let Some(key) = key.as_str() else {
            diagnostics.push(Diagnostic::warning(
                Some(relative_file.clone()),
                "strict Agent Skills validation found a non-string frontmatter key",
            ));
            continue;
        };
        if key == "tags" {
            diagnostics.push(Diagnostic::warning(
                Some(relative_file.clone()),
                "strict Agent Skills validation: top-level `tags` is an intentional extension and is ignored by strict consumers",
            ));
        } else if key != "name" && key != "description" {
            diagnostics.push(Diagnostic::warning(
                Some(relative_file.clone()),
                format!("strict Agent Skills validation: unsupported top-level field `{key}`"),
            ));
        }
    }
    let valid_name = (1..=64).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });
    if !valid_name {
        diagnostics.push(Diagnostic::warning(
            Some(relative_file.clone()),
            "strict Agent Skills validation: name must be 1-64 lowercase letters, numbers, or single hyphens",
        ));
    }
    if description.len() > 1024 {
        diagnostics.push(Diagnostic::warning(
            Some(relative_file),
            "strict Agent Skills validation: description exceeds 1024 characters",
        ));
    }
    diagnostics
}

fn required_string(value: Option<serde_yaml::Value>, field: &str) -> Result<String, String> {
    let Some(value) = value else {
        return Err(format!("frontmatter requires {field}"));
    };
    let Some(value) = value.as_str() else {
        return Err(format!("frontmatter {field} must be a string"));
    };
    if value.trim().is_empty() {
        return Err(format!("frontmatter {field} must not be empty"));
    }
    Ok(value.to_string())
}

fn parse_tags(value: Option<serde_yaml::Value>) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = match value {
        serde_yaml::Value::Sequence(values) => values,
        serde_yaml::Value::String(value) => vec![serde_yaml::Value::String(value)],
        _ => return Err("frontmatter tags must be a sequence of strings".into()),
    };
    let mut tags = BTreeSet::new();
    for value in values {
        let Some(tag) = value.as_str() else {
            return Err("frontmatter tags must contain only strings".into());
        };
        if !valid_tag(tag) {
            return Err(format!(
                "invalid tag {tag:?}; tags must match [a-z0-9][a-z0-9./-]*"
            ));
        }
        tags.insert(tag.to_string());
    }
    Ok(tags.into_iter().collect())
}

fn validate_tags(tags: &[String]) -> Result<(), String> {
    for tag in tags {
        if !valid_tag(tag) {
            return Err(format!(
                "invalid tag {tag:?}; tags must match [a-z0-9][a-z0-9./-]*"
            ));
        }
    }
    Ok(())
}

fn valid_tag(tag: &str) -> bool {
    let mut chars = tag.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    chars.all(|char| {
        char.is_ascii_lowercase() || char.is_ascii_digit() || matches!(char, '.' | '/' | '-')
    })
}

fn has_skipped_ancestor(source: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(source) else {
        return false;
    };
    relative.components().any(|component| {
        let Component::Normal(value) = component else {
            return false;
        };
        let value = value.to_string_lossy();
        SKIPPED_DIRECTORIES.iter().any(|skipped| *skipped == value)
    })
}

fn catalog_path_excluded(path: &str, matchers: &[GlobMatcher]) -> bool {
    matchers.iter().any(|matcher| {
        matcher.is_match(path) || (!path.is_empty() && matcher.is_match(format!("{path}/SKILL.md")))
    })
}

fn validate_relative_string(path: &str) -> Result<(), String> {
    validate_relative_path(Path::new(path)).map(|_| ())
}

fn normalized_relative_string(path: &str) -> Result<String, String> {
    let path = validate_relative_path(Path::new(path))?;
    Ok(path
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/"))
}

fn validate_relative_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Err("path must be relative".into());
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => clean.push(value),
            Component::ParentDir => return Err("path cannot contain '..'".into()),
            Component::RootDir | Component::Prefix(_) => return Err("path must be relative".into()),
        }
    }
    Ok(clean)
}

fn has_glob_marker(value: &str) -> bool {
    value.contains('*') || value.contains('?') || value.contains('[')
}

fn is_bare_selector(value: &str) -> bool {
    !value.starts_with("tag:") && !value.contains('/') && !has_glob_marker(value)
}

fn relative_path(source: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(source).ok()?;
    if relative.as_os_str().is_empty() {
        return Some(String::new());
    }
    Some(
        relative
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn skill(root: &Path, path: &str, name: &str, tags: &[&str]) {
        let dir = root.join(path);
        fs::create_dir_all(&dir).unwrap();
        let mut file = fs::File::create(dir.join("SKILL.md")).unwrap();
        writeln!(file, "---").unwrap();
        writeln!(file, "name: {name}").unwrap();
        writeln!(file, "description: {name} description").unwrap();
        if !tags.is_empty() {
            writeln!(file, "tags:").unwrap();
            for tag in tags {
                writeln!(file, "  - {tag}").unwrap();
            }
        }
        writeln!(file, "---\n# {name}").unwrap();
    }

    #[test]
    fn discovers_nested_skills_and_artifacts_in_path_order() {
        let temp = TempDir::new().unwrap();
        skill(temp.path(), "z/deep", "deep", &["web"]);
        fs::create_dir_all(temp.path().join("z/deep/scripts")).unwrap();
        fs::write(temp.path().join("z/deep/scripts/run.sh"), "#!/bin/sh\n").unwrap();
        skill(temp.path(), "a", "a", &[]);
        let result = discover(temp.path()).unwrap();
        assert_eq!(
            result
                .skills
                .iter()
                .map(|s| s.path.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "z/deep"]
        );
        assert_eq!(result.skills[1].files, vec!["SKILL.md", "scripts/run.sh"]);
    }

    #[test]
    fn duplicate_names_use_lexical_winner_and_explain_shadow() {
        let temp = TempDir::new().unwrap();
        skill(temp.path(), "z/skill", "same", &[]);
        skill(temp.path(), "a/skill", "same", &[]);
        let result = discover(temp.path()).unwrap();
        assert_eq!(result.skills[0].path, "a/skill");
        assert_eq!(result.shadowed[0].winner, "a/skill");
        assert_eq!(result.shadowed[0].path, "z/skill");
    }

    #[test]
    fn selectors_union_intersect_and_exclude() {
        let temp = TempDir::new().unwrap();
        skill(temp.path(), "one", "one", &["web", "frontend"]);
        skill(temp.path(), "two", "two", &["web"]);
        skill(temp.path(), "three", "three", &["project-management"]);
        let result = discover(temp.path()).unwrap();
        let selected = result.select(
            &["tag:web+frontend".into(), "tag:project-management".into()],
            &["two".into()],
        );
        assert_eq!(
            selected
                .selected
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "three"]
        );
        assert!(selected.missing_includes.is_empty());
    }

    #[test]
    fn traversal_diagnostics_preserve_relative_path_and_error() {
        let temp = TempDir::new().unwrap();
        let source = fs::canonicalize(temp.path()).unwrap();
        let missing = source.join("missing");
        let error = WalkDir::new(&missing)
            .into_iter()
            .next()
            .expect("missing root traversal result")
            .expect_err("missing root should produce a traversal error");

        let diagnostic = traversal_diagnostic(&source, &source, error);
        assert_eq!(diagnostic.level, DiagnosticLevel::Error);
        assert_eq!(diagnostic.path.as_deref(), Some("missing"));
        assert!(diagnostic
            .message
            .contains("cannot traverse directory: NotFound:"));
        assert!(!diagnostic.message.contains(&source.display().to_string()));
    }

    #[test]
    fn malformed_and_invalid_frontmatter_are_diagnostics() {
        let temp = TempDir::new().unwrap();
        let malformed = temp.path().join("malformed");
        fs::create_dir_all(&malformed).unwrap();
        fs::write(malformed.join("SKILL.md"), "---\nname: [\n---\n").unwrap();
        let invalid = temp.path().join("invalid");
        fs::create_dir_all(&invalid).unwrap();
        fs::write(
            invalid.join("SKILL.md"),
            "---\nname: invalid\ndescription: bad\ntags: [UPPER]\n---\n",
        )
        .unwrap();
        let result = discover(temp.path()).unwrap();
        assert!(result.skills.is_empty());
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|d| d.level == DiagnosticLevel::Error)
                .count(),
            2
        );
    }
}
