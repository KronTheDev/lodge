//! Directory walker — produces a flat list of `FileEntry` values.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use walkdir::WalkDir;

/// A single file or directory discovered during a walk.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Absolute path to the file or directory.
    pub path: PathBuf,
    /// File size in bytes (0 for directories).
    pub size: u64,
    /// Last-access time, if available.
    pub accessed: Option<SystemTime>,
    /// Last-modification time, if available.
    pub modified: Option<SystemTime>,
    /// SHA-256 hex digest — populated lazily during heuristic scoring.
    #[allow(dead_code)]
    pub hash: Option<String>,
    /// True if the entry is a directory.
    pub is_dir: bool,
    /// True if the entry is an empty directory or a zero-byte file.
    pub is_empty: bool,
}

/// Directories that are never walked regardless of user configuration.
const HARD_EXCLUDES: &[&str] = &[
    // Windows
    "C:\\Windows",
    "C:\\Program Files",
    "C:\\Program Files (x86)",
    // macOS
    "/System",
    "/Library",
    // Unix common
    "/usr",
    "/etc",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/proc",
    "/sys",
    "/dev",
    "/run",
];

/// Returns `true` if `path` begins with any hard-excluded prefix.
fn is_hard_excluded(path: &Path) -> bool {
    HARD_EXCLUDES
        .iter()
        .any(|ex| path.starts_with(ex))
}

/// Returns `true` if `path` matches any user-configured exclude pattern.
///
/// Patterns are treated as simple prefix strings or glob-style `*` wildcards.
fn is_user_excluded(path: &Path, exclude: &[String]) -> bool {
    let path_str = path.to_string_lossy();
    exclude.iter().any(|pattern| {
        if pattern.contains('*') {
            // Simple glob: split on `*` and check all parts are present in order
            let parts: Vec<&str> = pattern.split('*').collect();
            let mut remaining = path_str.as_ref();
            for (i, part) in parts.iter().enumerate() {
                if part.is_empty() {
                    continue;
                }
                if i == 0 {
                    // First segment must be a prefix
                    if let Some(rest) = remaining.strip_prefix(part) {
                        remaining = rest;
                    } else {
                        return false;
                    }
                } else if let Some(pos) = remaining.find(part) {
                    remaining = &remaining[pos + part.len()..];
                } else {
                    return false;
                }
            }
            true
        } else {
            path_str.starts_with(pattern.as_str())
        }
    })
}

/// Walk `dirs`, returning all discoverable files and directories, filtered
/// by system hard-excludes and the user-supplied `exclude` list.
pub fn walk(dirs: &[PathBuf], exclude: &[String]) -> Vec<FileEntry> {
    let mut entries = Vec::new();

    for root in dirs {
        for result in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let p = e.path();
                !is_hard_excluded(p) && !is_user_excluded(p, exclude)
            })
            .flatten()
        {
            let path = result.path().to_path_buf();

            // Skip the root directory itself to avoid confusing the caller.
            if path == *root {
                continue;
            }

            let meta = match result.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let is_dir = meta.is_dir();
            let size = if is_dir { 0 } else { meta.len() };
            let accessed = meta.accessed().ok();
            let modified = meta.modified().ok();

            let is_empty = if is_dir {
                // A directory is empty if it has no direct children.
                std::fs::read_dir(&path)
                    .map(|mut d| d.next().is_none())
                    .unwrap_or(false)
            } else {
                size == 0
            };

            entries.push(FileEntry {
                path,
                size,
                accessed,
                modified,
                hash: None,
                is_dir,
                is_empty,
            });
        }
    }

    entries
}
