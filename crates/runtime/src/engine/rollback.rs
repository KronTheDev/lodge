use anyhow::Result;

use super::{attester, feed, installer, uninstall};

/// Result of a successful rollback.
#[derive(Debug)]
pub struct RollbackResult {
    /// The version that was uninstalled.
    pub from_version: String,
    /// The version now active.
    pub to_version: String,
}

/// Rolls back `id` to its previous installed version.
///
/// Algorithm:
/// 1. Read all receipts for `id` (newest-first) — needs at least two.
/// 2. Find the previous version in the local feed.
/// 3. Uninstall the current version.
/// 4. Re-install the previous version from the feed.
///
/// Fails clearly if:
/// - `id` is not installed
/// - There is no prior installation record to roll back to
/// - The prior version is no longer in the local feed
pub fn rollback(id: &str, runtime_version: &str) -> Result<RollbackResult> {
    let receipts: Vec<_> = attester::list_receipts()
        .into_iter()
        .filter(|r| r.id == id)
        .collect();

    if receipts.is_empty() {
        anyhow::bail!("{id} is not installed.");
    }
    if receipts.len() < 2 {
        anyhow::bail!(
            "no previous version of {id} to roll back to — only one install record found."
        );
    }

    let current_version = receipts[0].version.clone();
    let previous_version = receipts[1].version.clone();

    // The previous version must be in the feed to reinstall from it.
    let entry = feed::find_version(id, &previous_version).ok_or_else(|| {
        anyhow::anyhow!(
            "{id} v{previous_version} is not in the local feed. \
             add the package to {} and try again.",
            feed::feed_dir().display()
        )
    })?;

    uninstall::uninstall(id)?;
    installer::silent_install(&entry.path, runtime_version)?;

    Ok(RollbackResult {
        from_version: current_version,
        to_version: previous_version,
    })
}

/// Formats a [`RollbackResult`] for display in the command bar.
pub fn format_rollback_result(id: &str, result: &RollbackResult) -> String {
    format!(
        "{id} rolled back  v{} → v{}.",
        result.from_version, result.to_version
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_installed_returns_err() {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _lock = LOCK.lock().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let original = std::env::var("LOCALAPPDATA").ok();
        unsafe { std::env::set_var("LOCALAPPDATA", tmp.path()) };

        let result = rollback("nonexistent-xyz", "0.1.0");

        unsafe {
            match &original {
                Some(v) => std::env::set_var("LOCALAPPDATA", v),
                None => std::env::remove_var("LOCALAPPDATA"),
            }
        }

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not installed"));
    }

    #[test]
    fn single_receipt_returns_err() {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _lock = LOCK.lock().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let receipts_dir = tmp.path().join("lodge").join("receipts");
        std::fs::create_dir_all(&receipts_dir).unwrap();

        // Write exactly one receipt
        let receipt = lodge_shared::receipt::Receipt {
            id: "onlyone".into(),
            version: "1.0.0".into(),
            installed_at: "2026-01-01T00:00:00+00:00".into(),
            scope: "user".into(),
            placements: vec![],
            registrations: vec![],
            hooks_run: vec![],
            runtime_version: "0.1.0".into(),
            receipt_hash: "sha256:fake".into(),
        };
        let path = receipts_dir.join("onlyone-1.0.0-20260101T000000Z.json");
        std::fs::write(&path, serde_json::to_string(&receipt).unwrap()).unwrap();

        let original = std::env::var("LOCALAPPDATA").ok();
        unsafe { std::env::set_var("LOCALAPPDATA", tmp.path()) };

        let result = rollback("onlyone", "0.1.0");

        unsafe {
            match &original {
                Some(v) => std::env::set_var("LOCALAPPDATA", v),
                None => std::env::remove_var("LOCALAPPDATA"),
            }
        }

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no previous version"));
    }

    #[test]
    fn format_rollback_result_shows_versions() {
        let r = RollbackResult {
            from_version: "2.0.0".into(),
            to_version: "1.0.0".into(),
        };
        let s = format_rollback_result("mytool", &r);
        assert!(s.contains("2.0.0"));
        assert!(s.contains("1.0.0"));
        assert!(s.contains("→"));
    }
}
