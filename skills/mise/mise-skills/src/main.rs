use clap::{Args, CommandFactory, Parser, Subcommand};
use mise_skills::{discover_with_subpath, DiagnosticLevel, Explain, Selection, Skill};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "mise-skills",
    version,
    about = "Discover and select Agent Skills from an immutable source snapshot"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List discovered skills, optionally applying selectors.
    List(ListArgs),
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
