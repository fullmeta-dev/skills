mod config;
mod lifecycle;
mod source;

use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{
    generate,
    shells::{Bash, Fish, PowerShell, Zsh},
};
use config::{Config, Scope};
use serde::Serialize;
use skills::{discover_with_subpath, DiagnosticLevel, Explain, Selection, Skill};
use source::{
    canonical_version, local_pin, resolve_git_pin, validate_subpath, SourceKind, SourceSpec,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};

#[derive(Debug, Parser)]
#[command(
    name = "skills",
    version,
    about = "Discover and manage Agent Skills from immutable source snapshots",
    long_about = "Discover and manage Agent Skills from immutable source snapshots. Choose a friendly source and selectors; skills resolves an immutable pin, while only upgrade follows an explicit track.",
    after_help = "Normal behavior: list is read-only; use, upgrade, and remove edit the selected mise config; install, sync, and prune only touch links owned by that config root. Failures are reported on stderr with a non-zero exit status and leave unrelated files untouched.\n\nScope: project is the default; use --global, --path PATH, or --env ENV to select a config.\n\nExamples:\n  skills list path:../skills-source --json\n  skills use fullmeta-dev/skills code-review --agent pi\n  skills install\n  skills sync --locked\n  skills status --json\n  skills completions bash"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List discovered skills, optionally applying selectors.
    #[command(
        after_help = "Normal: reads the local source and prints selected skills; missing selectors are warnings, not failures. Failure: malformed source data, unsafe links, or discovery errors return non-zero without editing files. Scope: source and optional --subpath only; no mise config is read. Example: skills list path:../skills-source --include tag:web --tree"
    )]
    List(ListArgs),
    /// Declare a source and selected skills, resolving its immutable pin.
    #[command(
        after_help = "Normal: writes a source declaration, selectors, agents, and a direct postinstall sync hook to the selected mise config. Git sources resolve a full commit; local sources resolve a tree pin. Failure: invalid source, selector policy, agent, ref, or config leaves the file unchanged. Scope: project by default, or --global/--path/--env. Example: skills use fullmeta-dev/skills code-review --agent pi --agent claude"
    )]
    Use(UseArgs),
    /// Refresh immutable pins for declared sources. This never installs or syncs.
    #[command(
        after_help = "Normal: resolves each declared source's explicit track and changes only its immutable version. Without --locked, this is the only command that follows track. Failure: an undeclared tool, invalid source, or unavailable ref returns non-zero and does not install or sync. Scope: project by default, or --global/--path/--env. Example: skills upgrade skills:fullmeta-dev/skills"
    )]
    Upgrade(UpgradeArgs),
    /// Install selected configuration tools through normal mise, then sync links.
    #[command(
        after_help = "Normal: runs mise install against only the selected config and then directly runs locked sync. Failure: the selected config is left unchanged and unmanaged targets are untouched. Scope: project by default, or --global/--path/--env. Example: skills install --env development"
    )]
    Install(InstallArgs),
    /// Update the CLI and its compatible backend as one safe pair.
    #[command(
        name = "self-update",
        after_help = "Normal: discovers the latest release asset, installs the selected CLI release, updates its pinned backend, and commits config only after both succeed. --dry-run prints the pair and config plan without installing or editing. Failure: the old config and backend declaration remain usable. Scope: global by default, or --path/--env. Example: skills self-update"
    )]
    SelfUpdate(SelfUpdateArgs),
    /// Diagnose executable collisions and selected mise ownership.
    #[command(
        after_help = "Normal: reports PATH and mise ownership for the public skills executable without changing files. Failure: unavailable inspection commands are reported. Scope: selected project/global mise config. Example: skills doctor --global"
    )]
    Doctor(DoctorArgs),
    /// Converge the selected configuration without following update tracks.
    #[command(
        after_help = "Normal: verifies installed immutable artifacts and repairs only links owned by this config root; --locked forbids version resolution. Failure: missing artifacts, local drift, unsafe links, or conflicts are reported and unmanaged targets remain untouched. Scope: project by default, or --global/--path/--env. Example: skills sync --locked"
    )]
    Sync(SyncArgs),
    /// Show declared skill sources and their link state.
    #[command(
        after_help = "Normal: reports declarations, immutable versions, and active, missing, dangling, or conflicting links without changing files. Failure: invalid config data returns non-zero. Scope: project by default, or --global/--path/--env. Example: skills status --global --json"
    )]
    Status(StatusArgs),
    /// Remove a declaration or selected entries from its include policy.
    #[command(
        after_help = "Normal: removes selected includes or the whole declaration, syncs owned links, and delegates normal mise artifact removal only when unreferenced. Failure: unknown tools, selectors, or changed unmanaged links are reported without deleting unrelated targets. Scope: project by default, or --global/--path/--env. Example: skills remove skills:fullmeta-dev/skills code-review"
    )]
    Remove(RemoveArgs),
    /// Remove stale links recorded for the selected configuration root.
    #[command(
        after_help = "Normal: removes only stale links recorded for this config root and never materializes desired links. Failure: changed or unmanaged targets are reported and left untouched. Scope: project by default, or --global/--path/--env. Example: skills prune --global"
    )]
    Prune(PruneArgs),
    /// Report strict Agent Skills conformance without blocking normal use.
    #[command(
        visible_alias = "check",
        after_help = "Normal: prints non-blocking diagnostics for strict Agent Skills consumers; the intentional top-level tags extension is reported but remains valid for normal discovery and installation. Failure: unreadable sources or ordinary discovery errors return non-zero. Scope: source and optional --subpath only; no config, network, or installation side effects. Example: skills validate path:../skills-source --json"
    )]
    Validate(ValidateArgs),
    /// Generate a completion script for one supported shell.
    #[command(
        visible_alias = "completion",
        after_help = "Normal: writes a completion script to stdout and never edits config or contacts a source. Failure: unsupported shells are rejected before output. Scope: shell argument only; no project or global config is read. Example: skills completions bash > ~/.local/share/bash-completion/completions/skills"
    )]
    Completions(CompletionsArgs),
}

#[derive(Debug, Args)]
struct ScopeArgs {
    /// Select mise's environment-specific config (`mise.<env>.toml`).
    #[arg(long, env = "MISE_ENV", value_name = "ENV")]
    env: Option<String>,
    /// Edit the user-owned global mise config.
    #[arg(long, conflicts_with = "path")]
    global: bool,
    /// Edit this mise config file (or mise.toml inside a directory).
    #[arg(long, value_name = "PATH", conflicts_with = "global")]
    path: Option<PathBuf>,
}

impl ScopeArgs {
    fn scope(&self) -> Scope {
        if let Some(environment) = &self.env {
            config::validate_environment_name(environment).expect("clap validates environment");
            std::env::set_var("MISE_ENV", environment);
        }
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
struct InstallArgs {
    #[command(flatten)]
    scope: ScopeArgs,
    /// Require mise's existing lockfile data without resolving tool URLs.
    #[arg(long)]
    locked: bool,
    /// Show normal mise installation work without changing files.
    #[arg(long)]
    dry_run: bool,
    /// Explicitly accept an unrelated existing `skills` executable.
    #[arg(long)]
    accept_collision: bool,
}

#[derive(Debug, Args)]
struct SelfUpdateArgs {
    /// Update the global config by default, or select one config with --path.
    #[command(flatten)]
    scope: SelfUpdateScopeArgs,
    /// Install a specific release from the compatibility manifest.
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// Show the compatible pair and planned changes without installing or editing.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct SelfUpdateScopeArgs {
    /// Update the user-owned global mise config (the default).
    #[arg(long, conflicts_with = "path")]
    global: bool,
    /// Update this mise config file.
    #[arg(long, value_name = "PATH", conflicts_with = "global")]
    path: Option<PathBuf>,
    /// Select an environment-specific config.
    #[arg(long, env = "MISE_ENV", value_name = "ENV")]
    env: Option<String>,
}

impl SelfUpdateScopeArgs {
    fn scope(&self) -> Scope {
        if let Some(environment) = &self.env {
            config::validate_environment_name(environment).expect("clap validates environment");
            std::env::set_var("MISE_ENV", environment);
        }
        self.path.clone().map(Scope::Path).unwrap_or(Scope::Global)
    }
}

#[derive(Debug, Args)]
struct DoctorArgs {
    #[command(flatten)]
    scope: ScopeArgs,
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

#[derive(Debug, Args)]
struct ValidateArgs {
    /// Local checkout or snapshot directory. `path:` is accepted as a prefix.
    source: PathBuf,
    /// Select a source subpath before validation.
    #[arg(long)]
    subpath: Option<PathBuf>,
    /// Emit stable machine-readable diagnostics.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CompletionsArgs {
    /// Shell to generate: bash, zsh, fish, or powershell.
    #[arg(value_parser = ["bash", "zsh", "fish", "powershell"])]
    shell: String,
}

#[derive(Debug, Serialize)]
struct ValidateOutput {
    source: String,
    diagnostics: Vec<skills::Diagnostic>,
}

#[derive(Debug, Serialize)]
struct ListOutput {
    source: String,
    catalog: skills::CatalogSummary,
    skills: Vec<Skill>,
    shadowed: Vec<skills::ShadowedSkill>,
    diagnostics: Vec<skills::Diagnostic>,
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
    selector_resolutions: Vec<skills::SelectorResolution>,
}

#[derive(Debug, Serialize)]
struct StatusOutput {
    config: String,
    environment: String,
    tools: Vec<config::ToolInfo>,
    links: Vec<lifecycle::LinkStatus>,
    diagnostics: Vec<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        // Clap's generated help is intentionally the only output for a bare
        // invocation and is always side-effect free.
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
        Command::Install(args) => run_install(args),
        Command::SelfUpdate(args) => run_self_update(args),
        Command::Doctor(args) => run_doctor(args),
        Command::Sync(args) => run_sync(args),
        Command::Status(args) => run_status(args),
        Command::Remove(args) => run_remove(args),
        Command::Prune(args) => run_prune(args),
        Command::Validate(args) => run_validate(args),
        Command::Completions(args) => run_completions(args),
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

fn run_validate(args: ValidateArgs) -> ExitCode {
    let source = strip_path_prefix(&args.source);
    let discovery = match discover_with_subpath(&source, args.subpath.as_deref()) {
        Ok(discovery) => discovery,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };
    let mut diagnostics = discovery.diagnostics.clone();
    diagnostics.extend(discovery.strict_diagnostics());
    let output = ValidateOutput {
        source: discovery.source().display().to_string(),
        diagnostics,
    };
    if args.json {
        match serde_json::to_string_pretty(&output) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("error: cannot encode validation diagnostics as JSON: {error}");
                return ExitCode::from(1);
            }
        }
    } else if output.diagnostics.is_empty() {
        println!(
            "strict Agent Skills validation passed for {}",
            output.source
        );
    } else {
        println!("strict Agent Skills diagnostics for {}:", output.source);
        for diagnostic in &output.diagnostics {
            let level = match diagnostic.level {
                DiagnosticLevel::Error => "error",
                DiagnosticLevel::Warning => "warning",
            };
            if let Some(path) = &diagnostic.path {
                println!("{level}: {path}: {}", diagnostic.message);
            } else {
                println!("{level}: {}", diagnostic.message);
            }
        }
        if !discovery.has_errors() {
            println!("normal discovery and installation remain non-blocking");
        }
    }
    if discovery.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn run_completions(args: CompletionsArgs) -> ExitCode {
    let mut command = Cli::command();
    let mut stdout = io::stdout();
    match args.shell.as_str() {
        "bash" => generate(Bash, &mut command, "skills", &mut stdout),
        "zsh" => generate(Zsh, &mut command, "skills", &mut stdout),
        "fish" => generate(Fish, &mut command, "skills", &mut stdout),
        "powershell" => generate(PowerShell, &mut command, "skills", &mut stdout),
        _ => unreachable!("shell is constrained by clap"),
    }
    ExitCode::SUCCESS
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

fn run_install(args: InstallArgs) -> ExitCode {
    let scope = args.scope.scope();
    match Config::load(&scope).and_then(|config| {
        let collisions = collision_diagnostics(&config);
        if !collisions.is_empty() && !args.accept_collision {
            return Err(format!(
                "refusing to install because the public skills executable is shadowed:\n{}\nuse --accept-collision only after inspecting the owners",
                collisions.join("\n")
            ));
        }
        for collision in collisions {
            eprintln!("warning: {collision}");
        }
        let output = run_normal_mise_install(&config, args.locked, args.dry_run)?;
        if !output.trim().is_empty() {
            print!("{output}");
        }
        if args.dry_run {
            return Ok(());
        }
        let report = lifecycle::sync(&config, args.scope.is_global())?;
        println!(
            "installed selected mise tools and synced {} link(s), removed {} link(s) in {}",
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

fn run_doctor(args: DoctorArgs) -> ExitCode {
    let scope = args.scope.scope();
    match Config::load(&scope).map(|config| {
        println!("config: {}", config.path.display());
        println!(
            "environment: {}",
            std::env::var("MISE_ENV").unwrap_or_else(|_| "default".into())
        );
        let collisions = collision_diagnostics(&config);
        if collisions.is_empty() {
            println!("skills executable: no unrelated owner detected");
        } else {
            for collision in collisions {
                println!("{collision}");
            }
        }
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}

#[derive(Debug, serde::Deserialize)]
struct CompatibilityManifest {
    schema_version: u32,
    latest: Option<String>,
    releases: std::collections::BTreeMap<String, CompatibilityRelease>,
}

#[derive(Debug, serde::Deserialize)]
struct CompatibilityRelease {
    minimum_mise: String,
    plugin_url: String,
    plugin_revision: String,
}

#[derive(Debug, serde::Deserialize)]
struct LatestRelease {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, serde::Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

fn run_self_update(args: SelfUpdateArgs) -> ExitCode {
    match self_update_command(args) {
        Ok((version, path, dry_run)) => {
            if dry_run {
                println!(
                    "would update skills CLI and backend to {version} in {}",
                    path.display()
                );
            } else {
                println!(
                    "updated skills CLI and backend to {version} in {}",
                    path.display()
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail(error),
    }
}

fn self_update_command(args: SelfUpdateArgs) -> Result<(String, PathBuf, bool), String> {
    let scope = args.scope.scope();
    let mut config = Config::load(&scope)?;
    let before = config.document.to_string();
    let manifest = load_compatibility_manifest()?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported release compatibility manifest schema {}",
            manifest.schema_version
        ));
    }
    let version = args
        .version
        .or(manifest.latest.clone())
        .or_else(|| {
            manifest
                .releases
                .keys()
                .max_by(|left, right| compare_versions(left, right))
                .cloned()
        })
        .ok_or_else(|| "release compatibility manifest contains no releases".to_string())?;
    if version.is_empty()
        || version.starts_with('-')
        || !version
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
    {
        return Err("release version must be a safe semver-like value".into());
    }
    let release = manifest
        .releases
        .get(&version)
        .ok_or_else(|| format!("release {version:?} is absent from the compatibility manifest"))?;
    require_mise_version(&release.minimum_mise)?;
    if release.plugin_url.is_empty()
        || release.plugin_url.starts_with('-')
        || release.plugin_url.contains(['\0', '\n', '\r'])
        || release.plugin_url.chars().any(char::is_whitespace)
        || release.plugin_revision.len() != 40
        || !release
            .plugin_revision
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err("compatibility manifest contains an unsafe backend pin".into());
    }

    let cli_key = "github:fullmeta-dev/skills";
    let tools = config
        .document
        .get("tools")
        .and_then(toml_edit::Item::as_table)
        .ok_or_else(|| "selected config has no [tools] table".to_string())?;
    let cli = tools
        .get(cli_key)
        .ok_or_else(|| format!("selected config does not declare {cli_key:?}"))?;
    if cli.as_str().is_some() {
        return Err(format!(
            "CLI declaration {cli_key:?} must be a table with bin = \\\"skills\\\""
        ));
    }
    let cli_table = cli
        .as_table()
        .ok_or_else(|| format!("CLI declaration {cli_key:?} must be a table"))?;
    if cli_table.get("bin").and_then(toml_edit::Item::as_str) != Some("skills") {
        return Err(format!(
            "CLI declaration {cli_key:?} must expose bin = \\\"skills\\\""
        ));
    }
    // The backend is installed in mise's plugin data, not necessarily
    // repeated in the selected config. Capture that installed state before
    // changing it so a failed pair update can restore the exact old plugin.
    let old_plugin = inspect_installed_plugin()?;

    let collisions = collision_diagnostics(&config);
    if !collisions.is_empty() {
        return Err(format!(
            "refusing self-update due to executable collision:\\n{}",
            collisions.join("\\n")
        ));
    }

    let new_plugin = format!("{}#{}", release.plugin_url, release.plugin_revision);
    if args.dry_run {
        println!("would install CLI github:fullmeta-dev/skills@{version} through mise");
        println!("would install backend plugin skills from {new_plugin}");
        println!("would update only {}", config.path.display());
        return Ok((version, config.path, true));
    }

    // Install the new executable first, without mutating the selected config.
    // This keeps the currently running binary and old declaration usable when
    // a download, backend update, or compatibility check fails.
    let install = run_normal_mise_install_at(
        &config,
        &[format!("github:fullmeta-dev/skills@{version}")],
        false,
        false,
    )?;
    if !install.status.success() {
        return Err(process_failure("mise install", &install));
    }

    install_plugin_pin(&new_plugin)?;

    if let Err(error) = config::update_cli_release(&mut config.document, version.as_str())
        .and_then(|_| config.save_if_changed(&before).map(|_| ()))
    {
        let rollback = restore_installed_plugin(old_plugin.as_deref());
        return Err(match rollback {
            Ok(()) => format!("cannot commit self-update; old config retained: {error}"),
            Err(rollback) => format!(
                "cannot commit self-update; old config retained: {error}; plugin rollback failed: {rollback}"
            ),
        });
    }
    Ok((version, config.path, false))
}

fn load_compatibility_manifest() -> Result<CompatibilityManifest, String> {
    // The local override is deliberately retained as a deterministic test and
    // air-gapped development seam. Normal updates discover the latest release
    // through GitHub's API and consume the immutable asset attached to it;
    // mutable repository branches are never a manifest source.
    if let Ok(location) = std::env::var("SKILLS_COMPATIBILITY_MANIFEST") {
        return parse_compatibility_manifest(&read_manifest_location(&location)?);
    }

    let api = std::env::var("SKILLS_RELEASE_API").unwrap_or_else(|_| {
        "https://api.github.com/repos/fullmeta-dev/skills/releases/latest".into()
    });
    let release_bytes = read_release_api(&api)?;
    let release: LatestRelease = serde_json::from_slice(&release_bytes)
        .map_err(|error| format!("cannot parse latest GitHub release response: {error}"))?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == "release-compatibility.json")
        .ok_or_else(|| {
            format!(
                "latest GitHub release {} has no release-compatibility.json asset",
                release.tag_name
            )
        })?;
    parse_compatibility_manifest(&read_manifest_location(&asset.browser_download_url)?)
}

fn read_release_api(location: &str) -> Result<Vec<u8>, String> {
    if location.contains("://") {
        let output = ProcessCommand::new("curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--header",
                "Accept: application/vnd.github+json",
                "--header",
                "X-GitHub-Api-Version: 2022-11-28",
                location,
            ])
            .output()
            .map_err(|error| format!("cannot run curl for latest GitHub release: {error}"))?;
        if !output.status.success() {
            return Err(process_failure("latest GitHub release lookup", &output));
        }
        Ok(output.stdout)
    } else {
        fs::read(location)
            .map_err(|error| format!("cannot read release API fixture {location}: {error}"))
    }
}

fn read_manifest_location(location: &str) -> Result<Vec<u8>, String> {
    if location.contains("://") {
        let output = ProcessCommand::new("curl")
            .args(["--fail", "--location", "--silent", "--show-error", location])
            .output()
            .map_err(|error| format!("cannot run curl for compatibility manifest: {error}"))?;
        if !output.status.success() {
            return Err(process_failure("compatibility manifest download", &output));
        }
        Ok(output.stdout)
    } else {
        fs::read(location)
            .map_err(|error| format!("cannot read compatibility manifest {location}: {error}"))
    }
}

fn parse_compatibility_manifest(contents: &[u8]) -> Result<CompatibilityManifest, String> {
    serde_json::from_slice(contents)
        .map_err(|error| format!("cannot parse compatibility manifest: {error}"))
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let parse = |value: &str| {
        value
            .trim_start_matches('v')
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    parse(left).cmp(&parse(right))
}

fn require_mise_version(minimum: &str) -> Result<(), String> {
    let output = ProcessCommand::new(std::env::var_os("MISE_BIN").unwrap_or_else(|| "mise".into()))
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot determine mise version: {error}"))?;
    if !output.status.success() {
        return Err("cannot determine mise version".into());
    }
    let version_output = String::from_utf8_lossy(&output.stdout);
    let actual = version_output.split_whitespace().next().unwrap_or_default();
    if compare_versions(actual, minimum) == std::cmp::Ordering::Less {
        return Err(format!(
            "skills release requires mise >= {minimum}; found {actual}"
        ));
    }
    Ok(())
}

fn mise_plugin_dir() -> Result<PathBuf, String> {
    std::env::var_os("MISE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/share/mise"))
        })
        .ok_or_else(|| "cannot determine mise data directory for backend plugin".to_string())
        .map(|path| path.join("plugins/skills"))
}

fn inspect_installed_plugin() -> Result<Option<String>, String> {
    let plugin_dir = mise_plugin_dir()?;
    if !plugin_dir.exists() {
        return Ok(None);
    }
    if !plugin_dir.join(".git").exists() {
        return Err(format!(
            "cannot inspect installed skills backend: {} is not a Git plugin checkout",
            plugin_dir.display()
        ));
    }
    let directory = plugin_dir.to_str().ok_or_else(|| {
        format!(
            "cannot inspect installed skills backend path: {}",
            plugin_dir.display()
        )
    })?;
    let url = ProcessCommand::new("git")
        .args(["-C", directory, "config", "--get", "remote.origin.url"])
        .output()
        .map_err(|error| format!("cannot inspect installed backend URL: {error}"))?;
    if !url.status.success() {
        return Err(process_failure("inspect installed backend URL", &url));
    }
    let url = String::from_utf8_lossy(&url.stdout).trim().to_string();
    let revision = ProcessCommand::new("git")
        .args(["-C", directory, "rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("cannot inspect installed backend revision: {error}"))?;
    if !revision.status.success() {
        return Err(process_failure(
            "inspect installed backend revision",
            &revision,
        ));
    }
    let revision = String::from_utf8_lossy(&revision.stdout).trim().to_string();
    if url.is_empty() || revision.is_empty() {
        return Err(format!(
            "installed skills backend has incomplete Git state: {}",
            plugin_dir.display()
        ));
    }
    Ok(Some(format!("{url}#{revision}")))
}

fn restore_installed_plugin(pin: Option<&str>) -> Result<(), String> {
    let plugin_dir = mise_plugin_dir()?;
    let mise = std::env::var_os("MISE_BIN").unwrap_or_else(|| "mise".into());
    if plugin_dir.exists() {
        let remove = ProcessCommand::new(&mise)
            .args(["plugins", "uninstall", "--yes", "skills"])
            .output()
            .map_err(|error| format!("cannot run mise plugin uninstall: {error}"))?;
        if !remove.status.success() {
            return Err(process_failure("mise plugin uninstall", &remove));
        }
    }
    if let Some(pin) = pin {
        let output = ProcessCommand::new(mise)
            .args(["plugins", "install", "skills", pin])
            .output()
            .map_err(|error| format!("cannot run mise plugin install: {error}"))?;
        if !output.status.success() {
            return Err(process_failure("mise plugin rollback install", &output));
        }
    }
    Ok(())
}

fn install_plugin_pin(pin: &str) -> Result<(), String> {
    let (desired_url, desired_revision) = pin.rsplit_once('#').ok_or_else(|| {
        "backend compatibility pin must contain an URL and immutable revision".to_string()
    })?;
    if desired_url.is_empty() || desired_revision.is_empty() {
        return Err("backend compatibility pin must contain an URL and immutable revision".into());
    }
    let old = inspect_installed_plugin()?;
    if old.as_deref() == Some(pin) {
        return Ok(());
    }
    let result = restore_installed_plugin(Some(pin));
    if let Err(error) = result {
        let rollback = restore_installed_plugin(old.as_deref());
        return Err(match rollback {
            Ok(()) => error,
            Err(rollback) => format!("{error}; plugin rollback failed: {rollback}"),
        });
    }
    Ok(())
}

fn run_normal_mise_install(config: &Config, locked: bool, dry_run: bool) -> Result<String, String> {
    let output = run_normal_mise_install_at(config, &[], locked, dry_run)?;
    if !output.status.success() {
        return Err(process_failure("mise install", &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_normal_mise_install_at(
    config: &Config,
    tools: &[String],
    locked: bool,
    dry_run: bool,
) -> Result<std::process::Output, String> {
    let mise = std::env::var_os("MISE_BIN").unwrap_or_else(|| "mise".into());
    let mut command = ProcessCommand::new(mise);
    command
        .current_dir(&config.root)
        .env("MISE_CONFIG_FILE", &config.path)
        .args(["install", "--yes", "--jobs", "1"]);
    if let Ok(environment) = std::env::var("MISE_ENV") {
        if !environment.is_empty() {
            command.args(["--env", &environment]);
        }
    }
    if locked {
        command.arg("--locked");
    }
    if dry_run {
        command.arg("--dry-run");
    }
    for tool in tools {
        command.arg(tool);
    }
    command
        .output()
        .map_err(|error| format!("cannot run normal mise install: {error}"))
}

fn process_failure(action: &str, output: &std::process::Output) -> String {
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if detail.is_empty() {
        format!("{action} failed with status {}", output.status)
    } else {
        format!("{action} failed: {detail}")
    }
}

fn collision_diagnostics(config: &Config) -> Vec<String> {
    let mut diagnostics = Vec::new();
    let current = std::env::current_exe()
        .ok()
        .and_then(|path| fs::canonicalize(path).ok());
    if let Some(path) = skills_on_path() {
        let canonical = fs::canonicalize(&path).ok();
        if current.is_none() || canonical != current {
            diagnostics.push(format!("PATH owner: {}", path.display()));
        }
    }
    let mise = std::env::var_os("MISE_BIN").unwrap_or_else(|| "mise".into());
    if let Ok(output) = ProcessCommand::new(mise)
        .current_dir(&config.root)
        .env("MISE_CONFIG_FILE", &config.path)
        .args(["which", "skills"])
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                let resolved = PathBuf::from(&path);
                let canonical = fs::canonicalize(&resolved).ok();
                if current.is_none() || canonical != current {
                    diagnostics.push(format!("mise owner: {path}"));
                }
            }
        }
    }
    diagnostics.sort();
    diagnostics.dedup();
    diagnostics
}

fn skills_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    // Inspect both spellings on every host. This keeps diagnostics portable
    // when a Windows-created skills.exe is present on a Unix PATH (and vice
    // versa), while the current executable comparison still filters our own
    // binary.
    for directory in std::env::split_paths(&path) {
        for name in ["skills", "skills.exe"] {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
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
            environment: std::env::var("MISE_ENV").unwrap_or_else(|_| "default".into()),
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
            println!("config\t{}", output.config);
            println!("environment\t{}", output.environment);
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
    let removed_tool = config::tool_info(&config.document, &key)?
        .ok_or_else(|| format!("skill tool {key:?} is not declared"))?;
    config::remove_tool(&mut config.document, &key, &args.selectors)?;
    let tool_still_declared = config::tool_info(&config.document, &key)?.is_some();
    let remaining_reference = has_remaining_artifact_reference(&config, &removed_tool)?;
    config.save_if_changed(&before)?;
    let report = lifecycle::sync(&config, args.scope.is_global())?;
    for conflict in report.conflicts {
        eprintln!("warning: {conflict}");
    }

    // Partial selector removal leaves the declaration (and therefore its
    // artifact) in use. Even after removing the whole table, keep the normal
    // mise artifact whenever another declaration in this config identifies
    // the same source+version.
    if !tool_still_declared && !remaining_reference {
        uninstall_artifact(&config, &removed_tool)?;
    }
    Ok((before != config.document.to_string(), config.path))
}

fn has_remaining_artifact_reference(
    config: &Config,
    removed: &config::ToolInfo,
) -> Result<bool, String> {
    let removed_source =
        SourceSpec::from_declared(&removed.source, &config.root, Some(&removed.key))?;
    let removed_version = canonical_version(&removed.version, removed_source.kind)?;
    if document_references_artifact(
        &config.root,
        &config.document,
        &removed_source,
        &removed_version,
        Some(&removed.key),
    )? {
        return Ok(true);
    }

    // A global and a project declaration can share one mise installation even
    // though each has its own lifecycle manifest. Inspect the normal scopes
    // that can be active from this invocation before delegating removal; do
    // not attempt a repository-wide configuration or garbage-collection scan.
    for scope in [Scope::Global, Scope::Project] {
        let path = scope.config_path()?;
        if same_config_path(&path, &config.path) || !path.is_file() {
            continue;
        }
        let other = Config::load_path(&path)?;
        if document_references_artifact(
            &other.root,
            &other.document,
            &removed_source,
            &removed_version,
            None,
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn document_references_artifact(
    root: &Path,
    document: &toml_edit::DocumentMut,
    removed_source: &SourceSpec,
    removed_version: &str,
    removed_key: Option<&str>,
) -> Result<bool, String> {
    for tool in config::skill_tools(document)? {
        if removed_key.is_some_and(|key| key == tool.key) {
            continue;
        }
        let source = SourceSpec::from_declared(&tool.source, root, Some(&tool.key))?;
        let version = canonical_version(&tool.version, source.kind)?;
        if source.canonical == removed_source.canonical && version == removed_version {
            return Ok(true);
        }
    }
    Ok(false)
}

fn same_config_path(left: &Path, right: &Path) -> bool {
    fs::canonicalize(left).ok() == fs::canonicalize(right).ok() || left == right
}

fn uninstall_artifact(config: &Config, tool: &config::ToolInfo) -> Result<(), String> {
    let mise = std::env::var_os("MISE_BIN").unwrap_or_else(|| "mise".into());
    let selector = format!("{}@{}", tool.key, tool.version);
    let output = ProcessCommand::new(&mise)
        .current_dir(&config.root)
        .env("MISE_CONFIG_FILE", &config.path)
        .args(["uninstall", "--yes", "--quiet", "--", &selector])
        .output()
        .map_err(|error| format!("cannot run normal mise artifact removal: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            format!("normal mise artifact removal failed for {selector}")
        } else {
            format!("normal mise artifact removal failed for {selector}: {detail}")
        });
    }
    Ok(())
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
