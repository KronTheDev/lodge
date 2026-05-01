use anyhow::{Context, Result};

use lodge_shared::receipt::Receipt;

use super::attester;

/// Result of an uninstall operation.
pub struct UninstallResult {
    pub removed_files: Vec<String>,
    pub missing_files: Vec<String>,
    pub shim_removed: bool,
}

/// Reads the most recent receipt for `id`, removes every placed file,
/// unregisters the shim, and deletes the receipt.
///
/// Returns `Err` if no receipt is found. Partial failures (missing files,
/// undeleteable paths) are collected and returned in the result rather than
/// stopping the operation.
pub fn uninstall(id: &str) -> Result<UninstallResult> {
    let receipt = find_latest_receipt(id)
        .with_context(|| format!("no installation record found for '{id}'"))?;

    let mut removed_files = Vec::new();
    let mut missing_files = Vec::new();

    // Remove every placed file
    for placed in &receipt.placements {
        let path = std::path::Path::new(&placed.destination);
        if !path.exists() {
            missing_files.push(placed.destination.clone());
            continue;
        }
        match std::fs::remove_file(path) {
            Ok(_) => {
                removed_files.push(placed.destination.clone());
                // Remove parent directory if now empty
                if let Some(parent) = path.parent() {
                    let _ = std::fs::remove_dir(parent); // ignores non-empty
                }
            }
            Err(e) => {
                missing_files.push(format!("{} (couldn't remove: {e})", placed.destination));
            }
        }
    }

    // Unregister shim
    let shim_removed = super::super::shim::register::unregister(id).is_ok();

    // Delete the receipt file
    delete_receipt_for(&receipt)?;

    Ok(UninstallResult { removed_files, missing_files, shim_removed })
}

/// Finds the most recent receipt file for the given package id.
fn find_latest_receipt(id: &str) -> Option<Receipt> {
    attester::list_receipts()
        .into_iter()
        .find(|r| r.id == id)
}

/// Deletes the receipt file that matches a receipt's id + version + installed_at.
fn delete_receipt_for(receipt: &Receipt) -> Result<()> {
    let dir = attester::receipt_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(());
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(json) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(r) = serde_json::from_str::<Receipt>(&json) else {
            continue;
        };
        if r.id == receipt.id
            && r.version == receipt.version
            && r.installed_at == receipt.installed_at
        {
            std::fs::remove_file(&path)
                .with_context(|| format!("couldn't delete receipt {:?}", path))?;
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lodge_shared::receipt::{PlacedFile, Receipt};
    use std::sync::Mutex;
    use tempfile::tempdir;

    // Serialize tests that mutate LOCALAPPDATA to avoid env-var races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn write_fake_receipt(dir: &std::path::Path, id: &str, dest: &str) -> Receipt {
        let receipt = Receipt {
            id: id.to_string(),
            version: "1.0.0".into(),
            installed_at: "2026-01-01T00:00:00+00:00".into(),
            scope: "user".into(),
            placements: vec![PlacedFile {
                source: "bin/tool.exe".into(),
                destination: dest.to_string(),
                hash: "sha256:abc".into(),
            }],
            registrations: vec![],
            hooks_run: vec![],
            runtime_version: "0.1.0".into(),
            receipt_hash: "sha256:fake".into(),
        };
        let path = dir.join(format!("{id}-1.0.0-20260101T000000Z.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
        receipt
    }

    #[test]
    fn no_receipt_returns_err() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempdir().unwrap();
        unsafe { std::env::set_var("LOCALAPPDATA", tmp.path()) };
        let result = uninstall("nonexistent-package-xyz");
        unsafe { std::env::remove_var("LOCALAPPDATA") };
        assert!(result.is_err());
    }

    #[test]
    fn removes_placed_files() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempdir().unwrap();
        let receipts = tmp.path().join("lodge").join("receipts");
        std::fs::create_dir_all(&receipts).unwrap();

        let file = tmp.path().join("tool.exe");
        std::fs::write(&file, b"binary").unwrap();

        unsafe { std::env::set_var("LOCALAPPDATA", tmp.path()) };
        write_fake_receipt(&receipts, "testtool", &file.to_string_lossy());
        let result = uninstall("testtool").unwrap();
        unsafe { std::env::remove_var("LOCALAPPDATA") };

        assert!(result.removed_files.contains(&file.to_string_lossy().to_string()));
        assert!(!file.exists());
    }

    #[test]
    fn missing_file_reported_not_fatal() {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempdir().unwrap();
        let receipts = tmp.path().join("lodge").join("receipts");
        std::fs::create_dir_all(&receipts).unwrap();

        let missing = tmp.path().join("missing.exe").to_string_lossy().to_string();
        unsafe { std::env::set_var("LOCALAPPDATA", tmp.path()) };
        write_fake_receipt(&receipts, "missingtool", &missing);
        let result = uninstall("missingtool").unwrap();
        unsafe { std::env::remove_var("LOCALAPPDATA") };

        assert!(!result.missing_files.is_empty());
        assert!(result.removed_files.is_empty());
    }
}
