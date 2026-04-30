use std::path::PathBuf;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use lodge_shared::{
    manifest::{Manifest, Scope},
    placement::PlacementPlan,
    receipt::{PlacedFile, Receipt},
};

use super::executor::effective_destination;

/// Writes a tamper-evident receipt for a completed installation and returns it.
///
/// The `receipt_hash` field is SHA-256 of the full receipt JSON with that field
/// set to an empty string, making receipts independently verifiable.
///
/// Receipt path:
/// - Windows: `%LOCALAPPDATA%\lodge\receipts\{id}-{version}-{ts}.json`
/// - Unix:    `~/.local/share/lodge/receipts/{id}-{version}-{ts}.json`
pub fn write_receipt(
    manifest: &Manifest,
    plan: &PlacementPlan,
    scope: &Scope,
    hooks_run: Vec<String>,
    runtime_version: &str,
) -> Result<Receipt> {
    let installed_at = chrono::Utc::now().to_rfc3339();

    // Build placement records by reading the destination files that were placed.
    let placements: Vec<PlacedFile> = plan
        .entries
        .iter()
        .filter_map(|entry| {
            let dest = effective_destination(entry);
            if !dest.exists() {
                return None; // skip entries that didn't place successfully
            }
            let bytes = std::fs::read(&dest).ok()?;
            Some(PlacedFile {
                source: entry.source.to_string_lossy().into_owned(),
                destination: dest.to_string_lossy().into_owned(),
                hash: format!("sha256:{}", sha256_hex(&bytes)),
            })
        })
        .collect();

    let scope_str = match scope {
        Scope::User => "user",
        Scope::System => "system",
    }
    .to_string();

    let registrations: Vec<String> = {
        let mut r = Vec::new();
        if plan.registrations.add_to_path {
            r.push("PATH".into());
        }
        if let Some(ref var) = plan.registrations.env_var {
            r.push(var.clone());
        }
        if plan.registrations.start_menu_entry {
            r.push("StartMenu".into());
        }
        r
    };

    // Build receipt without hash, then hash it.
    let mut receipt = Receipt {
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        installed_at,
        scope: scope_str,
        placements,
        registrations,
        hooks_run,
        runtime_version: runtime_version.to_string(),
        receipt_hash: String::new(),
    };

    let json_without_hash =
        serde_json::to_string(&receipt).context("couldn't serialise receipt for hashing")?;
    receipt.receipt_hash = format!("sha256:{}", sha256_hex(json_without_hash.as_bytes()));

    // Write to disk.
    let dir = receipt_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("couldn't create receipt directory {:?}", dir))?;

    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let filename = format!("{}-{}-{}.json", manifest.id, manifest.version, ts);
    let path = dir.join(&filename);

    let pretty = serde_json::to_string_pretty(&receipt).context("couldn't serialise receipt")?;
    std::fs::write(&path, pretty)
        .with_context(|| format!("couldn't write receipt to {:?}", path))?;

    Ok(receipt)
}

/// Returns the directory where Lodge stores installation receipts.
pub fn receipt_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base).join("lodge").join("receipts")
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(base)
            .join(".local")
            .join("share")
            .join("lodge")
            .join("receipts")
    }
}

/// Verifies a receipt's `receipt_hash` field against its own contents.
///
/// Returns `true` if the receipt is intact, `false` if it has been tampered with.
#[allow(dead_code)]
pub fn verify_receipt(receipt: &Receipt) -> bool {
    let stored_hash = receipt.receipt_hash.clone();
    let mut copy = receipt.clone();
    copy.receipt_hash = String::new();
    let Ok(json) = serde_json::to_string(&copy) else {
        return false;
    };
    format!("sha256:{}", sha256_hex(json.as_bytes())) == stored_hash
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lodge_shared::placement::{PlacementEntry, PlacementPlan, RegistrationEffects};
    use lodge_shared::manifest::{Manifest, PackageType, Prefers, Scope};
    use std::fs;
    use tempfile::tempdir;

    fn minimal_manifest(id: &str) -> Manifest {
        Manifest {
            id: id.to_string(),
            version: "1.0.0".into(),
            package_type: PackageType::CliTool,
            description: None,
            author: None,
            prefers: Prefers { scope: Some(Scope::User), ..Default::default() },
            requires: Default::default(),
            naming: Default::default(),
            overrides: vec![],
            hooks: Default::default(),
        }
    }

    fn empty_plan() -> PlacementPlan {
        PlacementPlan {
            entries: vec![],
            registrations: RegistrationEffects::default(),
            hooks_order: vec![],
            requires_elevation: false,
        }
    }

    #[test]
    fn sha256_hex_produces_64_char_lowercase_hex() {
        let h = sha256_hex(b"hello");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sha256_is_deterministic() {
        assert_eq!(sha256_hex(b"lodge"), sha256_hex(b"lodge"));
    }

    #[test]
    fn sha256_differs_for_different_input() {
        assert_ne!(sha256_hex(b"a"), sha256_hex(b"b"));
    }

    #[test]
    fn receipt_hash_verifies() {
        let dst_dir = tempdir().unwrap();
        let dst = dst_dir.path().join("tool");
        fs::write(&dst, b"binary").unwrap();

        let mut plan = empty_plan();
        plan.entries.push(PlacementEntry {
            source: dst.clone(),
            destination: dst.clone(),
            rename: None,
        });

        // write_receipt needs a real dir — override receipt_dir behaviour by using
        // the function with a manual path. We test verify_receipt directly instead.
        let manifest = minimal_manifest("testpkg");
        let mut receipt = lodge_shared::receipt::Receipt {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            installed_at: "2026-01-01T00:00:00+00:00".into(),
            scope: "user".into(),
            placements: vec![],
            registrations: vec![],
            hooks_run: vec![],
            runtime_version: "0.1.0".into(),
            receipt_hash: String::new(),
        };
        let json = serde_json::to_string(&receipt).unwrap();
        receipt.receipt_hash = format!("sha256:{}", sha256_hex(json.as_bytes()));

        assert!(verify_receipt(&receipt));
    }

    #[test]
    fn tampered_receipt_fails_verification() {
        let mut receipt = lodge_shared::receipt::Receipt {
            id: "tool".into(),
            version: "1.0.0".into(),
            installed_at: "2026-01-01T00:00:00+00:00".into(),
            scope: "user".into(),
            placements: vec![],
            registrations: vec![],
            hooks_run: vec![],
            runtime_version: "0.1.0".into(),
            receipt_hash: "sha256:fakehash".into(),
        };
        assert!(!verify_receipt(&receipt));
        // Tampering after a valid hash
        let json = serde_json::to_string(&{ let mut r = receipt.clone(); r.receipt_hash = String::new(); r }).unwrap();
        receipt.receipt_hash = format!("sha256:{}", sha256_hex(json.as_bytes()));
        // Now tamper
        receipt.version = "9.9.9".into();
        assert!(!verify_receipt(&receipt));
    }
}
