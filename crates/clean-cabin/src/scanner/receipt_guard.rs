//! Lodge receipt guard — prevents auto-selecting files managed by Lodge.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Minimal subset of a receipt file, sufficient to extract placement paths.
#[derive(Deserialize)]
struct Receipt {
    #[serde(default)]
    placements: Vec<Placement>,
}

#[derive(Deserialize)]
struct Placement {
    destination: String,
}

/// Load the set of paths currently managed by Lodge from its receipt files.
///
/// Returns an empty set if the receipt directory is absent or unreadable.
pub fn load_receipt_paths() -> HashSet<PathBuf> {
    let receipt_dir = receipt_dir();
    let mut guarded = HashSet::new();

    let dir = match receipt_dir {
        Some(d) => d,
        None => return guarded,
    };

    let read_dir = match std::fs::read_dir(&dir) {
        Ok(d) => d,
        Err(_) => return guarded,
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(receipt) = serde_json::from_str::<Receipt>(&contents) {
                for placement in receipt.placements {
                    guarded.insert(PathBuf::from(placement.destination));
                }
            }
        }
    }

    guarded
}

/// Returns `true` if `path` is present in the Lodge receipt guard set.
pub fn is_guarded(path: &Path, guarded: &HashSet<PathBuf>) -> bool {
    guarded.contains(path)
}

fn receipt_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|b| PathBuf::from(b).join("lodge").join("receipts"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join(".local")
                .join("share")
                .join("lodge")
                .join("receipts")
        })
    }
}
