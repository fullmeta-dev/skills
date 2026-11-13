//! Keep generated skill links out of `git status`.
//!
//! Activation links point into this machine's mise installs, so a committed one
//! breaks on every other machine. They are listed in the repository's
//! `.git/info/exclude`, which belongs to this clone alone, in a block this
//! program owns and rewrites. Nothing in the working tree changes, and
//! handwritten skills beside the links stay visible to git.
//!
//! A linked worktree is left alone: it shares `info/exclude` with the main
//! checkout and every other worktree, and a pattern there would apply to all of
//! them, hiding another worktree's handwritten skills.

use fslock::LockFile;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Where a skills directory lives in its repository's working tree, and that
/// repository's `info/exclude`. `None` outside a repository.
struct Repo {
    /// The directory from the working-tree root, as an anchored pattern prefix.
    prefix: String,
    exclude: PathBuf,
}

/// The git directory of the repository at `dot_git`, which is either a `.git`
/// directory or the file a submodule or linked worktree has instead. `None` for
/// a linked worktree, whose `info/exclude` is shared and not its own.
fn own_git_dir(dot_git: &Path) -> Option<PathBuf> {
    let git_dir = if dot_git.is_dir() {
        dot_git.to_path_buf()
    } else {
        let text = fs::read_to_string(dot_git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        dot_git.parent()?.join(target)
    };
    (!git_dir.join("commondir").exists()).then_some(git_dir)
}

/// Resolve `dir` to an absolute path even when it does not exist yet, by
/// canonicalizing the nearest existing ancestor and re-appending the missing
/// tail. A first sync creates the adapter directory only while linking, so the
/// rollback snapshot must resolve the exclude file before that happens.
fn canonical_dir(dir: &Path) -> Option<PathBuf> {
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut current = dir;
    loop {
        if let Ok(resolved) = fs::canonicalize(current) {
            let mut result = resolved;
            for part in missing.iter().rev() {
                result.push(part);
            }
            return Some(result);
        }
        let name = current.file_name()?.to_os_string();
        missing.push(name);
        current = current.parent()?;
    }
}

fn repo_of(dir: &Path) -> Result<Option<Repo>, String> {
    let Some(dir) = canonical_dir(dir) else {
        return Ok(None);
    };
    let Some(root) = dir.ancestors().find(|path| path.join(".git").exists()) else {
        return Ok(None);
    };
    let Some(git_dir) = own_git_dir(&root.join(".git")) else {
        return Ok(None);
    };
    let Ok(relative) = dir.strip_prefix(root) else {
        return Ok(None);
    };
    let mut prefix = String::new();
    for part in relative.components() {
        let Component::Normal(part) = part else {
            return Ok(None);
        };
        let Some(part) = part.to_str() else {
            return Ok(None);
        };
        prefix.push('/');
        prefix.push_str(&escape(part)?);
    }
    Ok(Some(Repo {
        prefix,
        exclude: git_dir.join("info").join("exclude"),
    }))
}

/// A name as a literal gitignore pattern. A line break would end the pattern and
/// start another, so a name holding one is refused.
fn escape(name: &str) -> Result<String, String> {
    if name.contains(['\n', '\r']) {
        return Err(format!(
            "{name:?} has a line break and cannot be written as an ignore pattern"
        ));
    }
    let mut out = String::with_capacity(name.len());
    let last = name.chars().count().saturating_sub(1);
    for (index, character) in name.chars().enumerate() {
        // Git drops trailing spaces from a pattern unless they are escaped.
        if matches!(character, '\\' | '*' | '?' | '[' | ']')
            || (index == 0 && matches!(character, '#' | '!' | ' '))
            || (index == last && character == ' ')
        {
            out.push('\\');
        }
        out.push(character);
    }
    Ok(out)
}

fn markers(prefix: &str) -> (String, String) {
    (
        format!("# >>> skills ({prefix})"),
        format!("# <<< skills ({prefix})"),
    )
}

/// `text` with the block between `begin` and `end` replaced by `entries`, or
/// removed when there are none, and every other line left as it was. A marker
/// with no end after it is one a person cut into: only the marker goes, and
/// what followed it stays theirs. The file's existing line ending is kept so
/// unrelated content is not rewritten from CRLF to LF.
fn with_block(text: &str, begin: &str, end: &str, entries: &[String]) -> String {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<&str> = text.lines().collect();
    while let Some(start) = lines.iter().position(|line| *line == begin) {
        let stop = lines[start..].iter().position(|line| *line == end);
        lines.drain(start..stop.map_or(start + 1, |n| start + n + 1));
    }
    let mut out = lines.join(newline);
    if !entries.is_empty() {
        if !out.is_empty() {
            out.push_str(newline);
        }
        out.push_str(begin);
        for entry in entries {
            out.push_str(newline);
            out.push_str(entry);
        }
        out.push_str(newline);
        out.push_str(end);
    }
    if !out.is_empty() {
        out.push_str(newline);
    }
    out
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let temporary = path.with_extension(format!("skills-tmp-{}", std::process::id()));
    fs::write(&temporary, contents)
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot replace {}: {error}", path.display()));
    }
    Ok(())
}

/// Make the repository ignore `names` in `dir`, and nothing else there. With no
/// names, stop ignoring what an earlier call listed. Does nothing outside a git
/// repository or in a linked worktree.
pub fn exclude_links(dir: &Path, names: &[String]) -> Result<(), String> {
    let Some(repo) = repo_of(dir)? else {
        return Ok(());
    };
    if std::env::var("MISE_SKILLS_FAIL_GIT_EXCLUDE")
        .ok()
        .as_deref()
        == Some("1")
    {
        return Err("injected git exclude failure".into());
    }
    let entries = names
        .iter()
        .map(|name| Ok(format!("{}/{}", repo.prefix, escape(name)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let (begin, end) = markers(&repo.prefix);
    if let Some(parent) = repo.exclude.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "cannot create git exclude directory {}: {error}",
                parent.display()
            )
        })?;
    }
    // Serialize read-modify-write across processes so two syncs for different
    // directories in one repository cannot drop each other's block. The lock is
    // advisory and is released when the handle drops.
    let mut lock =
        LockFile::open(&repo.exclude.with_extension("skills-lock")).map_err(|error| {
            format!(
                "cannot open git exclude lock for {}: {error}",
                repo.exclude.display()
            )
        })?;
    lock.lock().map_err(|error| {
        format!(
            "cannot lock git exclude {}: {error}",
            repo.exclude.display()
        )
    })?;
    // Only a file that is not there is empty. One that cannot be read, for
    // instance because a pattern in it is not UTF-8, is somebody's rules, and
    // writing over it would lose them.
    let before = match fs::read_to_string(&repo.exclude) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(format!(
                "cannot read git exclude {}: {error}",
                repo.exclude.display()
            ))
        }
    };
    let after = with_block(&before, &begin, &end, &entries);
    if after != before {
        write_atomic(&repo.exclude, &after)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn repo() -> TempDir {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join(".git")).unwrap();
        fs::create_dir_all(root.path().join("app/.agents/skills")).unwrap();
        root
    }

    fn exclude_of(root: &Path) -> String {
        fs::read_to_string(root.join(".git/info/exclude")).unwrap_or_default()
    }

    #[test]
    fn links_are_listed_anchored_beside_what_was_already_there() {
        let root = repo();
        fs::create_dir_all(root.path().join(".git/info")).unwrap();
        fs::write(root.path().join(".git/info/exclude"), "*.swp\n").unwrap();
        let dir = root.path().join("app/.agents/skills");
        exclude_links(&dir, &["aube".to_string(), "hk".to_string()]).unwrap();
        assert_eq!(
            exclude_of(root.path()),
            "*.swp\n\
             # >>> skills (/app/.agents/skills)\n\
             /app/.agents/skills/aube\n\
             /app/.agents/skills/hk\n\
             # <<< skills (/app/.agents/skills)\n"
        );
    }

    #[test]
    fn the_block_follows_the_links_and_leaves_with_the_last_one() {
        let root = repo();
        let dir = root.path().join("app/.agents/skills");
        exclude_links(&dir, &["aube".to_string(), "hk".to_string()]).unwrap();
        exclude_links(&dir, &["hk".to_string()]).unwrap();
        let text = exclude_of(root.path());
        assert!(text.contains("/app/.agents/skills/hk\n"));
        assert!(!text.contains("aube"));
        exclude_links(&dir, &[]).unwrap();
        assert_eq!(exclude_of(root.path()), "");
    }

    #[test]
    fn each_directory_keeps_its_own_block() {
        let root = repo();
        let a = root.path().join("app/.agents/skills");
        let b = root.path().join("app/.claude/skills");
        fs::create_dir_all(&b).unwrap();
        exclude_links(&a, &["one".to_string()]).unwrap();
        exclude_links(&b, &["two".to_string()]).unwrap();
        exclude_links(&a, &[]).unwrap();
        let text = exclude_of(root.path());
        assert!(text.contains("/app/.claude/skills/two\n"));
        assert!(!text.contains(".agents"));
    }

    #[test]
    fn a_marker_with_no_end_costs_only_the_marker() {
        let root = repo();
        fs::create_dir_all(root.path().join(".git/info")).unwrap();
        fs::write(
            root.path().join(".git/info/exclude"),
            "# >>> skills (/app/.agents/skills)\nmine\n",
        )
        .unwrap();
        let dir = root.path().join("app/.agents/skills");
        exclude_links(&dir, &[]).unwrap();
        assert_eq!(exclude_of(root.path()), "mine\n");
    }

    #[test]
    fn names_are_literal_patterns() {
        assert_eq!(escape("a*b").unwrap(), "a\\*b");
        assert_eq!(escape("!bang").unwrap(), "\\!bang");
        assert_eq!(escape("plain-name_1").unwrap(), "plain-name_1");
        // Git drops a trailing space unless it is escaped.
        assert_eq!(escape("demo ").unwrap(), "demo\\ ");
        // A line break would start another pattern.
        assert!(escape("demo\n*").is_err());
        assert!(escape("demo\r").is_err());
    }

    #[test]
    fn an_exclude_file_that_cannot_be_read_is_left_as_it_is() {
        let root = repo();
        fs::create_dir_all(root.path().join(".git/info")).unwrap();
        let exclude = root.path().join(".git/info/exclude");
        fs::write(&exclude, b"caf\xe9\n").unwrap();
        let dir = root.path().join("app/.agents/skills");
        assert!(exclude_links(&dir, &["a".to_string()]).is_err());
        assert_eq!(fs::read(&exclude).unwrap(), b"caf\xe9\n");
    }

    #[test]
    fn a_linked_worktree_is_left_alone() {
        let main = TempDir::new().unwrap();
        let wt_git = main.path().join(".git/worktrees/wt");
        fs::create_dir_all(&wt_git).unwrap();
        fs::write(wt_git.join("commondir"), "../..\n").unwrap();
        let work = TempDir::new().unwrap();
        fs::write(
            work.path().join(".git"),
            format!("gitdir: {}\n", wt_git.display()),
        )
        .unwrap();
        let dir = work.path().join("skills");
        fs::create_dir_all(&dir).unwrap();
        exclude_links(&dir, &["a".to_string()]).unwrap();
        assert!(!main.path().join(".git/info").exists());
        assert!(!wt_git.join("info").exists());
    }

    #[test]
    fn a_submodule_keeps_its_own_exclude() {
        let super_repo = TempDir::new().unwrap();
        let modules = super_repo.path().join(".git/modules/sub");
        fs::create_dir_all(&modules).unwrap();
        let work = super_repo.path().join("sub");
        fs::create_dir_all(work.join("skills")).unwrap();
        fs::write(work.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();
        exclude_links(&work.join("skills"), &["a".to_string()]).unwrap();
        let text = fs::read_to_string(modules.join("info/exclude")).unwrap();
        assert!(text.contains("/skills/a\n"));
    }

    #[test]
    fn existing_line_endings_are_preserved() {
        let root = repo();
        fs::create_dir_all(root.path().join(".git/info")).unwrap();
        fs::write(root.path().join(".git/info/exclude"), "*.swp\r\n").unwrap();
        let dir = root.path().join("app/.agents/skills");
        exclude_links(&dir, &["demo".to_string()]).unwrap();
        let text = exclude_of(root.path());
        assert!(text.contains("*.swp\r\n"), "kept content: {text:?}");
        assert!(
            text.contains("/app/.agents/skills/demo\r\n"),
            "block uses CRLF: {text:?}"
        );
        assert!(!text.contains("\n\r"), "no mixed endings: {text:?}");
    }

    #[test]
    fn outside_a_repository_nothing_is_written() {
        let dir = TempDir::new().unwrap();
        exclude_links(dir.path(), &["a".to_string()]).unwrap();
        assert!(!dir.path().join(".git").exists());
    }

    #[test]
    fn a_directory_that_does_not_exist_yet_still_resolves_its_exclude() {
        let root = repo();
        let missing = root.path().join("app/.pi/skills");
        assert!(!missing.exists());
        exclude_links(&missing, &["demo".to_string()]).unwrap();
        assert!(exclude_of(root.path()).contains("/app/.pi/skills/demo\n"));
    }
}
