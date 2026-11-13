mod cache;
mod config;
mod git_exclude;
mod lifecycle;
mod source;
mod style;

use cache::Cache;
use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use clap_complete::{
    generate,
    shells::{Bash, Fish, PowerShell, Zsh},
};
use config::{Config, Scope};
use serde::Serialize;
use skills::{discover_with_subpath, DiagnosticLevel, Explain, Selection, Skill};
use source::{
    canonical_version, local_pin, prepare_discovery_source, resolve_git_pin, validate_subpath,
    SourceKind, SourceSpec,
};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};

#[derive(Debug, Parser)]
#[command(
    name = "skills",
    version,
    about = "Discover and manage Agent Skills from immutable source snapshots",
    long_about = "Discover Agent Skills from local directories and Git repositories and declare immutable source snapshots as ordinary mise tools.\n\nStart with `skills list SOURCE` to inspect names, descriptions, tags, aliases, and files. `skills use SOURCE [SELECTOR...] --agent AGENT...` records the selection and an immutable pin. `skills install` runs normal mise installation and activates owned links. Only `skills upgrade` follows a recorded update track; install and sync never do.\n\nRun `skills COMMAND --help` for that command's arguments, defaults, side effects, and failure behavior.",
    after_help = r#"Bootstrap (once per machine):
  Requires mise >= 2026.11.7, Git, curl (for online self-update), and an
  activated mise shell. Published
  binaries support Linux x86_64 and macOS arm64 only. Pick the compatible
  CLI/backend pair from the latest release's immutable release-compatibility.json
  asset (releases[VERSION].plugin_revision); do not guess the backend commit.

    mise plugins install skills "https://github.com/fullmeta-dev/skills.git#<compatible-plugin-commit>"
    mise use --global --pin "github:fullmeta-dev/skills@<release>"

  In the global mise config the CLI is an ordinary tool table:
    [tools."github:fullmeta-dev/skills"]
    version = "<release>"
    bin = "skills"

  No task wrapper or repeated [plugins] entry is required. Once a release
  exists, `skills self-update` installs the latest compatible pair.

Sources (list and use):
  owner/repository          GitHub shorthand
  github:owner/repository   explicit GitHub source
  git:https://host/o/r.git  explicit Git source (https, ssh, or file URL)
  path:DIR or plain DIR     local directory
  list resolves relative local paths from the working directory; use resolves
  them from the selected config's directory. A bare value that looks like
  owner/repository is read as GitHub shorthand even when a matching directory
  exists; write ./dir or path:dir to force a local directory.

Selectors:
  name or alias      basename, frontmatter name, or catalog alias
  tag:web            requires tag web
  tag:web+frontend   requires both tags
  skills/name        repository-relative skill-directory path
  'skills/**'        repository-relative glob (quote globs in the shell)

  Includes are ORed; tags joined with + are ANDed; excludes always win.
  No includes selects every discovered skill (subject to excludes). Ambiguous
  bare selectors use the lexically first path; list --explain shows the
  decision. Missing selectors are reported by list (text and --explain) and do
  not fail selection. list accepts positional
  selectors and repeated --include/--exclude; use takes positional includes.

Configuration written by use:
  skills use fullmeta-dev/skills mise --agent pi

  [tools."skills:fullmeta-dev/skills"]
  source = "github:fullmeta-dev/skills"
  version = "rev:<resolved-40-character-commit>"
  track = "<resolved-branch>"
  include = ["mise"]
  agents = ["pi"]

  [hooks]
  postinstall = ["skills sync --locked"]

  version is rev:<full-commit> for Git sources and tree-<sha256> for local
  sources. track records update intent; install and sync never follow it, and
  `skills upgrade` re-resolves it. There is no version = "latest": mise
  versions must be immutable, so "latest" is expressed as use (which records
  the remote default branch as track) followed by upgrade and install.
  include is written only when selectors are given. --subpath writes
  source_subpath = "relative/directory". exclude = ["tag:experimental"] is
  supported but must be added to the tool table by hand. A global or --path
  declaration writes the matching --global/--path flags into the hook, because
  mise runs postinstall hooks in the current project directory. Existing hooks
  are preserved and the sync action is added once.

Agents (project -> global):
  agents, codex, cursor -> .agents/skills  -> ~/.agents/skills
  pi                    -> .pi/skills      -> ~/.pi/agent/skills
  claude                -> .claude/skills  -> ~/.claude/skills

Scope:
  Config commands default to the nearest ancestor mise.toml, or ./mise.toml.
  --global selects the user config (~/.config/mise/config.toml by default).
  --path PATH selects a file, or mise.toml inside a directory.
  --env ENV (or MISE_ENV) inserts .ENV before the extension:
  mise.toml -> mise.development.toml, config.toml -> config.development.toml.
  --global and --path are mutually exclusive; --env combines with either.
  self-update defaults to global. --path alone does not select global agent
  targets; --global does.
  Commit shared project/profile configs. Select machine-local files with
  --path mise.local.toml and normally gitignore them.

Lifecycle:
  list -> use -> install -> status
  use edits config only; it never installs or activates.
  upgrade refreshes existing pins; run install afterward to activate.
  install runs normal mise install for the whole config, then syncs links.
  sync repairs owned links from already-installed artifacts; it never
  installs, fetches updates, or follows tracks.
  remove edits policy, syncs, and may uninstall an unreferenced artifact.
  prune removes stale owned links and never creates desired links.

Ownership and safety:
  Activation creates relative symlinks named by skill into immutable mise
  installations. Ownership is recorded per config directory under
  $MISE_STATE_DIR/skills/ (not per filename or environment).
  Unmanaged files, directories, symlinks, and changed managed links are
  collisions: they are never overwritten or adopted unless --force is given to
  sync or install. --force removes a conflicting symlink, or moves a
  conflicting file or directory aside to .skills-replaced-<name>-<timestamp>
  before linking. Link collisions are
  reported as warnings and may not fail the command; confirm with status --json.
  Executable collisions are separate: install refuses a foreign `skills` on
  PATH or in mise ownership unless --accept-collision is given. No executable
  is ever deleted automatically. Generated links are listed in the project's
  .git/info/exclude so they do not appear in git status (a linked worktree is
  left alone because it shares info/exclude); they are machine-specific and
  must not be committed.

Diagnostics:
  validate (alias check) checks local sources for strict Agent Skills
  conformance; the intentional top-level tags extension is reported but does
  not fail normal discovery.
  doctor (read-only) compares PATH and mise owners with the running binary;
  absence of diagnostics is not proof every inspection succeeded.
  self-update installs the latest compatible CLI/backend pair; --dry-run still
  inspects the release and compatibility manifest. Set SKILLS_RELEASE_API or
  SKILLS_COMPATIBILITY_MANIFEST to local JSON fixtures for offline runs.

Caching (list):
  Remote sources are fetched at an immutable commit. Checkouts are cached
  under $SKILLS_CACHE_DIR (else $MISE_CACHE_DIR/skills, else
  $XDG_CACHE_HOME/skills, else ~/.cache/skills) and reused forever because a
  revision cannot change.
  Resolved refs are cached for SKILLS_CACHE_TTL seconds (default 300).
  --refresh re-resolves refs; SKILLS_NO_CACHE disables caching entirely;
  `skills cache` reports or cleans the cache.

Examples:
  skills list fullmeta-dev/skills --json --explain
  skills list fullmeta-dev/skills 'skills/**' --refresh
  skills use fullmeta-dev/skills mise --agent pi --agent claude
  skills install && skills status --json
  skills upgrade && skills install
  skills sync --force
  skills use --env development fullmeta-dev/skills mise --agent agents
  skills sync --locked
  skills doctor --global
  skills cache --clean
  skills self-update --dry-run
  skills remove skills:fullmeta-dev/skills mise
  skills prune"#
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List discovered skills, optionally applying selectors.
    #[command(
        after_help = "Normal: read-only discovery; no config is read or changed. SOURCE is required (path:DIR, a local directory, owner/repository, github:owner/repository, or git/https/ssh/file URL). Selectors are optional; with none, every discovered skill is listed. Remote sources are fetched at an immutable commit into a read-only checkout that is cached by revision and reused. --refresh re-resolves refs instead of using the cached resolution; SKILLS_CACHE_TTL and SKILLS_NO_CACHE control the cache. Without --track or --reference, Git sources use the remote default branch. Text output lists names, descriptions, tags, aliases, source, and revision; --json adds artifact files and selector data, --tree renders artifact files, and --explain explains shadowing and selector decisions. Missing selectors are reported in text output (JSON carries them as data without a warning); they never fail selection. Failure: malformed metadata, unsafe links, or unavailable refs return non-zero without editing files. Example: skills list fullmeta-dev/skills --json --explain"
    )]
    List(ListArgs),
    /// Declare a source and selected skills, resolving its immutable pin.
    #[command(
        after_help = "Normal: writes a source declaration, selectors, agents, and a direct postinstall sync hook to the selected mise config. SOURCE and at least one --agent are required. Positional selectors replace the include list when supplied and preserve it when omitted; a new declaration with no selectors selects every discovered skill. Supplied agents replace the recorded agent list. Git sources resolve a full commit; local sources resolve a tree digest. use edits config only: it never installs or activates. Run skills list first to inspect matches; use records selectors without checking that they match. --locked requires an existing declaration of the same source and subpath and preserves its version and track. --reference resolves a ref now and records it as track; --track does the same for a branch or tag. Failure: an invalid source, agent, ref, or config value returns non-zero before saving config. Scope: project by default, --global, --path PATH, or --env ENV. Example: skills use fullmeta-dev/skills mise --agent pi --agent claude"
    )]
    Use(UseArgs),
    /// Refresh immutable pins for declared sources. This never installs or syncs.
    #[command(
        after_help = "Normal: with no TOOL arguments, refreshes every skills declaration; otherwise only the named tool keys, with or without the skills: prefix. Git sources resolve their stored track, or the default branch when absent; local sources recompute the tree digest. Only changed version fields are saved, after every requested refresh succeeds. upgrade never installs or syncs; run skills install afterward to activate. --locked checks the requested declarations without resolving or changing any version. Failure: an undeclared tool, invalid source, or unavailable ref returns non-zero without installing or syncing. Scope: project by default, or --global/--path/--env. Example: skills upgrade skills:fullmeta-dev/skills"
    )]
    Upgrade(UpgradeArgs),
    /// Install selected configuration tools through normal mise, then sync links.
    #[command(
        after_help = "Normal: runs normal mise install for every tool in the selected config (including non-skill tools), then directly runs locked sync to activate owned links. No positional tools are accepted. Source pins never move. --dry-run previews the mise installation and skips the direct sync; collision checks still run. --accept-collision bypasses only the public executable-owner check, never skill-link ownership. --force replaces unmanaged or changed link targets instead of reporting them as collisions. Failure: installation errors return non-zero and earlier installed artifacts may remain. Link collisions may only warn; inspect skills status --json. Scope: project by default, --global, --path PATH, or --env ENV. Example: skills install --env development"
    )]
    Install(InstallArgs),
    /// Update the CLI and its compatible backend as one safe pair.
    #[command(
        name = "self-update",
        after_help = "Normal: discovers the latest release's release-compatibility.json asset, checks minimum mise and executable ownership, installs the CLI, updates the installed skills backend plugin, then saves the selected config only after both succeed. Requires an existing [tools.\"github:fullmeta-dev/skills\"] table with version and bin = \"skills\"; this is not a bootstrap command. Default scope is global; --version must name an entry in the compatibility manifest. --dry-run still inspects the release and manifest but installs and edits nothing. Failure: returns non-zero and retains the old config; backend restoration is attempted and rollback failures are reported. Set SKILLS_RELEASE_API or SKILLS_COMPATIBILITY_MANIFEST to local JSON fixtures for offline runs. Example: skills self-update --dry-run"
    )]
    SelfUpdate(SelfUpdateArgs),
    /// Diagnose executable collisions and selected mise ownership.
    #[command(
        after_help = "Normal: read-only. Reports the effective config and environment and compares PATH and mise owners with the running executable; it does not inspect or repair skill-link collisions. Detected owner differences are informational. Failure: unreadable or invalid selected config returns non-zero. An unsuccessful mise ownership inspection may be omitted, so absence of diagnostics is not proof every inspection succeeded. Scope: selected project/global mise config. Example: skills doctor --global"
    )]
    Doctor(DoctorArgs),
    /// Converge the selected configuration without following update tracks.
    #[command(
        after_help = "Normal: verifies installed immutable artifacts and converges links owned by this config root. It never installs, fetches updates, or follows tracks. It creates missing desired links, updates unchanged owned links, removes stale owned links, and writes ownership state. Missing or invalid artifacts and local-source drift return non-zero; run install, or use/upgrade then install. --locked is accepted for the generated postinstall hook; sync never resolves versions, so it does not change behavior. Unmanaged or changed targets are left untouched and reported as warnings; --force replaces them instead (symlinks are removed, files and directories are moved aside). Scope: project by default, or --global/--path/--env. Example: skills sync --locked"
    )]
    Sync(SyncArgs),
    /// Show declared skill sources and their link state.
    #[command(
        after_help = "Normal: reports declarations, immutable versions, source selection, and link state without changing files. Text output groups config, environment, tools, and links; --json emits the same fields plus diagnostics for machines. Link states are active, missing, dangling, conflict, and unowned (the desired symlink exists but this root did not record it). Missing artifacts, drift, and unhealthy links can appear as diagnostics with exit status 0; success is not proof of a healthy state. Malformed config or ownership state returns non-zero; a declaration whose source or version cannot be resolved is reported as a diagnostic with exit status 0. Example: skills status --global --json"
    )]
    Status(StatusArgs),
    /// Remove a declaration or selected entries from its include policy.
    #[command(
        after_help = "Normal: removes the named stored include selectors (exact strings, not skill matches); omit selectors to remove the whole declaration. Removing the last include removes the declaration rather than selecting all. Config is saved before sync and artifact removal, so a later failure can leave the declaration changed. Artifacts are retained while another declaration in the selected config or the current project/global scopes references the same source+version. Failure: an unknown tool, or a request whose stored selectors all fail to match, returns non-zero; changed unmanaged links are reported and unrelated targets are untouched. Scope: project by default, or --global/--path/--env. Example: skills remove skills:fullmeta-dev/skills mise"
    )]
    Remove(RemoveArgs),
    /// Remove stale links recorded for the selected configuration root.
    #[command(
        after_help = "Normal: removes only stale links recorded for this config root; it never creates desired links and never changes declarations or installed artifacts. If desired links cannot be determined, links for still-declared tools are retained conservatively. Changed or unmanaged targets are retained with warnings. Failure: config/ownership or filesystem errors return non-zero. Scope: project by default, or --global/--path/--env. Example: skills prune --global"
    )]
    Prune(PruneArgs),
    /// Inspect or clean the immutable source cache used by list.
    #[command(
        after_help = "Normal: read-only unless --clean is given. Reports the cache directory, the number of cached resolved refs and checkouts, and their size. --clean removes the whole cache directory (pins, checkouts, and scratch space); it never touches mise installations, config, or activated links. The directory is $SKILLS_CACHE_DIR, else $MISE_CACHE_DIR/skills, else $XDG_CACHE_HOME/skills, else ~/.cache/skills. --json emits the same fields as JSON. Failure: an unreadable or unwritable cache directory returns non-zero. Example: skills cache --clean"
    )]
    Cache(CacheArgs),
    /// Report strict Agent Skills conformance without blocking normal use.
    #[command(
        visible_alias = "check",
        after_help = "Normal: checks local sources for strict Agent Skills conformance without blocking normal discovery. SOURCE is required and must be a local directory (an optional path: prefix is accepted); GitHub shorthand and Git URLs are not fetched. Relative paths use the working directory. The intentional top-level tags extension is reported but does not fail ordinary discovery. Exit 0 means ordinary discovery succeeded even when strict-only diagnostics exist. --json emits source and diagnostics. Failure: unreadable sources or ordinary discovery errors return non-zero. Alias: check. Example: skills validate path:../skills-source --json"
    )]
    Validate(ValidateArgs),
    /// Generate a completion script for one supported shell.
    #[command(
        visible_alias = "completion",
        after_help = "Normal: writes a completion script for SHELL (bash, zsh, fish, or powershell) to stdout; shell redirection creates or replaces the chosen file, so create its parent directory first. It never edits config or contacts a source. Failure: unsupported shells are rejected before output. Alias: completion. Example: mkdir -p ~/.local/share/bash-completion/completions && skills completions bash > ~/.local/share/bash-completion/completions/skills"
    )]
    Completions(CompletionsArgs),
}

#[derive(Debug, Args)]
struct ScopeArgs {
    /// Select mise's environment-specific config (`mise.<env>.toml`).
    #[arg(long, env = "MISE_ENV", value_name = "ENV")]
    env: Option<String>,
    /// Select the user-owned global mise config.
    #[arg(long, conflicts_with = "path")]
    global: bool,
    /// Select this mise config file (or mise.toml inside a directory).
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

    /// The direct postinstall hook for this scope. A global or explicit config
    /// hook runs with mise's project directory as cwd, so it must name its
    /// target instead of relying on the nearest project config.
    fn postinstall_command(&self) -> String {
        let mut command = String::from(config::POSTINSTALL_COMMAND);
        if let Some(environment) = &self.env {
            command.push_str(" --env ");
            command.push_str(environment);
        }
        if self.global {
            command.push_str(" --global");
        } else if let Some(path) = &self.path {
            let absolute = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(path))
                    .unwrap_or_else(|_| path.clone())
            };
            command.push_str(" --path ");
            command.push_str(&shell_quote(&absolute.display().to_string()));
        }
        command
    }
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '/' | '.' | '_' | '-' | ':' | '~' | '+' | '=')
        })
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Local path, GitHub shorthand, or Git URL to inspect.
    source: String,
    /// Branch or tag to inspect without changing any config.
    #[arg(long, conflicts_with = "reference")]
    track: Option<String>,
    /// Explicit Git ref or full commit to inspect without changing any config.
    #[arg(long, conflicts_with = "track")]
    reference: Option<String>,
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
    /// Re-resolve Git refs instead of using a cached resolution.
    #[arg(long)]
    refresh: bool,
    /// Additional selectors, equivalent to repeated `--include`.
    selectors: Vec<String>,
}

#[derive(Debug, Args)]
struct UseArgs {
    #[command(flatten)]
    scope: ScopeArgs,
    /// Source (`owner/repository`, `github:...`, `git:...`, or `path:...`).
    source: String,
    /// Optional include selectors; omit to keep an existing selection policy.
    selectors: Vec<String>,
    /// Agent adapters to record in the declaration.
    #[arg(long = "agent", required = true, value_name = "AGENT")]
    agents: Vec<String>,
    /// Source-relative skill directory, validated against traversal.
    #[arg(long, alias = "source-subpath")]
    subpath: Option<String>,
    /// Branch or tag to resolve now and record as track. Defaults to the remote's default branch.
    #[arg(long, conflicts_with = "reference")]
    track: Option<String>,
    /// Git ref or full commit to resolve now and record as track.
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
    /// Preview normal mise installation without changing files; skips the direct sync.
    #[arg(long)]
    dry_run: bool,
    /// Replace unmanaged or changed targets at desired link paths.
    #[arg(long)]
    force: bool,
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
    /// Accepted for the generated postinstall hook; sync never resolves versions.
    #[arg(long)]
    locked: bool,
    /// Replace unmanaged or changed targets at desired link paths.
    #[arg(long)]
    force: bool,
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
struct CacheArgs {
    /// Remove every cached entry.
    #[arg(long)]
    clean: bool,
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
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
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    track: Option<String>,
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

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default())
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD));

/// Apply the shared palette to the whole command tree and emphasize section
/// headings in every `after_help` block. clap strips the styling when output is
/// not a terminal, so captured and piped help stays plain.
fn typeset_help(command: clap::Command) -> clap::Command {
    let command = match style::color_override() {
        Some(true) => command.color(clap::ColorChoice::Always),
        Some(false) => command.color(clap::ColorChoice::Never),
        None => command,
    };
    let command = command.styles(STYLES).mut_subcommands(typeset_help);
    let styled = command
        .get_after_help()
        .map(|help| help.to_string())
        .filter(|help| !help.is_empty())
        .map(|help| typeset(&help));
    match styled {
        Some(styled) => command.after_help(styled),
        None => command,
    }
}

fn typeset(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if is_help_heading(line) {
            out.push_str("\x1b[1;36m");
            out.push_str(line);
            out.push_str("\x1b[0m");
        } else {
            out.push_str(line);
        }
    }
    out
}

fn is_help_heading(line: &str) -> bool {
    line.len() >= 3
        && line.len() <= 48
        && !line.starts_with(' ')
        && line.ends_with(':')
        && line.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && !line.contains("://")
        && !line.chars().any(char::is_control)
}

fn main() -> ExitCode {
    let command = typeset_help(Cli::command());
    let matches = command.get_matches();
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    let Some(command) = cli.command else {
        // Clap's generated help is intentionally the only output for a bare
        // invocation and is always side-effect free.
        let mut command = typeset_help(Cli::command());
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
        Command::Cache(args) => run_cache(args),
        Command::Validate(args) => run_validate(args),
        Command::Completions(args) => run_completions(args),
    }
}

fn run_list(args: ListArgs) -> ExitCode {
    let config_root = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => return fail(format!("cannot determine current directory: {error}")),
    };
    let cache = Cache::from_env(args.refresh);
    let prepared = match prepare_discovery_source(
        &args.source,
        &config_root,
        args.reference.as_deref().or(args.track.as_deref()),
        &cache,
    ) {
        Ok(source) => source,
        Err(error) => return fail(error),
    };
    let discovery = match discover_with_subpath(&prepared.root, args.subpath.as_deref()) {
        Ok(discovery) => discovery,
        Err(error) => {
            eprintln!("{}: {error}", style::error_label());
            return ExitCode::from(1);
        }
    };
    let mut includes = args.includes;
    includes.extend(args.selectors);
    let selection = discovery.select(&includes, &args.excludes);
    let explain = discovery.explain(&selection);
    let output = ListOutput {
        source: prepared.spec.canonical.clone(),
        version: prepared.version,
        revision: prepared.revision,
        track: prepared.track,
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
            Ok(json) => {
                if let Err(error) = write_stdout(&(json + "\n")) {
                    return if error.kind() == io::ErrorKind::BrokenPipe {
                        ExitCode::SUCCESS
                    } else {
                        eprintln!(
                            "{}: cannot write list output: {error}",
                            style::error_label()
                        );
                        ExitCode::from(1)
                    };
                }
            }
            Err(error) => {
                eprintln!(
                    "{}: cannot encode list as JSON: {error}",
                    style::error_label()
                );
                return ExitCode::from(1);
            }
        }
    } else if let Err(error) = print_text(&output, &selection, args.tree, args.explain) {
        return if error.kind() == io::ErrorKind::BrokenPipe {
            ExitCode::SUCCESS
        } else {
            eprintln!(
                "{}: cannot write list output: {error}",
                style::error_label()
            );
            ExitCode::from(1)
        };
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
            eprintln!("{}: {error}", style::error_label());
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
                eprintln!(
                    "{}: cannot encode validation diagnostics as JSON: {error}",
                    style::error_label()
                );
                return ExitCode::from(1);
            }
        }
    } else if output.diagnostics.is_empty() {
        println!(
            "{} {}",
            style::ok("strict Agent Skills validation passed for"),
            style::path(&output.source)
        );
    } else {
        println!(
            "{} {}:",
            style::warn_stdout("strict Agent Skills diagnostics for"),
            style::path(&output.source)
        );
        for diagnostic in &output.diagnostics {
            let level = match diagnostic.level {
                DiagnosticLevel::Error => "error",
                DiagnosticLevel::Warning => "warning",
            };
            let level = style::level(level);
            if let Some(path) = &diagnostic.path {
                println!("{level}: {}: {}", style::path(path), diagnostic.message);
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
            println!(
                "{} {} at {} in {}",
                style::ok("configured"),
                style::bold(&key),
                style::label(&version),
                style::path(&path.display().to_string())
            );
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
        &args.scope.postinstall_command(),
    )?;
    config.save_if_changed(&before)?;
    Ok((format!("skills:{}", source.key), version, config.path))
}

fn run_upgrade(args: UpgradeArgs) -> ExitCode {
    match upgrade_command(args) {
        Ok((count, path)) => {
            println!(
                "{} {count} skill source(s) in {}",
                style::ok("upgraded"),
                style::path(&path.display().to_string())
            );
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
            eprintln!("{}: {collision}", style::warning_label());
        }
        let output = run_normal_mise_install(&config, args.locked, args.dry_run)?;
        if !output.trim().is_empty() {
            print!("{output}");
        }
        if args.dry_run {
            return Ok(());
        }
        let report = lifecycle::sync(&config, args.scope.is_global(), args.force)?;
        println!(
            "{} {} link(s), removed {} link(s) in {}",
            style::ok("installed selected mise tools and synced"),
            report.linked,
            report.removed,
            style::path(&config.path.display().to_string())
        );
        print_report_details(&report);
        Ok(())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}

fn run_doctor(args: DoctorArgs) -> ExitCode {
    let scope = args.scope.scope();
    match Config::load(&scope).map(|config| {
        println!(
            "{} {}",
            style::label("config"),
            style::path(&config.path.display().to_string())
        );
        println!(
            "{} {}",
            style::label("environment"),
            std::env::var("MISE_ENV").unwrap_or_else(|_| "default".into())
        );
        let collisions = collision_diagnostics(&config);
        if collisions.is_empty() {
            println!(
                "{} {}",
                style::label("skills executable"),
                style::ok("no unrelated owner detected")
            );
        } else {
            for collision in collisions {
                println!("{}", style::warn_stdout(&collision));
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
    /// The API asset URL. Private repositories serve the asset only from here,
    /// with an `application/octet-stream` accept header.
    #[serde(default)]
    url: String,
}

fn run_self_update(args: SelfUpdateArgs) -> ExitCode {
    match self_update_command(args) {
        Ok((version, path, dry_run)) => {
            if dry_run {
                println!(
                    "{} skills CLI and backend to {} in {}",
                    style::warn_stdout("would update"),
                    style::label(&version),
                    style::path(&path.display().to_string())
                );
            } else {
                println!(
                    "{} skills CLI and backend to {} in {}",
                    style::ok("updated"),
                    style::label(&version),
                    style::path(&path.display().to_string())
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
    // A private repository serves the asset only from the API URL with an
    // octet-stream accept header; a public one works from either.
    let location = if github_token().is_some() && !asset.url.is_empty() {
        asset.url.as_str()
    } else {
        asset.browser_download_url.as_str()
    };
    parse_compatibility_manifest(&read_manifest_location(location)?)
}

fn github_token() -> Option<String> {
    ["MISE_GITHUB_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
}

/// Download a GitHub URL with curl, sending a token when one is configured so
/// private repositories work. curl does not forward the header across the
/// redirect to the asset host.
fn curl_github(location: &str, action: &str, headers: &[&str]) -> Result<Vec<u8>, String> {
    let mut command = ProcessCommand::new("curl");
    command.args(["--fail", "--location", "--silent", "--show-error"]);
    for header in headers {
        command.args(["--header", header]);
    }
    if location.contains("github.com") || location.contains("githubusercontent.com") {
        if let Some(token) = github_token() {
            command
                .arg("--header")
                .arg(format!("Authorization: Bearer {token}"));
        }
    }
    command.arg(location);
    let output = command
        .output()
        .map_err(|error| format!("cannot run curl for {action}: {error}"))?;
    if !output.status.success() {
        return Err(process_failure(action, &output));
    }
    Ok(output.stdout)
}

fn read_release_api(location: &str) -> Result<Vec<u8>, String> {
    if location.contains("://") {
        curl_github(
            location,
            "latest GitHub release lookup",
            &[
                "Accept: application/vnd.github+json",
                "X-GitHub-Api-Version: 2022-11-28",
            ],
        )
    } else {
        fs::read(location)
            .map_err(|error| format!("cannot read release API fixture {location}: {error}"))
    }
}

fn read_manifest_location(location: &str) -> Result<Vec<u8>, String> {
    if location.contains("://") {
        let headers: &[&str] = if location.contains("/releases/assets/") {
            &["Accept: application/octet-stream"]
        } else {
            &[]
        };
        curl_github(location, "compatibility manifest download", headers)
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
    // A private github: release cannot be resolved through the versions host;
    // ask the GitHub API directly, which uses GITHUB_TOKEN when set.
    if tools.iter().any(|tool| tool.starts_with("github:")) {
        command.env("MISE_USE_VERSIONS_HOST", "false");
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
        let report = lifecycle::sync(&config, args.scope.is_global(), args.force)?;
        println!(
            "{} {} link(s), removed {} link(s) in {}",
            style::ok("synced"),
            report.linked,
            report.removed,
            style::path(&config.path.display().to_string())
        );
        print_report_details(&report);
        Ok(())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}

/// Print the replaced/conflict/warning detail shared by sync, install, and
/// prune. Replaced entries go to stdout with the rest of the report; conflicts
/// and warnings are diagnostics on stderr.
fn print_report_details(report: &lifecycle::SyncReport) {
    for replaced in &report.replaced {
        println!("{} {replaced}", style::warn_stdout("replaced:"));
    }
    for conflict in &report.conflicts {
        eprintln!("{}: {conflict}", style::warning_label());
    }
    for warning in &report.warnings {
        eprintln!("{}: {warning}", style::warning_label());
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
            println!("{} {}", style::label("config"), style::path(&output.config));
            println!("{} {}", style::label("environment"), output.environment);
            if output.tools.is_empty() {
                println!("{} none declared", style::label("tools"));
            } else {
                println!(
                    "{}",
                    style::heading(&format!("Tools ({})", output.tools.len()))
                );
                for tool in &output.tools {
                    println!(
                        "  {}  {}",
                        style::bold(&tool.key),
                        style::path(&tool.version)
                    );
                }
            }
            if output.links.is_empty() {
                println!("{} none", style::label("links"));
            } else {
                println!(
                    "{}",
                    style::heading(&format!("Links ({})", output.links.len()))
                );
                for link in &output.links {
                    println!(
                        "  {}  {}  {}  {}",
                        style::label(&link.adapter),
                        style::bold(&link.skill),
                        link_state(&link.state),
                        style::path(&link.target)
                    );
                }
            }
            for diagnostic in &output.diagnostics {
                eprintln!("{}: {diagnostic}", style::warning_label());
            }
        }
        Ok(())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}

fn link_state(state: &str) -> String {
    match state {
        "active" => style::ok(state),
        "conflict" | "unowned" => style::warn_stdout(state),
        _ => style::path(state),
    }
}

fn run_remove(args: RemoveArgs) -> ExitCode {
    match remove_command(args) {
        Ok((removed, path)) => {
            if removed {
                println!(
                    "{} skill declaration(s) in {}",
                    style::ok("removed"),
                    style::path(&path.display().to_string())
                );
            } else {
                println!(
                    "no skill declaration changed in {}",
                    style::path(&path.display().to_string())
                );
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
    let report = lifecycle::sync(&config, args.scope.is_global(), false)?;
    print_report_details(&report);

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
            "{} {} stale link(s) in {}",
            style::ok("pruned"),
            report.removed,
            style::path(&config.path.display().to_string())
        );
        print_report_details(&report);
        Ok(())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(error),
    }
}

fn run_cache(args: CacheArgs) -> ExitCode {
    let cache = Cache::from_env(false);
    if args.clean {
        let before = match cache.clean() {
            Ok(stats) => stats,
            Err(error) => return fail(error),
        };
        if args.json {
            match serde_json::to_string_pretty(&before) {
                Ok(json) => println!("{json}"),
                Err(error) => return fail(format!("cannot encode cache stats: {error}")),
            }
        } else {
            println!(
                "{} {} entries ({}) from {}",
                style::ok("cleaned"),
                before.pins + before.checkouts,
                human_bytes(before.bytes),
                style::path(before.dir.as_deref().unwrap_or("unknown"))
            );
        }
        return ExitCode::SUCCESS;
    }
    if let Err(error) = cache.validate() {
        return fail(error);
    }
    let stats = cache.stats();
    if args.json {
        match serde_json::to_string_pretty(&stats) {
            Ok(json) => println!("{json}"),
            Err(error) => return fail(format!("cannot encode cache stats: {error}")),
        }
    } else {
        match &stats.dir {
            Some(dir) => println!("{} {}", style::label("cache dir"), style::path(dir)),
            None => println!("{} disabled", style::label("cache dir")),
        }
        println!(
            "{} {} refs, {} checkouts",
            style::label("entries"),
            stats.pins,
            stats.checkouts
        );
        println!("{} {}", style::label("size"), human_bytes(stats.bytes));
    }
    ExitCode::SUCCESS
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
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
    eprintln!("{}: {error}", style::error_label());
    ExitCode::from(1)
}

fn strip_path_prefix(path: &Path) -> PathBuf {
    path.to_str()
        .and_then(|value| value.strip_prefix("path:"))
        .map(PathBuf::from)
        .unwrap_or_else(|| path.to_path_buf())
}

fn write_stdout(value: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(value.as_bytes())?;
    stdout.flush()
}

/// Render untrusted source metadata without allowing terminal control sequences.
fn terminal_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| character.escape_default())
        .collect()
}

fn print_text(
    output: &ListOutput,
    selection: &Selection,
    tree: bool,
    explain: bool,
) -> io::Result<()> {
    let mut text = String::new();
    use std::fmt::Write as _;
    writeln!(
        text,
        "{} {}",
        style::label("source"),
        style::bold(&terminal_text(&output.source))
    )
    .expect("writing String cannot fail");
    writeln!(
        text,
        "{} {}",
        style::label("version"),
        style::path(&terminal_text(&output.version))
    )
    .expect("writing String cannot fail");
    if let Some(revision) = &output.revision {
        writeln!(
            text,
            "{} {}",
            style::label("revision"),
            style::path(&terminal_text(revision))
        )
        .expect("writing String cannot fail");
    }
    if let Some(track) = &output.track {
        writeln!(text, "{} {}", style::label("track"), terminal_text(track))
            .expect("writing String cannot fail");
    }
    writeln!(text).expect("writing String cannot fail");
    writeln!(
        text,
        "{}",
        style::heading(&format!("Skills ({})", output.skills.len()))
    )
    .expect("writing String cannot fail");
    if output.skills.is_empty() {
        writeln!(text, "  {}", style::path("none selected")).expect("writing String cannot fail");
    }
    for skill in &output.skills {
        let tags = if skill.tags.is_empty() {
            "none".to_string()
        } else {
            skill.tags.join(", ")
        };
        let aliases = if skill.aliases.is_empty() {
            "none".to_string()
        } else {
            skill.aliases.join(", ")
        };
        writeln!(
            text,
            "  {}  {}",
            style::bold(&terminal_text(&skill.name)),
            style::path(&terminal_text(&skill.path))
        )
        .expect("writing String cannot fail");
        writeln!(text, "    {}", terminal_text(&skill.description))
            .expect("writing String cannot fail");
        writeln!(
            text,
            "    {} {}",
            style::label("tags:"),
            terminal_text(&tags)
        )
        .expect("writing String cannot fail");
        writeln!(
            text,
            "    {} {}",
            style::label("aliases:"),
            terminal_text(&aliases)
        )
        .expect("writing String cannot fail");
        if tree {
            for file in &skill.files {
                writeln!(text, "    {}", style::path(&terminal_text(file)))
                    .expect("writing String cannot fail");
            }
        }
    }
    writeln!(
        text,
        "\n{} {} selected, {} excluded, {} shadowed, {} diagnostics",
        style::label("summary"),
        output.skills.len(),
        output.selection.excluded.len(),
        output.shadowed.len(),
        output.diagnostics.len()
    )
    .expect("writing String cannot fail");
    if explain {
        writeln!(text, "\n{}", style::heading("Explain")).expect("writing String cannot fail");
        if output.shadowed.is_empty() {
            writeln!(text, "  {} none", style::label("shadowed"))
                .expect("writing String cannot fail");
        } else {
            writeln!(text, "  {}", style::label("shadowed")).expect("writing String cannot fail");
            for shadow in &output.shadowed {
                writeln!(
                    text,
                    "    {}: {} wins over {}",
                    terminal_text(&shadow.name),
                    terminal_text(&shadow.winner),
                    terminal_text(&shadow.path)
                )
                .expect("writing String cannot fail");
            }
        }
        for decision in &selection.decisions {
            writeln!(
                text,
                "  {}: {}",
                terminal_text(&decision.path),
                terminal_text(&decision.reason)
            )
            .expect("writing String cannot fail");
        }
        for selector in &selection.missing_includes {
            writeln!(
                text,
                "  missing include selector: {}",
                terminal_text(selector)
            )
            .expect("writing String cannot fail");
        }
        for resolution in &selection.selector_resolutions {
            writeln!(
                text,
                "  {}: {} wins over {}",
                terminal_text(&resolution.selector),
                terminal_text(&resolution.winner),
                terminal_text(&resolution.shadowed.join(", "))
            )
            .expect("writing String cannot fail");
        }
        for selector in &selection.missing_excludes {
            writeln!(
                text,
                "  missing exclude selector: {}",
                terminal_text(selector)
            )
            .expect("writing String cannot fail");
        }
    }
    for selector in &selection.missing_includes {
        if !explain {
            eprintln!(
                "{}: missing include selector: {}",
                style::warning_label(),
                terminal_text(selector)
            );
        }
    }
    for selector in &selection.missing_excludes {
        if !explain {
            eprintln!(
                "{}: missing exclude selector: {}",
                style::warning_label(),
                terminal_text(selector)
            );
        }
    }
    for diagnostic in &output.diagnostics {
        let level = match diagnostic.level {
            DiagnosticLevel::Error => "error",
            DiagnosticLevel::Warning => "warning",
        };
        let level = style::level_stderr(level);
        if let Some(path) = &diagnostic.path {
            eprintln!(
                "{level}: {}: {}",
                terminal_text(path),
                terminal_text(&diagnostic.message)
            );
        } else {
            eprintln!("{level}: {}", terminal_text(&diagnostic.message));
        }
    }
    write_stdout(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn terminal_text_escapes_untrusted_controls() {
        assert_eq!(
            terminal_text("safe\u{1b}]52;c;payload\u{7}"),
            "safe\\u{1b}]52;c;payload\\u{7}"
        );
        assert_eq!(terminal_text("line\nbreak"), "line\\nbreak");
    }

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
