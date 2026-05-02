use std::path::PathBuf;

use lodge_shared::manifest::Manifest;

use super::manifest as manifest_parser;

/// A single installable package entry found in the local feed.
#[derive(Debug, Clone)]
pub struct FeedEntry {
    pub id: String,
    pub version: String,
    /// Absolute path to the package directory (contains `lodge.json`).
    pub path: PathBuf,
    pub manifest: Manifest,
}

/// Returns the local feed directory.
///
/// - Windows: `%LOCALAPPDATA%\lodge\feed\`
/// - Unix:    `~/.local/share/lodge/feed/`
pub fn feed_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base).join("lodge").join("feed")
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(base)
            .join(".local")
            .join("share")
            .join("lodge")
            .join("feed")
    }
}

/// Scans the feed directory and returns all valid package entries.
///
/// Each immediate subdirectory of [`feed_dir`] that contains a parseable
/// `lodge.json` is returned as a [`FeedEntry`]. Entries that cannot be
/// parsed are silently skipped.
pub fn scan() -> Vec<FeedEntry> {
    scan_dir(&feed_dir())
}

/// Scans a specific directory instead of the default feed dir.
/// Used by tests to point at a temp directory.
pub fn scan_dir(dir: &std::path::Path) -> Vec<FeedEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let pkg_path = e.path();
            let json = std::fs::read_to_string(pkg_path.join("lodge.json")).ok()?;
            let manifest = manifest_parser::parse(&json).ok()?;
            Some(FeedEntry {
                id: manifest.id.clone(),
                version: manifest.version.clone(),
                path: pkg_path,
                manifest,
            })
        })
        .collect()
}

/// Returns all feed entries for `id`, sorted newest-first by semver.
pub fn find_all(id: &str) -> Vec<FeedEntry> {
    let mut entries: Vec<FeedEntry> = scan().into_iter().filter(|e| e.id == id).collect();
    sort_newest_first(&mut entries);
    entries
}

/// Returns the latest (highest semver) feed entry for `id`.
pub fn find_latest(id: &str) -> Option<FeedEntry> {
    find_all(id).into_iter().next()
}

/// Returns the feed entry exactly matching `id` + `version`.
pub fn find_version(id: &str, version: &str) -> Option<FeedEntry> {
    scan().into_iter().find(|e| e.id == id && e.version == version)
}

/// Returns all entries whose `id` or `description` contains `query` (case-insensitive).
pub fn search(query: &str) -> Vec<FeedEntry> {
    let q = query.to_lowercase();
    let mut results: Vec<FeedEntry> = scan()
        .into_iter()
        .filter(|e| {
            e.id.to_lowercase().contains(&q)
                || e.manifest
                    .description
                    .as_deref()
                    .map(|d| d.to_lowercase().contains(&q))
                    .unwrap_or(false)
        })
        .collect();
    sort_newest_first(&mut results);
    results
}

fn sort_newest_first(entries: &mut [FeedEntry]) {
    entries.sort_by(|a, b| {
        let va = semver::Version::parse(&a.version).ok();
        let vb = semver::Version::parse(&b.version).ok();
        vb.cmp(&va)
    });
}

/// Formats a feed entry list for display in the command bar.
pub fn format_search_results(entries: &[FeedEntry]) -> String {
    if entries.is_empty() {
        return "nothing found in the local feed.".into();
    }
    entries
        .iter()
        .map(|e| {
            let desc = e
                .manifest
                .description
                .as_deref()
                .unwrap_or("no description");
            format!("  {}  v{}  — {}", e.id, e.version, desc)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_manifest(dir: &std::path::Path, folder: &str, json: &str) {
        let pkg = dir.join(folder);
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("lodge.json"), json).unwrap();
    }

    #[test]
    fn scan_empty_dir_returns_empty() {
        let dir = tempdir().unwrap();
        assert!(scan_dir(dir.path()).is_empty());
    }

    #[test]
    fn scan_nonexistent_dir_returns_empty() {
        let entries = scan_dir(std::path::Path::new("/nonexistent/lodge/feed"));
        assert!(entries.is_empty());
    }

    #[test]
    fn scan_finds_valid_packages() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "mytool",
            r#"{"id":"mytool","version":"1.0.0","type":"cli-tool"}"#,
        );
        let entries = scan_dir(dir.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "mytool");
        assert_eq!(entries[0].version, "1.0.0");
    }

    #[test]
    fn scan_skips_missing_manifest() {
        let dir = tempdir().unwrap();
        // Directory with no lodge.json
        std::fs::create_dir_all(dir.path().join("notapackage")).unwrap();
        assert!(scan_dir(dir.path()).is_empty());
    }

    #[test]
    fn scan_skips_invalid_manifest() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), "broken", r#"{"not":"valid"}"#);
        assert!(scan_dir(dir.path()).is_empty());
    }

    #[test]
    fn find_all_sorted_newest_first() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "tool-v1",
            r#"{"id":"tool","version":"1.0.0","type":"cli-tool"}"#,
        );
        write_manifest(
            dir.path(),
            "tool-v2",
            r#"{"id":"tool","version":"2.0.0","type":"cli-tool"}"#,
        );

        let mut entries = scan_dir(dir.path());
        entries.retain(|e| e.id == "tool");
        sort_newest_first(&mut entries);

        assert_eq!(entries[0].version, "2.0.0");
        assert_eq!(entries[1].version, "1.0.0");
    }

    #[test]
    fn search_matches_id() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "mytool",
            r#"{"id":"mytool","version":"1.0.0","type":"cli-tool"}"#,
        );
        write_manifest(
            dir.path(),
            "other",
            r#"{"id":"other","version":"1.0.0","type":"cli-tool"}"#,
        );

        let results: Vec<_> = scan_dir(dir.path())
            .into_iter()
            .filter(|e| e.id.contains("my"))
            .collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "mytool");
    }

    #[test]
    fn format_results_empty() {
        assert_eq!(
            format_search_results(&[]),
            "nothing found in the local feed."
        );
    }

    #[test]
    fn format_results_shows_id_and_version() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "mytool",
            r#"{"id":"mytool","version":"1.2.3","type":"cli-tool","description":"a great tool"}"#,
        );
        let entries = scan_dir(dir.path());
        let s = format_search_results(&entries);
        assert!(s.contains("mytool"));
        assert!(s.contains("1.2.3"));
        assert!(s.contains("a great tool"));
    }

    #[test]
    fn feed_dir_is_non_empty_path() {
        let dir = feed_dir();
        assert!(!dir.as_os_str().is_empty());
        assert!(dir.to_string_lossy().contains("lodge"));
    }
}
