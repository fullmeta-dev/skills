mod config;
mod lifecycle;
mod source;

use clap::{Args, CommandFactory, Parser, Subcommand};
use config::{Config, Scope};
use mise_skills::{discover_with_subpath, DiagnosticLevel, Explain, Selection, Skill};
use serde::Serialize;
use source::{
    canonical_version, local_pin, resolve_git_pin, validate_subpath, SourceKind, SourceSpec,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "mise-skills",
    version,
    about = "Discover and manage Agent Skills from immutable source snapshots"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List discovered skills, optionally applying selectors.
    List(ListArgs),
    /// Declare a source and selected skills, resolving its immutable pin.
    Use(UseArgs),
    /// Refresh immutable pins for declared sources. This never installs or syncs.
    Upgrade(UpgradeArgs),
    /// Converge the selected configuration without following update tracks.
    Sync(SyncArgs),
    /// Show declared skill sources and their link state.
    Status(StatusArgs),
    /// Remove a declaration or selected entries from its include policy.
    Remove(RemoveArgs),
    /// Remove stale links recorded for the selected configuration root.
    Prune(PruneArgs),
}

#[derive(Debug, Args)]
struct ScopeArgs {
    /// Edit the user-owned global mise config.
    #[arg(long, conflicts_with = "path")]
    global: bool,
    /// Edit this mise config file (or mise.toml inside a directory).
    #[arg(long, value_name = "PATH", conflicts_with = "global")]
    path: Option<PathBuf>,
}

impl ScopeArgs {
    fn scope(&self) -> Scope {
        if self.global {
            Scope::Global
        } else if let Some(path) = &self.path {
            Scope::Path(path.clone())
        } else {
            Scope::Project
        }
    }

    fn is_global(&self) -> bool {
        self.global
    }
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Local checkout or snapshot directory. `path:` is accepted as a prefix.
    source: PathBuf,
    /// Select a source subpath before applying catalog discovery.
    #[arg(long)]
    subpath: Option<PathBuf>,
    /// Include a selector. Repeated values are ORed.
    #[arg(long = "include", short = 'i')]
    includes: Vec<String>,
    /// Exclude a selector. Exclusions always win.
    #[arg(long = "exclude", short = 'e')]
    excludes: Vec<String>,
    /// Render every selected skill's complete artifact tree.
    #[arg(long)]
    tree: bool,
    /// Emit stable machine-readable JSON.
    #[arg(long)]
    json: bool,
    /// Explain shadowing, selector decisions, and missing selectors.
    #[arg(long)]
    explain: bool,
    /// Additional selectors, equivalent to repeated `--include`.
    #[arg(trailing_var_arg = true)]
    selectors: Vec<String>,
}

#[derive(Debug, Args)]
struct UseArgs {
    #[command(flatten)]
    scope: ScopeArgs,
    /// Source (`owner/repository`, `github:...`, `git:...`, or `path:...`).
    source: String,
    /// Skills to include. An omitted list keeps an existing selection policy.
    selectors: Vec<String>,
    /// Agent adapters to record in the declaration.
    #[arg(long = "agent", required = true, num_args = 1..)]
    agents: Vec<String>,
    /// Source-relative skill directory, validated against traversal.
    #[arg(long, alias = "source-subpath")]
    subpath: Option<String>,
    /// Branch or tag to resolve. Defaults to the remote's default branch.
    #[arg(long, conflicts_with = "reference")]
    track: Option<String>,
    /// Explicit Git ref to resolve once.
    #[arg(long, conflicts_with = "track")]
    reference: Option<String>,
    /// Do not resolve or rewrite an existing immutable version.
    #[arg(long)]
    locked: bool,
}

#[derive(Debug, Args)]
struct UpgradeArgs {
    #[command(flatten)]
    scope: ScopeArgs,
    /// Skill tool keys to refresh. With none, refresh every declaration.
    tools: Vec<String>,
    /// Do not resolve or rewrite any declared version.
    #[arg(long)]
    locked: bool,
}

#[derive(Debug, Args)]
struct SyncArgs {
    #[command(flatten)]
    scope: ScopeArgs,
    /// Validate and converge using only already-declared immutable versions.
    #[arg(long)]
    locked: bool,
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[command(flatten)]
    scope: ScopeArgs,
    /// Emit machine-readable status.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RemoveArgs {
    #[command(flatten)]
    scope: ScopeArgs,
    /// Skill tool key, with or without the `skills:` prefix.
    tool: String,
    /// Explicit include selectors to remove; omit to remove the whole tool.
    selectors: Vec<String>,
}

#[derive(Debug, Args)]
struct PruneArgs {
    #[command(flatten)]
    scope: ScopeArgs,
}

#[derive(Debug, Serialize)]
struct ListOutput {
    source: String,
    catalog: mise_skills::CatalogSummary,
    skills: Vec<Skill>,
    shadowed: Vec<mise_skills::ShadowedSkill>,
    diagnostics: Vec<mise_skills::Diagnostic>,
    selection: SelectionOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    explain: Option<Explain>,
}

#[derive(Debug, Serialize)]
struct SelectionOutput {
    selected: Vec<String>,
    excluded: Vec<String>,
    missing_includes: Vec<String>,
    missing_excludes: Vec<String>,
    selector_resolutions: Vec<mise_skills::SelectorResolution>,
}

#[derive(Debug, Serialize)]
struct StatusOutput {
    config: String,
    tools: Vec<config::ToolInfo>,
    links: Vec<lifecycle::LinkStatus>,
    diagnostics: Vec<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        // Clap's generated help is intentionally the only output for a bare
        // invocation; this keeps `mise run skills --help` side-effect free.
        let mut command = Cli::command();
        if let Err(error) = command.print_help() {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
        println!();
        return ExitCode::SUCCESS;
    };

    match command {
        Command::List(args) => run_list(args),
        Command::Use(args) => run_use(args),
        Command::Upgrade(args) => run_upgrade(args),
        Command::Sync(args) => run_sync(args),
        Command::Status(args) => run_status(args),
        Command::Remove(args) => run_remove(args),
        Command::Prune(args) => run_prune(args),
    }
}

fn run_list(args: ListArgs) -> ExitCode {
    let source = strip_path_prefix(&args.source);
    let discovery = match discover_with_subpath(&source, args.subpath.as_deref()) {
        Ok(discovery) => discovery,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };
    let mut includes = args.includes;
    includes.extend(args.selectors);
    let selection = discovery.select(&includes, &args.excludes);
    let explain = discovery.explain(&selection);
    let output = ListOutput {
        source: discovery.source().display().to_string(),
        catalog: discovery.catalog.clone(),
        skills: selection.selected.clone(),
        shadowed: discovery.shadowed.clone(),
        diagnostics: discovery.diagnostics.clone(),
        selection: SelectionOutput {
            selected: selection
                .selected
                .iter()
                .map(|skill| skill.path.clone())
                .collect(),
            excluded: selection
                .excluded
                .iter()
                .map(|decision| decision.path.clone())
                .collect(),
            missing_includes: selection.missing_includes.clone(),
            missing_excludes: selection.missing_excludes.clone(),
            selector_resolutions: selection.selector_resolutions.clone(),
        },
        explain: args.explain.then_some(explain.clone()),
    };

    if args.json {
        match serde_json::to_string_pretty(&output) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("error: cannot encode list as JSON: {error}");
                return ExitCode::from(1);
            }
        }
    } else {
        print_text(&output, &selection, args.tree, args.explain);
    }

    if discovery.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn run_use(args: UseArgs) -> ExitCode {
    match use_command(args) {
        Ok((key, version, path)) => {
            println!("configured {key} at {version} in {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => fail(error),
    }
}

fn use_command(args: UseArgs) -> Result<(String, String, PathBuf), String> {
    let mut config = Config::load(&args.scope.scope())?;
    let before = config.document.to_string();
    let subpath = validate_subpath(args.subpath.as_deref())?;
    for agent in &args.agents {
        lifecycle::validate_agent(agent)?;
    }
    let source = SourceSpec::parse(&args.source, &config.root)?;

    let (version, track) = if args.locked {
        let key = format!("skills:{}", source.key);
        let info = config::tool_info(&config.document, &key)?
            .ok_or_else(|| format!("--locked requires an existing pinned declaration for {key}"))?;
        let existing = SourceSpec::from_declared(&info.source, &config.root, Some(&key))?;
        if !same_source(&existing, &source) {
            return Err(format!(
                "tool key {key:?} already refers to {}, not {}; refusing to repoint it",
                existing.canonical, source.canonical
            ));
        }
        let existing_subpath = config::tool_subpath(&config.document, &key)?;
        if existing_subpath != subpath {
            return Err(format!(
                "--locked cannot change source subpath for {key}; use without --locked to refresh it"
            ));
        }
        // Validate the lockfile spelling, but retain its exact text: locked
        // mode must not rewrite even a legacy raw Git commit pin.
        canonical_version(&info.version, source.kind)?;
        (
            info.version,
            if source.kind == SourceKind::Path {
                None
            } else {
                info.track
            },
        )
    } else {
        match source.kind {
            SourceKind::Path => (local_pin(&source, &subpath)?, None),
            SourceKind::Github | SourceKind::Git => {
                let reference = args.reference.as_deref().or(args.track.as_deref());
                let pin = resolve_git_pin(&source, reference)?;
                (pin.version, Some(pin.track))
            }
        }
    };

    config::apply_use(
        &mut config.document,
        &config.root,
        &source,
        &subpath,
        &version,
        track.as_deref(),
        &args.selectors,
        &args.agents,
    )?;
    config.save_if_changed(&before)?;
    Ok((format!("skills:{}", source.key), version, config.path))
}

fn run_upgrade(args: UpgradeArgs) -> ExitCode {
    match upgrade_command(args) {
        Ok((count, path)) => {
            println!("upgraded {count} skill source(s) in {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => fail(error),
    }
}

fn upgrade_command(args: UpgradeArgs) -> Result<(usize, PathBuf), String> {
    let mut config = Config::load(&args.scope.scope())?;
    let before = config.document.to_string();
    let all_tools = config::skill_tools(&config.document)?;
    let requested: Vec<String> = if args.tools.is_empty() {
        all_tools.iter().map(|tool| tool.key.clone()).collect()
    } else {
        args.tools
            .iter()
            .map(|tool| {
                if tool.starts_with("skills:") {
                    tool.clone()
                } else {
                    format!("skills:{tool}")
                }
            })
            .collect()
    };
    let mut changed = 0;
    for key in requested {
        let Some(info) = all_tools.iter().find(|tool| tool.key == key) else {
            return Err(format!("skill tool {key:?} is not declared"));
        };
        let source = SourceSpec::from_declared(&info.source, &config.root, Some(&key))?;
        if args.locked {
            continue;
        }
        let subpath = config::tool_subpath(&config.document, &key)?;
        let subpath = validate_subpath(Some(&subpath))?;
        let version = match source.kind {
            SourceKind::Path => local_pin(&source, &subpath)?,
            SourceKind::Github | SourceKind::Git => {
                resolve_git_pin(&source, info.track.as_deref())?.version
            }
        };
        if canonical_version(&info.version, source.kind)? != version {
            config::update_version(&mut config.document, &key, &version)?;
            changed += 1;
        }
    }
    config.save_if_changed(&before)?;
    Ok((changed, config.path))
}

fn run_sync(args: SyncArgs) -> ExitCode {
    match Config::load(&args.scope.scope()).and_then(|config| {
        let report = lifecycle::sync(&config, args.scope.is_global())?;
        println!(
            "synced {} link(s), removed {} link(s) in {}",
            report.linked,
            report.removed,
            config.path.display()
        );
        for conflict in report.conflicts {
            eprintln!("warning: {conflict}");
        }
        Ok(())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}

fn run_status(args: StatusArgs) -> ExitCode {
    match Config::load(&args.scope.scope()).and_then(|config| {
        let lifecycle = lifecycle::status(&config, args.scope.is_global())?;
        let output = StatusOutput {
            config: config.path.display().to_string(),
            tools: config::skill_tools(&config.document)?,
            links: lifecycle.links,
            diagnostics: lifecycle.diagnostics,
        };
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&output).map_err(|error| error.to_string())?
            );
        } else {
            for tool in &output.tools {
                println!("{}\t{}", tool.key, tool.version);
            }
            for link in &output.links {
                println!(
                    "{}\t{}\t{}\t{}",
                    link.adapter, link.skill, link.state, link.target
                );
            }
            for diagnostic in &output.diagnostics {
                eprintln!("warning: {diagnostic}");
            }
        }
        Ok(())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}

fn run_remove(args: RemoveArgs) -> ExitCode {
    match remove_command(args) {
        Ok((removed, path)) => {
            if removed {
                println!("removed skill declaration(s) in {}", path.display());
            } else {
                println!("no skill declaration changed in {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail(error),
    }
}

fn remove_command(args: RemoveArgs) -> Result<(bool, PathBuf), String> {
    let mut config = Config::load(&args.scope.scope())?;
    let before = config.document.to_string();
    let key = if args.tool.starts_with("skills:") {
        args.tool
    } else {
        format!("skills:{}", args.tool)
    };
    config::remove_tool(&mut config.document, &key, &args.selectors)?;
    config.save_if_changed(&before)?;
    let report = lifecycle::sync(&config, args.scope.is_global())?;
    for conflict in report.conflicts {
        eprintln!("warning: {conflict}");
    }
    Ok((before != config.document.to_string(), config.path))
}

fn run_prune(args: PruneArgs) -> ExitCode {
    match Config::load(&args.scope.scope()).and_then(|config| {
        let report = lifecycle::prune(&config, args.scope.is_global())?;
        println!(
            "pruned {} stale link(s) in {}",
            report.removed,
            config.path.display()
        );
        for conflict in report.conflicts {
            eprintln!("warning: {conflict}");
        }
        Ok(())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}

fn same_source(left: &SourceSpec, right: &SourceSpec) -> bool {
    match (left.kind, right.kind) {
        (SourceKind::Path, SourceKind::Path) => left.resolved_path == right.resolved_path,
        (SourceKind::Github, SourceKind::Github) | (SourceKind::Git, SourceKind::Git) => {
            left.value == right.value
        }
        _ => false,
    }
}

fn fail(error: String) -> ExitCode {
    eprintln!("error: {error}");
    ExitCode::from(1)
}

fn strip_path_prefix(path: &Path) -> PathBuf {
    path.to_str()
        .and_then(|value| value.strip_prefix("path:"))
        .map(PathBuf::from)
        .unwrap_or_else(|| path.to_path_buf())
}

fn print_text(output: &ListOutput, selection: &Selection, tree: bool, explain: bool) {
    for skill in &output.skills {
        let tags = if skill.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", skill.tags.join(", "))
        };
        println!("{}\t{}{}", skill.path, skill.name, tags);
        if tree {
            for file in &skill.files {
                println!("  {file}");
            }
        }
    }
    if explain {
        println!();
        println!("Explanation:");
        if output.shadowed.is_empty() {
            println!("  Shadowed: none");
        } else {
            println!("  Shadowed:");
            for shadow in &output.shadowed {
                println!(
                    "    {}: {} wins over {}",
                    shadow.name, shadow.winner, shadow.path
                );
            }
        }
        for decision in &selection.decisions {
            println!("  {}: {}", decision.path, decision.reason);
        }
        for selector in &selection.missing_includes {
            println!("  missing include selector: {selector}");
        }
        for resolution in &selection.selector_resolutions {
            println!(
                "  {}: {} wins over {}",
                resolution.selector,
                resolution.winner,
                resolution.shadowed.join(", ")
            );
        }
        for selector in &selection.missing_excludes {
            println!("  missing exclude selector: {selector}");
        }
    }
    for selector in &selection.missing_includes {
        if !explain {
            eprintln!("warning: missing include selector: {selector}");
        }
    }
    for selector in &selection.missing_excludes {
        if !explain {
            eprintln!("warning: missing exclude selector: {selector}");
        }
    }
    for diagnostic in &output.diagnostics {
        let level = match diagnostic.level {
            DiagnosticLevel::Error => "error",
            DiagnosticLevel::Warning => "warning",
        };
        if let Some(path) = &diagnostic.path {
            eprintln!("{level}: {path}: {}", diagnostic.message);
        } else {
            eprintln!("{level}: {}", diagnostic.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn strips_only_the_explicit_path_prefix() {
        assert_eq!(
            strip_path_prefix(&PathBuf::from("path:demo")).as_path(),
            Path::new("demo")
        );
        assert_eq!(
            strip_path_prefix(&PathBuf::from("demo")).as_path(),
            Path::new("demo")
        );
    }
}
