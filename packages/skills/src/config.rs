use crate::source::{SourceKind, SourceSpec};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use toml_edit::{value, Array, DocumentMut, Item, Table, Value};

pub const POSTINSTALL_COMMAND: &str = "skills sync --locked";

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
    /// Tolerate malformed skill files outside the selection. Source-wide
    /// errors still fail. Off by default so a declaration never silently
    /// activates fewer skills than intended.
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_source_errors: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Scope {
    pub fn config_path(&self) -> Result<PathBuf, String> {
        let environment = std::env::var("MISE_ENV").unwrap_or_default();
        validate_environment_name(&environment)?;
        let with_environment = |path: PathBuf| {
            if environment.is_empty() {
                return path;
            }
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                return path;
            };
            let suffix = format!(".{environment}");
            let (stem, extension) = match file_name.rsplit_once('.') {
                Some((stem, extension)) => (stem.to_string(), format!(".{extension}")),
                None => (file_name.to_string(), String::new()),
            };
            if stem.ends_with(&suffix) {
                return path;
            }
            path.with_file_name(format!("{stem}{suffix}{extension}"))
        };
        match self {
            Self::Project => {
                let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
                let mut candidate = cwd.as_path();
                loop {
                    let config = candidate.join("mise.toml");
                    if config.is_file() {
                        return Ok(with_environment(config));
                    }
                    let Some(parent) = candidate.parent() else {
                        break;
                    };
                    if parent == candidate {
                        break;
                    }
                    candidate = parent;
                }
                Ok(with_environment(cwd.join("mise.toml")))
            }
            Self::Global => {
                if let Ok(path) = std::env::var("MISE_CONFIG_FILE") {
                    if !path.is_empty() {
                        return Ok(with_environment(PathBuf::from(path)));
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
                Ok(with_environment(dir.join("config.toml")))
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
                    Ok(with_environment(path.join("mise.toml")))
                } else {
                    Ok(with_environment(path))
                }
            }
        }
    }
}

pub fn validate_environment_name(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > 64
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err("mise environment must contain only letters, numbers, '_' or '-'".into());
    }
    Ok(())
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
        let temporary = self.path.with_extension("toml.skills-tmp");
        fs::write(&temporary, after)
            .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
        if let Err(error) = replace_file(&temporary, &self.path) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("cannot replace {}: {error}", self.path.display()));
        }
        Ok(true)
    }
}

fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
    }
    #[cfg(windows)]
    {
        replace_file_windows(temporary, destination)
    }
}

#[cfg(windows)]
fn replace_file_windows(temporary: &Path, destination: &Path) -> io::Result<()> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    const ERROR_FILE_NOT_FOUND: i32 = 2;

    #[link(name = "kernel32")]
    extern "system" {
        #[link_name = "ReplaceFileW"]
        fn replace_file_w(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
    }

    let destination_path = destination;
    let temporary_path = temporary;
    let destination: Vec<u16> = destination_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let temporary: Vec<u16> = temporary_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // ReplaceFileW is atomic for an existing destination. A new config has no
    // destination for ReplaceFileW to replace, so retain rename semantics for
    // that one case; both paths are in the same directory.
    // SAFETY: all pointers reference NUL-terminated UTF-16 buffers or null
    // optional arguments, and the buffers remain alive for the call.
    let result = unsafe {
        replace_file_w(
            destination.as_ptr(),
            temporary.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if result != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) {
        fs::rename(temporary_path, destination_path)
    } else {
        Err(error)
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
        let allow_source_errors = bool_field(table, "allow_source_errors", key)?;
        result.push(ToolInfo {
            key: key.to_string(),
            source,
            version,
            track,
            subpath,
            include,
            exclude,
            agents,
            allow_source_errors,
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
    postinstall: &str,
    allow_source_errors: bool,
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
        if allow_source_errors {
            table["allow_source_errors"] = value(true);
        } else {
            table.remove("allow_source_errors");
        }
    }
    ensure_postinstall_command(document, postinstall)
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

pub fn update_cli_release(document: &mut DocumentMut, version: &str) -> Result<(), String> {
    let tools = document
        .get_mut("tools")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| "config has no [tools] table".to_string())?;
    let item = tools
        .get_mut("github:fullmeta-dev/skills")
        .ok_or_else(|| "config has no github:fullmeta-dev/skills CLI declaration".to_string())?;
    match item {
        Item::Table(table) => {
            if table.get("bin").and_then(Item::as_str) != Some("skills") {
                return Err("github:fullmeta-dev/skills must expose bin = \"skills\"".into());
            }
            if !table.contains_key("version") {
                return Err("github:fullmeta-dev/skills has no version".into());
            }
            replace_string(table, "version", version);
        }
        Item::Value(Value::InlineTable(table)) => {
            if table.get("bin").and_then(Value::as_str) != Some("skills") {
                return Err("github:fullmeta-dev/skills must expose bin = \"skills\"".into());
            }
            if !table.contains_key("version") {
                return Err("github:fullmeta-dev/skills has no version".into());
            }
            table.insert("version", Value::from(version));
        }
        _ => return Err("github:fullmeta-dev/skills must be a table with bin = \"skills\"".into()),
    }
    Ok(())
}

#[cfg(test)]
pub fn ensure_postinstall(document: &mut DocumentMut) -> Result<(), String> {
    ensure_postinstall_command(document, POSTINSTALL_COMMAND)
}

/// Add `postinstall` to the existing hook forms without replacing user content.
/// The command is scope-aware so a hook in a global or explicit config targets
/// that config rather than whatever project directory mise runs the hook in.
pub fn ensure_postinstall_command(
    document: &mut DocumentMut,
    postinstall: &str,
) -> Result<(), String> {
    let hooks = ensure_table(document, "hooks")?;
    match hooks.get_mut("postinstall") {
        None => {
            hooks.insert(
                "postinstall",
                Item::Value(Value::Array(string_array_values(
                    &[postinstall.to_string()],
                ))),
            );
        }
        Some(item) if item.as_str().is_some() => {
            let old = item.as_str().unwrap_or_default().to_string();
            match rewrite_managed_postinstall(&old, postinstall) {
                Some(rewritten) => {
                    if rewritten != old {
                        let value = item.as_value_mut().expect("string item has a value");
                        replace_value_preserving_decor(value, &rewritten);
                    }
                }
                None => {
                    let mut old_value = item.as_value().cloned().expect("string item has a value");
                    let decor = old_value.decor().clone();
                    old_value.decor_mut().clear();
                    let mut array = Array::new();
                    array.push_formatted(old_value);
                    array.push(postinstall);
                    let mut replacement = Value::Array(array);
                    *replacement.decor_mut() = decor;
                    *item = Item::Value(replacement);
                }
            }
        }
        Some(item) if item.as_array().is_some() => {
            let array = item.as_array_mut().expect("array checked");
            let mut managed = false;
            for entry in array.iter_mut() {
                if let Some(text) = entry.as_str().map(str::to_string) {
                    if let Some(rewritten) = rewrite_managed_postinstall(&text, postinstall) {
                        if rewritten != text {
                            *entry = Value::from(rewritten);
                        }
                        managed = true;
                    }
                } else if let Some(table) = entry.as_inline_table_mut() {
                    let old = table.get("run").and_then(Value::as_str).map(str::to_string);
                    if let Some(old) = old {
                        if let Some(rewritten) = rewrite_managed_postinstall(&old, postinstall) {
                            if rewritten != old {
                                table.insert("run", Value::from(rewritten));
                            }
                            managed = true;
                        }
                    }
                }
            }
            if !managed {
                array.push(postinstall);
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
                Some(old) => match rewrite_managed_postinstall(&old, postinstall) {
                    Some(rewritten) => {
                        if rewritten != old {
                            replace_string(table, "run", &rewritten);
                        }
                    }
                    None => replace_string(table, "run", &format!("{old}\n{postinstall}")),
                },
                None => {
                    table["run"] = value(postinstall);
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
                Some(old) => match rewrite_managed_postinstall(&old, postinstall) {
                    Some(rewritten) => {
                        if rewritten != old {
                            let run = table.get_mut("run").expect("run checked");
                            replace_value_preserving_decor(run, &rewritten);
                        }
                    }
                    None => {
                        let run = table.get_mut("run").expect("run checked");
                        replace_value_preserving_decor(run, &format!("{old}\n{postinstall}"));
                    }
                },
                None => {
                    table.insert("run", Value::from(postinstall));
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

/// Whether `value` is one of our generated `skills sync` hooks. A shell-composed
/// or wrapped command that merely starts with `skills sync` is the user's and is
/// preserved rather than rewritten.
fn is_postinstall_command(value: &str) -> bool {
    let value = value.trim();
    let Some(rest) = value.strip_prefix("skills sync") else {
        return false;
    };
    if !(rest.is_empty() || rest.starts_with(' ')) {
        return false;
    }
    let mut in_single_quotes = false;
    for character in rest.chars() {
        match character {
            '\'' => in_single_quotes = !in_single_quotes,
            _ if in_single_quotes => {}
            '&' | '|' | ';' | '<' | '>' | '$' | '`' | '(' | ')' => return false,
            _ => {}
        }
    }
    true
}

/// Rewrite each managed `skills sync` line to `desired`. Returns `None` when
/// `value` holds no managed command, so an unrelated hook is appended instead.
fn rewrite_managed_postinstall(value: &str, desired: &str) -> Option<String> {
    let mut found = false;
    let rewritten = value
        .split('\n')
        .map(|line| {
            if is_postinstall_command(line) {
                found = true;
                desired
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    found.then_some(rewritten)
}

fn bool_field(table: &Table, field: &str, key: &str) -> Result<bool, String> {
    match table.get(field) {
        None => Ok(false),
        Some(item) => item
            .as_bool()
            .ok_or_else(|| format!("skill tool {key:?} field {field:?} must be a boolean")),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(input: &str) -> DocumentMut {
        input.parse().unwrap()
    }

    #[test]
    fn save_if_changed_replaces_an_existing_config() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mise.toml");
        std::fs::write(&path, "[tools]\npython = \"3\"\n").unwrap();
        let mut config = Config::load_path(&path).unwrap();
        let before = config.document.to_string();
        config.document["tools"]["node"] = value("22");

        assert!(config.save_if_changed(&before).unwrap());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[tools]\npython = \"3\"\nnode = \"22\"\n"
        );
        assert!(!path.with_extension("toml.skills-tmp").exists());
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
    fn postinstall_deduplicates_managed_actions() {
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
            assert_eq!(rendered.matches(POSTINSTALL_COMMAND).count(), 1);
        }

        for input in [
            "[hooks]\npostinstall = \"skills sync --locked\"\n",
            "[hooks]\npostinstall = [\"skills sync --locked\"]\n",
            "[hooks.postinstall]\nrun = \"skills sync --locked\"\n",
            "[hooks]\npostinstall = { run = \"skills sync --locked\" }\n",
        ] {
            let mut document = doc(input);
            ensure_postinstall(&mut document).unwrap();
            assert_eq!(document.to_string().matches(POSTINSTALL_COMMAND).count(), 1);
        }
    }

    #[test]
    fn a_stale_managed_hook_is_rewritten_to_the_desired_scope() {
        for input in [
            "[hooks]\npostinstall = \"skills sync --locked\"\n",
            "[hooks]\npostinstall = [\"skills sync --locked\"]\n",
            "[hooks.postinstall]\nrun = \"skills sync --locked\"\n",
            "[hooks]\npostinstall = { run = \"skills sync --locked\" }\n",
        ] {
            let mut document = doc(input);
            ensure_postinstall_command(&mut document, "skills sync --locked --global").unwrap();
            let rendered = document.to_string();
            assert!(
                rendered.contains("skills sync --locked --global"),
                "{rendered}"
            );
            assert!(!rendered.contains("\"skills sync --locked\""), "{rendered}");
            assert_eq!(rendered.matches("skills sync").count(), 1, "{rendered}");
        }
    }

    #[test]
    fn shell_composed_managed_commands_are_preserved() {
        let mut document = doc("[hooks]\npostinstall = \"skills sync --locked && echo keep\"\n");
        ensure_postinstall_command(&mut document, "skills sync --locked --global").unwrap();
        let rendered = document.to_string();
        assert!(
            rendered.contains("skills sync --locked && echo keep"),
            "{rendered}"
        );
        assert!(
            rendered.contains("skills sync --locked --global"),
            "{rendered}"
        );
    }

    #[test]
    fn an_array_inline_table_managed_command_is_rewritten() {
        let mut document = doc(
            "[hooks]\npostinstall = [{ run = \"skills sync --locked\", env = { KEEP = \"yes\" } }]\n",
        );
        ensure_postinstall_command(&mut document, "skills sync --locked --global").unwrap();
        let rendered = document.to_string();
        assert!(
            rendered.contains("skills sync --locked --global"),
            "{rendered}"
        );
        assert!(rendered.contains("KEEP"), "{rendered}");
        assert_eq!(rendered.matches("skills sync").count(), 1, "{rendered}");
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
            POSTINSTALL_COMMAND,
            false,
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
            POSTINSTALL_COMMAND,
            false,
        )
        .is_err());
    }

    #[test]
    fn updates_an_inline_table_cli_release() {
        let mut document = doc(
            "[tools]\n\"github:fullmeta-dev/skills\" = { version = \"0.1.0\", bin = \"skills\" }\n",
        );
        update_cli_release(&mut document, "0.2.0").unwrap();
        assert!(document.to_string().contains("version = \"0.2.0\""));
    }
}
