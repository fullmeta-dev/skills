use crate::source::{SourceKind, SourceSpec};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{value, Array, DocumentMut, Item, Table, Value};

pub const POSTINSTALL_COMMAND: &str = "mise run skills -- sync --locked";

#[derive(Debug, Clone)]
pub enum Scope {
    Project,
    Global,
    Path(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub path: PathBuf,
    pub root: PathBuf,
    pub document: DocumentMut,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ToolInfo {
    pub key: String,
    pub source: String,
    pub version: String,
    pub track: Option<String>,
}

impl Scope {
    pub fn config_path(&self) -> Result<PathBuf, String> {
        match self {
            Self::Project => {
                let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
                let mut candidate = cwd.as_path();
                loop {
                    let config = candidate.join("mise.toml");
                    if config.is_file() {
                        return Ok(config);
                    }
                    let Some(parent) = candidate.parent() else {
                        break;
                    };
                    if parent == candidate {
                        break;
                    }
                    candidate = parent;
                }
                Ok(cwd.join("mise.toml"))
            }
            Self::Global => {
                if let Ok(path) = std::env::var("MISE_CONFIG_FILE") {
                    if !path.is_empty() {
                        return Ok(PathBuf::from(path));
                    }
                }
                let dir = std::env::var_os("MISE_CONFIG_DIR")
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("XDG_CONFIG_HOME")
                            .map(|path| PathBuf::from(path).join("mise"))
                    })
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .map(|path| PathBuf::from(path).join(".config").join("mise"))
                    })
                    .ok_or_else(|| {
                        "cannot determine the global mise config directory".to_string()
                    })?;
                Ok(dir.join("config.toml"))
            }
            Self::Path(path) => {
                let path = if path.is_absolute() {
                    path.clone()
                } else {
                    std::env::current_dir()
                        .map_err(|error| error.to_string())?
                        .join(path)
                };
                if path.is_dir() {
                    Ok(path.join("mise.toml"))
                } else {
                    Ok(path)
                }
            }
        }
    }
}

impl Config {
    pub fn load(scope: &Scope) -> Result<Self, String> {
        Self::load_path(&scope.config_path()?)
    }

    pub fn load_path(path: &Path) -> Result<Self, String> {
        let document = match fs::read_to_string(path) {
            Ok(contents) => contents
                .parse::<DocumentMut>()
                .map_err(|error| format!("cannot parse {}: {error}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        let root = path
            .parent()
            .ok_or_else(|| format!("config path has no parent: {}", path.display()))?;
        let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        Ok(Self {
            path: path.to_path_buf(),
            root,
            document,
        })
    }

    pub fn save_if_changed(&self, before: &str) -> Result<bool, String> {
        let after = self.document.to_string();
        if after == before {
            return Ok(false);
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "cannot create config directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        fs::write(&self.path, after)
            .map_err(|error| format!("cannot write {}: {error}", self.path.display()))?;
        Ok(true)
    }
}

pub fn skill_tools(document: &DocumentMut) -> Result<Vec<ToolInfo>, String> {
    let Some(tools) = document.get("tools").and_then(Item::as_table) else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for (key, item) in tools.iter() {
        if !key.starts_with("skills:") {
            continue;
        }
        let table = item.as_table().ok_or_else(|| {
            format!("tool declaration {key:?} must be a table with source and version")
        })?;
        let source = string_field(table, "source", key)?;
        let version = string_field(table, "version", key)?;
        let track = optional_string_field(table, "track", key)?;
        result.push(ToolInfo {
            key: key.to_string(),
            source,
            version,
            track,
        });
    }
    Ok(result)
}

pub fn tool_info(document: &DocumentMut, key: &str) -> Result<Option<ToolInfo>, String> {
    Ok(skill_tools(document)?
        .into_iter()
        .find(|tool| tool.key == key))
}

pub fn tool_subpath(document: &DocumentMut, key: &str) -> Result<String, String> {
    let tools = document
        .get("tools")
        .and_then(Item::as_table)
        .ok_or_else(|| "config has no [tools] table".to_string())?;
    let table = tools
        .get(key)
        .and_then(Item::as_table)
        .ok_or_else(|| format!("skill tool {key:?} is not a table"))?;
    match table.get("source_subpath").or_else(|| table.get("subpath")) {
        None => Ok(String::new()),
        Some(item) => item
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| format!("skill tool {key:?} source subpath must be a string")),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn apply_use(
    document: &mut DocumentMut,
    root: &Path,
    source: &SourceSpec,
    subpath: &str,
    version: &str,
    track: Option<&str>,
    selectors: &[String],
    agents: &[String],
) -> Result<(), String> {
    let key = format!("skills:{}", source.key);
    {
        let tools = ensure_table(document, "tools")?;
        let table = match tools.get_mut(&key) {
            Some(item) => item.as_table_mut().ok_or_else(|| {
                format!("existing tool declaration {key:?} is not a table; refusing to replace it")
            })?,
            None => {
                tools.insert(&key, Item::Table(Table::new()));
                tools
                    .get_mut(&key)
                    .and_then(Item::as_table_mut)
                    .expect("new table inserted")
            }
        };

        if let Some(existing_source) = table.get("source").and_then(Item::as_str) {
            let existing = SourceSpec::parse(existing_source, root)
                .map_err(|error| format!("cannot understand existing source in {key}: {error}"))?;
            if !same_source(&existing, source) {
                return Err(format!(
                    "tool key {key:?} already refers to {}, not {}; refusing to repoint it",
                    existing.canonical, source.canonical
                ));
            }
        } else if table.contains_key("source") {
            return Err(format!("source in {key:?} must be a string"));
        }

        replace_string(table, "source", &source.canonical);
        replace_string(table, "version", version);
        table.remove("source_subpath");
        table.remove("subpath");
        table.remove("source_path");
        if !subpath.is_empty() {
            replace_string(table, "source_subpath", subpath);
        }
        if let Some(track) = track {
            replace_string(table, "track", track);
        } else if source.kind == SourceKind::Path {
            table.remove("track");
        }
        if !selectors.is_empty() {
            table["include"] = string_array(selectors);
        }
        if !agents.is_empty() {
            table["agents"] = string_array(agents);
        }
    }
    ensure_postinstall(document)
}

pub fn update_version(document: &mut DocumentMut, key: &str, version: &str) -> Result<(), String> {
    let tools = document
        .get_mut("tools")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| "config has no [tools] table".to_string())?;
    let table = tools
        .get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| format!("skill tool {key:?} is not a table"))?;
    if !table.contains_key("version") {
        return Err(format!("skill tool {key:?} has no version"));
    }
    replace_string(table, "version", version);
    Ok(())
}

pub fn ensure_postinstall(document: &mut DocumentMut) -> Result<(), String> {
    let hooks = ensure_table(document, "hooks")?;
    match hooks.get_mut("postinstall") {
        None => {
            hooks.insert(
                "postinstall",
                Item::Value(Value::Array(string_array_values(&[
                    POSTINSTALL_COMMAND.to_string()
                ]))),
            );
        }
        Some(item) if item.as_str().is_some() => {
            let old = item.as_str().unwrap_or_default().to_string();
            if old.contains(POSTINSTALL_COMMAND) {
                return Ok(());
            }
            let mut array = Array::new();
            array.push(old);
            array.push(POSTINSTALL_COMMAND);
            let decor = item.as_value().map(|value| value.decor().clone());
            let mut replacement = Value::Array(array);
            if let Some(decor) = decor {
                *replacement.decor_mut() = decor;
            }
            *item = Item::Value(replacement);
        }
        Some(item) if item.as_array().is_some() => {
            let array = item.as_array_mut().expect("array checked");
            if !array_contains_command(array) {
                array.push(POSTINSTALL_COMMAND);
            }
        }
        Some(item) if item.as_table().is_some() => {
            let table = item.as_table_mut().expect("table checked");
            match table.get_mut("run") {
                Some(run) if run.as_str().is_some() => {
                    let old = run.as_str().unwrap_or_default().to_string();
                    if !old.contains(POSTINSTALL_COMMAND) {
                        *run = value(format!("{old}\n{POSTINSTALL_COMMAND}"));
                    }
                }
                Some(_) => return Err("hooks.postinstall.run must be a string".into()),
                None => {
                    table["run"] = value(POSTINSTALL_COMMAND);
                }
            }
        }
        Some(item) if item.as_inline_table().is_some() => {
            let table = item.as_inline_table_mut().expect("inline table checked");
            match table.get_mut("run") {
                Some(run) if run.as_str().is_some() => {
                    let old = run.as_str().unwrap_or_default().to_string();
                    if !old.contains(POSTINSTALL_COMMAND) {
                        *run = Value::from(format!("{old}\n{POSTINSTALL_COMMAND}"));
                    }
                }
                Some(_) => return Err("hooks.postinstall.run must be a string".into()),
                None => {
                    table.insert("run", Value::from(POSTINSTALL_COMMAND));
                }
            }
        }
        Some(_) => return Err("hooks.postinstall must be a string, table, or array".into()),
    }
    Ok(())
}

fn ensure_table<'a>(document: &'a mut DocumentMut, key: &str) -> Result<&'a mut Table, String> {
    if !document.contains_key(key) {
        document.insert(key, Item::Table(Table::new()));
    }
    document
        .get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| format!("{key} must be a TOML table"))
}

fn replace_string(table: &mut Table, field: &str, replacement: &str) {
    let decor = table
        .get(field)
        .and_then(Item::as_value)
        .map(|value| value.decor().clone());
    let mut item = value(replacement.to_string());
    if let Some(decor) = decor {
        if let Some(value) = item.as_value_mut() {
            *value.decor_mut() = decor;
        }
    }
    table[field] = item;
}

fn string_field(table: &Table, field: &str, key: &str) -> Result<String, String> {
    table
        .get(field)
        .and_then(Item::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| format!("skill tool {key:?} requires string {field:?}"))
}

fn optional_string_field(table: &Table, field: &str, key: &str) -> Result<Option<String>, String> {
    match table.get(field) {
        None => Ok(None),
        Some(item) => item
            .as_str()
            .map(|value| Some(value.to_string()))
            .ok_or_else(|| format!("skill tool {key:?} field {field:?} must be a string")),
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

fn string_array(values: &[String]) -> Item {
    Item::Value(Value::Array(string_array_values(values)))
}

fn string_array_values(values: &[String]) -> Array {
    let mut array = Array::new();
    for item in values {
        array.push(item.as_str());
    }
    array
}

fn array_contains_command(array: &Array) -> bool {
    array.iter().any(|item| {
        item.as_str()
            .is_some_and(|value| value.contains(POSTINSTALL_COMMAND))
            || item
                .as_inline_table()
                .and_then(|table| table.get("run"))
                .and_then(Value::as_str)
                .is_some_and(|value| value.contains(POSTINSTALL_COMMAND))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(input: &str) -> DocumentMut {
        input.parse().unwrap()
    }

    #[test]
    fn appends_postinstall_to_string_array_and_table_forms_once() {
        for input in [
            "[hooks]\npostinstall = \"echo old\"\n",
            "[hooks]\npostinstall = [\"echo old\"]\n",
            "[hooks.postinstall]\nrun = \"echo old\"\nenv = { KEEP = \"yes\" }\n",
            "[hooks]\npostinstall = { run = \"echo old\", env = { KEEP = \"yes\" } }\n",
        ] {
            let mut document = doc(input);
            ensure_postinstall(&mut document).unwrap();
            let once = document.to_string();
            ensure_postinstall(&mut document).unwrap();
            assert_eq!(once, document.to_string());
            assert!(document.to_string().contains(POSTINSTALL_COMMAND));
            assert!(document.to_string().contains("echo old"));
        }
    }

    #[test]
    fn use_preserves_unrelated_tables_and_rejects_source_repointing() {
        let root = tempfile::tempdir().unwrap();
        let source_dir = root.path().join("source");
        std::fs::create_dir(&source_dir).unwrap();
        let source =
            SourceSpec::parse(&format!("path:{}", source_dir.display()), root.path()).unwrap();
        let mut document = doc("# keep\n[other]\nvalue = \"yes\"\n");
        apply_use(
            &mut document,
            root.path(),
            &source,
            "",
            "tree-0000000000000000000000000000000000000000000000000000000000000000",
            None,
            &["hello".into()],
            &["pi".into()],
        )
        .unwrap();
        let rendered = document.to_string();
        assert!(rendered.contains("# keep"));
        assert!(rendered.contains("[other]"));
        assert!(rendered.contains("include = [\"hello\"]"));
        let other_root = tempfile::tempdir().unwrap();
        let other_dir = other_root.path().join("source");
        std::fs::create_dir(&other_dir).unwrap();
        let other =
            SourceSpec::parse(&format!("path:{}", other_dir.display()), other_root.path()).unwrap();
        assert!(apply_use(
            &mut document,
            root.path(),
            &other,
            "",
            "tree-0000000000000000000000000000000000000000000000000000000000000000",
            None,
            &[],
            &[],
        )
        .is_err());
    }
}
