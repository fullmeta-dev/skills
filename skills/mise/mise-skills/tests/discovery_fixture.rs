use skills::{discover, DiagnosticLevel};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures")
        .join(name)
}

#[test]
fn catalog_discovers_roots_and_excludes_without_inventory_records() {
    let result = discover(fixture("discovery")).expect("fixture source");
    let paths = result
        .skills
        .iter()
        .map(|skill| skill.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![
            "packages/demo/agent-skills/package-skill",
            "skills/nested/deep/review",
            "skills/ordinary",
            "skills/path/a/tool",
            "skills/path/b/tool",
            "skills/shadow/early",
        ]
    );
    assert!(!paths.iter().any(|path| path.contains("excluded")));
    assert!(!paths.iter().any(|path| path.contains("node_modules")));
    assert_eq!(result.shadowed.len(), 1);
    assert_eq!(result.shadowed[0].winner, "skills/shadow/early");
    assert_eq!(result.shadowed[0].path, "skills/shadow/late");
    assert!(result
        .skills
        .iter()
        .any(|skill| skill.files.contains(&"references/README.md".to_string())));
}

#[test]
fn catalog_tags_aliases_globs_and_tag_intersections_are_deterministic() {
    let result = discover(fixture("discovery")).expect("fixture source");
    let review = result.select(&["review".into()], &[]);
    assert_eq!(review.selected[0].path, "skills/nested/deep/review");

    let package = result.select(&["packages/**/package-skill".into()], &[]);
    assert_eq!(package.selected[0].name, "package-skill");

    let basename = result.select(&["tool".into()], &[]);
    assert_eq!(basename.selected[0].path, "skills/path/a/tool");
    assert_eq!(
        basename.selector_resolutions[0].shadowed,
        vec!["skills/path/b/tool"]
    );

    let union = result.select(&["tag:web".into(), "tag:package-only".into()], &[]);
    assert_eq!(union.selected.len(), 2);
    assert_eq!(
        union.selected[0].path,
        "packages/demo/agent-skills/package-skill"
    );

    let intersection = result.select(&["tag:repository+nested".into()], &[]);
    assert_eq!(intersection.selected[0].name, "code-review");
}

#[test]
fn exclusion_wins_and_missing_selectors_are_diagnostics_not_errors() {
    let result = discover(fixture("discovery")).expect("fixture source");
    let selection = result.select(
        &["tag:repository".into()],
        &["tag:stable".into(), "missing".into()],
    );
    assert!(!selection
        .selected
        .iter()
        .any(|skill| skill.name == "shadowed"));
    assert_eq!(selection.missing_excludes, vec!["missing"]);
    assert!(result
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.level != DiagnosticLevel::Error));
}

#[test]
fn approved_implementation_directories_are_skipped() {
    let temp = TempDir::new().expect("temporary source");
    for directory in [".git", "node_modules", "dist", "build", "__pycache__"] {
        let root = temp.path().join(directory).join("hidden");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            format!("---\nname: {directory}\ndescription: skipped\n---\n"),
        )
        .unwrap();
    }
    let result = discover(temp.path()).expect("temporary source");
    assert!(result.skills.is_empty());
}

#[test]
fn malformed_yaml_invalid_tags_and_escaping_links_are_reported() {
    let result = discover(fixture("discovery-invalid")).expect("fixture source");
    assert!(result.skills.is_empty());
    assert!(result.has_errors());
    let messages = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();
    assert!(messages
        .iter()
        .any(|message| message.contains("malformed YAML")));
    assert!(messages
        .iter()
        .any(|message| message.contains("invalid tag")));
    assert!(messages
        .iter()
        .any(|message| message.contains("escapes skill root")));
}

#[test]
fn invalid_catalog_tags_are_diagnostics_and_not_attached() {
    let result = discover(fixture("discovery-invalid-catalog")).expect("fixture source");
    assert!(result.has_errors());
    assert_eq!(result.skills.len(), 1);
    assert_eq!(result.skills[0].tags, vec!["valid"]);
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("invalid tag")));
}

#[test]
fn source_and_catalog_roots_can_be_skills() {
    let source_root = discover(fixture("discovery-source-root-skill")).expect("fixture source");
    assert_eq!(source_root.skills.len(), 1);
    assert_eq!(source_root.skills[0].path, "");

    let catalog_root = discover(fixture("discovery-catalog-root-skill")).expect("fixture source");
    assert_eq!(catalog_root.skills.len(), 1);
    assert_eq!(catalog_root.skills[0].path, "skill");
}
