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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subpath: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
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
        let subpath = tool_subpath(document, key)?;
        let include = string_array_field(table, "include", key)?;
        let exclude = string_array_field(table, "exclude", key)?;
        let agents = string_array_field(table, "agents", key)?;
        result.push(ToolInfo {
            key: key.to_string(),
            source,
            version,
            track,
            subpath,
            include,
            exclude,
            agents,
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

pub fn remove_tool(
    document: &mut DocumentMut,
    key: &str,
    selectors: &[String],
) -> Result<bool, String> {
    let tools = document
        .get_mut("tools")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| "config has no [tools] table".to_string())?;
    let Some(item) = tools.get_mut(key) else {
        return Err(format!("skill tool {key:?} is not declared"));
    };
    if selectors.is_empty() {
        tools.remove(key);
        return Ok(true);
    }
    let table = item
        .as_table_mut()
        .ok_or_else(|| format!("skill tool {key:?} is not a table"))?;
    let includes = string_array_field(table, "include", key)?;
    if includes.is_empty() {
        return Err(format!(
            "skill tool {key:?} has no explicit include list; omit selectors to remove it"
        ));
    }
    let original_len = includes.len();
    let remaining: Vec<String> = includes
        .into_iter()
        .filter(|include| !selectors.iter().any(|selector| selector == include))
        .collect();
    if remaining.len() == original_len {
        return Err(format!(
            "none of the requested selectors are declared for {key}"
        ));
    }
    if remaining.is_empty() {
        tools.remove(key);
    } else {
        table["include"] = string_array(&remaining);
    }
    Ok(true)
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
            if contains_exact_postinstall_action(&old) {
                return Ok(());
            }
            let mut old_value = item.as_value().cloned().expect("string item has a value");
            let decor = old_value.decor().clone();
            old_value.decor_mut().clear();
            let mut array = Array::new();
            array.push_formatted(old_value);
            array.push(POSTINSTALL_COMMAND);
            let mut replacement = Value::Array(array);
            *replacement.decor_mut() = decor;
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
            let old = match table.get("run") {
                Some(run) if run.as_str().is_some() => {
                    Some(run.as_str().unwrap_or_default().to_string())
                }
                Some(_) => return Err("hooks.postinstall.run must be a string".into()),
                None => None,
            };
            match old {
                Some(old) if !contains_exact_postinstall_action(&old) => {
                    replace_string(table, "run", &format!("{old}\n{POSTINSTALL_COMMAND}"));
                }
                Some(_) => {}
                None => {
                    table["run"] = value(POSTINSTALL_COMMAND);
                }
            }
        }
        Some(item) if item.as_inline_table().is_some() => {
            let table = item.as_inline_table_mut().expect("inline table checked");
            let old = match table.get("run") {
                Some(run) if run.as_str().is_some() => {
                    Some(run.as_str().unwrap_or_default().to_string())
                }
                Some(_) => return Err("hooks.postinstall.run must be a string".into()),
                None => None,
            };
            match old {
                Some(old) if !contains_exact_postinstall_action(&old) => {
                    let run = table.get_mut("run").expect("run checked");
                    replace_value_preserving_decor(run, &format!("{old}\n{POSTINSTALL_COMMAND}"));
                }
                Some(_) => {}
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

fn replace_value_preserving_decor(item: &mut Value, replacement: &str) {
    let decor = item.decor().clone();
    let mut value = Value::from(replacement);
    *value.decor_mut() = decor;
    *item = value;
}

fn is_postinstall_command(value: &str) -> bool {
    value == POSTINSTALL_COMMAND
}

fn contains_exact_postinstall_action(value: &str) -> bool {
    value.split('\n').any(is_postinstall_command)
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

fn string_array_field(table: &Table, field: &str, key: &str) -> Result<Vec<String>, String> {
    let Some(item) = table.get(field) else {
        return Ok(Vec::new());
    };
    if let Some(value) = item.as_str() {
        return Ok(vec![value.to_string()]);
    }
    let Some(array) = item.as_array() else {
        return Err(format!(
            "skill tool {key:?} field {field:?} must be a string or array"
        ));
    };
    array
        .iter()
        .map(|value| {
            value.as_str().map(ToString::to_string).ok_or_else(|| {
                format!("skill tool {key:?} field {field:?} must contain only strings")
            })
        })
        .collect()
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
        item.as_str().is_some_and(contains_exact_postinstall_action)
            || item
                .as_inline_table()
                .and_then(|table| table.get("run"))
                .and_then(Value::as_str)
                .is_some_and(contains_exact_postinstall_action)
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
            "[hooks]\n# keep string comment\npostinstall = \"echo old\" # keep string suffix\n",
            "[hooks]\npostinstall = [\"echo old\" # keep array item\n] # keep array suffix\n",
            "[hooks.postinstall]\n# keep run comment\nrun = \"echo old\" # keep run suffix\nenv = { KEEP = \"yes\" }\n",
            "[hooks]\npostinstall = { run = \"echo old\", env = { KEEP = \"yes\" } } # keep table suffix\n",
        ] {
            let mut document = doc(input);
            ensure_postinstall(&mut document).unwrap();
            let once = document.to_string();
            ensure_postinstall(&mut document).unwrap();
            let rendered = document.to_string();
            assert_eq!(once, rendered);
            assert!(rendered.contains(POSTINSTALL_COMMAND));
            assert!(rendered.contains("echo old"));
            assert!(rendered.contains("keep"));
        }
    }

    #[test]
    fn postinstall_deduplicates_only_exact_actions() {
        for input in [
            "[hooks]\npostinstall = \"echo mise run skills -- sync --locked\"\n",
            "[hooks]\npostinstall = [\"echo mise run skills -- sync --locked\"]\n",
            "[hooks.postinstall]\nrun = \"echo mise run skills -- sync --locked\"\n",
            "[hooks]\npostinstall = { run = \"echo mise run skills -- sync --locked\" }\n",
        ] {
            let mut document = doc(input);
            ensure_postinstall(&mut document).unwrap();
            let once = document.to_string();
            ensure_postinstall(&mut document).unwrap();
            let rendered = document.to_string();
            assert_eq!(once, rendered);
            assert_eq!(rendered.matches(POSTINSTALL_COMMAND).count(), 2);
        }

        for input in [
            "[hooks]\npostinstall = \"mise run skills -- sync --locked\"\n",
            "[hooks]\npostinstall = [\"mise run skills -- sync --locked\"]\n",
            "[hooks.postinstall]\nrun = \"mise run skills -- sync --locked\"\n",
            "[hooks]\npostinstall = { run = \"mise run skills -- sync --locked\" }\n",
        ] {
            let mut document = doc(input);
            ensure_postinstall(&mut document).unwrap();
            assert_eq!(document.to_string().matches(POSTINSTALL_COMMAND).count(), 1);
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
