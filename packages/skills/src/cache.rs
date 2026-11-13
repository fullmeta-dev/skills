//! Persistent cache for read-only Git discovery.
//!
//! Two things are cached, both under the same directory:
//!
//! * resolved refs (`pins/`), which are cheap but require a network round trip
//!   and are therefore only trusted for a short, configurable TTL; and
//! * checked-out revisions (`checkouts/`), which are immutable by construction
//!   and therefore safe to reuse forever.
//!
//! A cached checkout is never written to; discovery only reads it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_TTL_SECONDS: u64 = 300;
const MARKER_SUFFIX: &str = ".ok";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinRecord {
    pub version: String,
    pub track: String,
    pub resolved_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheStats {
    pub dir: Option<String>,
    pub pins: usize,
    pub checkouts: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct Cache {
    root: Option<PathBuf>,
    ttl: Duration,
    refresh: bool,
}

impl Cache {
    /// Build a cache from the environment. `refresh` (a user flag) forces
    /// re-resolution of refs while still allowing immutable checkouts to be
    /// reused.
    pub fn from_env(refresh: bool) -> Self {
        if std::env::var_os("SKILLS_NO_CACHE").is_some_and(|value| value != "0") {
            return Self::disabled();
        }
        let ttl = std::env::var("SKILLS_CACHE_TTL")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(DEFAULT_TTL_SECONDS));
        Self {
            root: resolve_root(),
            ttl,
            refresh,
        }
    }

    /// A cache that stores nothing; every lookup misses.
    pub fn disabled() -> Self {
        Self {
            root: None,
            ttl: Duration::ZERO,
            refresh: true,
        }
    }

    fn pin_path(&self, url: &str, reference: Option<&str>) -> Option<PathBuf> {
        let root = self.root.as_ref()?;
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        hasher.update([0]);
        hasher.update(reference.unwrap_or("").as_bytes());
        Some(
            root.join("pins")
                .join(format!("{:x}.json", hasher.finalize())),
        )
    }

    pub fn read_pin(&self, url: &str, reference: Option<&str>) -> Option<PinRecord> {
        if self.refresh || self.ttl.is_zero() {
            return None;
        }
        let path = self.pin_path(url, reference)?;
        let record: PinRecord = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        let age = now().saturating_sub(record.resolved_at);
        (age <= self.ttl.as_secs()).then_some(record)
    }

    pub fn write_pin(&self, url: &str, reference: Option<&str>, version: &str, track: &str) {
        let Some(path) = self.pin_path(url, reference) else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let record = PinRecord {
            version: version.to_string(),
            track: track.to_string(),
            resolved_at: now(),
        };
        let Ok(contents) = serde_json::to_vec(&record) else {
            return;
        };
        let _ = write_atomic(&path, &contents);
    }

    fn checkout_dir(&self, url: &str, revision: &str) -> Option<PathBuf> {
        let root = self.root.as_ref()?;
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        Some(
            root.join("checkouts")
                .join(format!("{:x}", hasher.finalize()))
                .join(revision),
        )
    }

    /// A previously published checkout for this exact immutable revision.
    pub fn cached_checkout(&self, url: &str, revision: &str) -> Option<PathBuf> {
        let dir = self.checkout_dir(url, revision)?;
        marker_for(&dir).is_file().then_some(dir)
    }

    /// Move a fully built checkout into the cache. Returns the cache path on
    /// success, or `None` when caching is unavailable (the caller keeps its
    /// temporary directory).
    pub fn publish_checkout(&self, url: &str, revision: &str, built: &Path) -> Option<PathBuf> {
        let dir = self.checkout_dir(url, revision)?;
        let marker = marker_for(&dir);
        let parent = dir.parent()?;
        if fs::create_dir_all(parent).is_err() {
            return None;
        }
        if fs::write(&marker, b"ok\n").is_err() {
            return None;
        }
        if fs::rename(built, &dir).is_err() {
            let _ = fs::remove_file(&marker);
            // A concurrent run may have published the same revision first.
            return self.cached_checkout(url, revision);
        }
        Some(dir)
    }

    /// A base directory on the same filesystem as the cache, so a finished
    /// checkout can be renamed into place atomically. Falls back to the system
    /// temporary directory when caching is unavailable.
    pub fn scratch_base(&self) -> Result<PathBuf, String> {
        let base = match &self.root {
            Some(root) => {
                let tmp = root.join("tmp");
                fs::create_dir_all(&tmp)
                    .map_err(|error| format!("cannot create cache directory: {error}"))?;
                tmp
            }
            None => std::env::temp_dir(),
        };
        Ok(base)
    }

    pub fn stats(&self) -> CacheStats {
        let Some(root) = &self.root else {
            return CacheStats {
                dir: None,
                pins: 0,
                checkouts: 0,
                bytes: 0,
            };
        };
        let mut stats = CacheStats {
            dir: Some(root.display().to_string()),
            pins: 0,
            checkouts: 0,
            bytes: 0,
        };
        let pins = root.join("pins");
        if let Ok(entries) = fs::read_dir(&pins) {
            for entry in entries.flatten() {
                if entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "json")
                {
                    stats.pins += 1;
                    stats.bytes += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                }
            }
        }
        let checkouts = root.join("checkouts");
        if let Ok(repositories) = fs::read_dir(&checkouts) {
            for repository in repositories.flatten() {
                let Ok(revisions) = fs::read_dir(repository.path()) else {
                    continue;
                };
                for revision in revisions.flatten() {
                    let path = revision.path();
                    if path.is_dir() {
                        stats.checkouts += 1;
                    }
                    stats.bytes += directory_size(&path);
                }
            }
        }
        stats
    }

    pub fn clean(&self) -> Result<CacheStats, String> {
        let before = self.stats();
        let Some(root) = &self.root else {
            return Ok(before);
        };
        for name in ["pins", "checkouts", "tmp"] {
            let path = root.join(name);
            if path.exists() {
                fs::remove_dir_all(&path)
                    .map_err(|error| format!("cannot clean cache {}: {error}", path.display()))?;
            }
        }
        Ok(before)
    }
}

fn resolve_root() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("SKILLS_CACHE_DIR") {
        let value = PathBuf::from(value);
        return if value.as_os_str().is_empty() {
            None
        } else {
            Some(value)
        };
    }
    if let Some(value) = std::env::var_os("MISE_CACHE_DIR") {
        let value = PathBuf::from(value);
        if !value.as_os_str().is_empty() {
            return Some(value.join("skills"));
        }
    }
    if let Some(value) = std::env::var_os("XDG_CACHE_HOME") {
        let value = PathBuf::from(value);
        if !value.as_os_str().is_empty() {
            return Some(value.join("skills"));
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache").join("skills"))
}

fn marker_for(dir: &Path) -> PathBuf {
    let mut marker = dir.as_os_str().to_os_string();
    marker.push(MARKER_SUFFIX);
    PathBuf::from(marker)
}

fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents)?;
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

fn directory_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            total += directory_size(&entry.path());
        }
    }
    total
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_cache(root: &Path, ttl: u64, refresh: bool) -> Cache {
        Cache {
            root: Some(root.to_path_buf()),
            ttl: Duration::from_secs(ttl),
            refresh,
        }
    }

    #[test]
    fn disabled_cache_never_reads_or_writes() {
        let cache = Cache::disabled();
        assert!(cache.root.is_none());
        cache.write_pin(
            "https://example.invalid/r.git",
            Some("main"),
            "rev:abc",
            "main",
        );
        assert!(cache
            .read_pin("https://example.invalid/r.git", Some("main"))
            .is_none());
        assert!(cache
            .cached_checkout("https://example.invalid/r.git", "rev1")
            .is_none());
    }

    #[test]
    fn pin_records_respect_the_ttl_and_refresh() {
        let temp = TempDir::new().unwrap();
        let url = "https://example.invalid/r.git";
        let cache = make_cache(temp.path(), 300, false);
        assert!(cache.read_pin(url, Some("main")).is_none());
        cache.write_pin(url, Some("main"), "rev:abc", "main");
        let record = cache.read_pin(url, Some("main")).unwrap();
        assert_eq!(record.version, "rev:abc");
        assert_eq!(record.track, "main");
        // A zero TTL disables reads even when a record exists.
        let no_ttl = make_cache(temp.path(), 0, false);
        assert!(no_ttl.read_pin(url, Some("main")).is_none());
        // A forced refresh bypasses the cache.
        let refresh = make_cache(temp.path(), 300, true);
        assert!(refresh.read_pin(url, Some("main")).is_none());
        // Different refs are cached independently.
        assert!(cache.read_pin(url, Some("release")).is_none());
    }

    #[test]
    fn published_checkouts_are_moved_reused_and_cleaned() {
        let temp = TempDir::new().unwrap();
        let url = "https://example.invalid/r.git";
        let cache = make_cache(temp.path(), 300, false);
        let built = temp.path().join("built");
        fs::create_dir_all(built.join("skills/demo")).unwrap();
        fs::write(built.join("skills/demo/SKILL.md"), "x").unwrap();
        assert!(cache.cached_checkout(url, "rev1").is_none());
        let published = cache.publish_checkout(url, "rev1", &built).unwrap();
        assert!(published.is_dir());
        assert!(!built.exists(), "a published checkout is moved, not copied");
        assert_eq!(
            cache.cached_checkout(url, "rev1").as_deref(),
            Some(published.as_path())
        );
        // A checkout without its completion marker is not trusted.
        assert!(cache.cached_checkout(url, "rev2").is_none());
        let stats = cache.stats();
        assert_eq!(stats.pins, 0);
        assert_eq!(stats.checkouts, 1);
        assert!(stats.bytes > 0);
        cache.clean().unwrap();
        assert!(cache.cached_checkout(url, "rev1").is_none());
        assert_eq!(cache.stats().checkouts, 0);
    }
}
