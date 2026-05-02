use anyhow::Result;

use super::{attester, feed, installer};

/// Outcome of a single update attempt.
#[derive(Debug)]
pub enum UpdateResult {
    /// The package was updated from one version to another.
    Updated { from: String, to: String },
    /// The installed version is already the latest in the feed.
    AlreadyLatest { version: String },
    /// The package is not installed — cannot update.
    NotInstalled,
    /// The package is installed but not in the feed.
    NotInFeed,
}

/// Checks the local feed for a newer version of `id` and installs it if found.
pub fn update(id: &str, runtime_version: &str) -> Result<UpdateResult> {
    // Find what's currently installed.
    let receipts = attester::list_receipts();
    let installed = match receipts.into_iter().find(|r| r.id == id) {
        Some(r) => r,
        None => return Ok(UpdateResult::NotInstalled),
    };

    // Find the latest version in the feed.
    let entry = match feed::find_latest(id) {
        Some(e) => e,
        None => return Ok(UpdateResult::NotInFeed),
    };

    let installed_ver = semver::Version::parse(&installed.version)
        .map_err(|e| anyhow::anyhow!("installed version '{}' is not valid semver: {e}", installed.version))?;
    let feed_ver = semver::Version::parse(&entry.version)
        .map_err(|e| anyhow::anyhow!("feed version '{}' is not valid semver: {e}", entry.version))?;

    if feed_ver <= installed_ver {
        return Ok(UpdateResult::AlreadyLatest {
            version: installed.version,
        });
    }

    installer::silent_install(&entry.path, runtime_version)?;

    Ok(UpdateResult::Updated {
        from: installed.version,
        to: entry.version,
    })
}

/// Attempts to update every installed package that appears in the feed.
///
/// Returns one result per installed package id (deduplicated to latest install).
pub fn update_all(runtime_version: &str) -> Vec<(String, Result<UpdateResult>)> {
    // Deduplicate installed packages — keep newest receipt per id.
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<String> = attester::list_receipts()
        .into_iter()
        .filter(|r| seen.insert(r.id.clone()))
        .map(|r| r.id)
        .collect();

    ids.into_iter()
        .map(|id| {
            let result = update(&id, runtime_version);
            (id, result)
        })
        .collect()
}

/// Formats an [`UpdateResult`] for display in the command bar.
pub fn format_update_result(id: &str, result: &UpdateResult) -> String {
    match result {
        UpdateResult::Updated { from, to } => {
            format!("{id} updated  v{from} → v{to}.")
        }
        UpdateResult::AlreadyLatest { version } => {
            format!("{id} v{version} is already the latest.")
        }
        UpdateResult::NotInstalled => {
            format!("{id} is not installed.")
        }
        UpdateResult::NotInFeed => {
            format!(
                "{id} is not in the local feed. add the package to {} to enable updates.",
                feed::feed_dir().display()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_installed_when_no_receipts() {
        // With an empty receipt dir (LOCALAPPDATA → temp), update returns NotInstalled.
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _lock = LOCK.lock().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let original = std::env::var("LOCALAPPDATA").ok();
        unsafe { std::env::set_var("LOCALAPPDATA", tmp.path()) };

        let result = update("nonexistent-xyz", "0.1.0").unwrap();

        unsafe {
            match &original {
                Some(v) => std::env::set_var("LOCALAPPDATA", v),
                None => std::env::remove_var("LOCALAPPDATA"),
            }
        }

        assert!(matches!(result, UpdateResult::NotInstalled));
    }

    #[test]
    fn format_already_latest() {
        let r = UpdateResult::AlreadyLatest {
            version: "1.0.0".into(),
        };
        let s = format_update_result("mytool", &r);
        assert!(s.contains("already the latest"));
        assert!(s.contains("1.0.0"));
    }

    #[test]
    fn format_updated() {
        let r = UpdateResult::Updated {
            from: "1.0.0".into(),
            to: "2.0.0".into(),
        };
        let s = format_update_result("mytool", &r);
        assert!(s.contains("1.0.0"));
        assert!(s.contains("2.0.0"));
        assert!(s.contains("→"));
    }

    #[test]
    fn format_not_in_feed() {
        let r = UpdateResult::NotInFeed;
        let s = format_update_result("mytool", &r);
        assert!(s.contains("not in the local feed"));
    }

    #[test]
    fn format_not_installed() {
        let r = UpdateResult::NotInstalled;
        let s = format_update_result("mytool", &r);
        assert!(s.contains("not installed"));
    }
}
