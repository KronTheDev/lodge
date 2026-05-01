use sha2::{Digest, Sha256};

use lodge_shared::receipt::PlacedFile;

use super::attester;

/// Outcome of verifying a single placed file.
#[derive(Debug)]
pub struct FileVerification {
    pub destination: String,
    pub status: VerifyStatus,
}

/// Status of a single file during verification.
#[derive(Debug, PartialEq, Eq)]
pub enum VerifyStatus {
    /// File matches the receipt hash exactly.
    Ok,
    /// File is present but has been modified since install.
    Modified,
    /// File is missing from disk entirely.
    Missing,
}

/// Result of verifying an entire installation.
pub struct VerifyResult {
    pub id: String,
    pub version: String,
    pub receipt_intact: bool,
    pub files: Vec<FileVerification>,
}

impl VerifyResult {
    /// True if the receipt is intact and all files match their recorded hashes.
    pub fn is_clean(&self) -> bool {
        self.receipt_intact && self.files.iter().all(|f| f.status == VerifyStatus::Ok)
    }
}

/// Verifies the installed files for `id` against the most recent receipt.
///
/// Checks:
/// 1. Receipt tamper-evidence (`receipt_hash`)
/// 2. Each placed file exists on disk
/// 3. Each placed file's SHA-256 matches the recorded hash
pub fn verify(id: &str) -> anyhow::Result<VerifyResult> {
    let receipt = attester::list_receipts()
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| anyhow::anyhow!("no installation record found for '{id}'"))?;

    let receipt_intact = attester::verify_receipt(&receipt);

    let files = receipt
        .placements
        .iter()
        .map(verify_file)
        .collect();

    Ok(VerifyResult {
        id: receipt.id.clone(),
        version: receipt.version.clone(),
        receipt_intact,
        files,
    })
}

fn verify_file(placed: &PlacedFile) -> FileVerification {
    let path = std::path::Path::new(&placed.destination);

    if !path.exists() {
        return FileVerification {
            destination: placed.destination.clone(),
            status: VerifyStatus::Missing,
        };
    }

    let status = match std::fs::read(path) {
        Ok(bytes) => {
            let actual = format!("sha256:{}", sha256_hex(&bytes));
            if actual == placed.hash {
                VerifyStatus::Ok
            } else {
                VerifyStatus::Modified
            }
        }
        Err(_) => VerifyStatus::Missing,
    };

    FileVerification { destination: placed.destination.clone(), status }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

/// Formats a [`VerifyResult`] as calm plain-language output for the command bar.
pub fn format_verify_result(result: &VerifyResult) -> String {
    if result.is_clean() {
        return format!("{} v{} is intact.", result.id, result.version);
    }

    let mut lines = vec![format!("{} v{} has issues:", result.id, result.version)];

    if !result.receipt_intact {
        lines.push("  receipt has been tampered with.".into());
    }

    for f in &result.files {
        let short = shorten_path(&f.destination);
        match f.status {
            VerifyStatus::Ok => {}
            VerifyStatus::Modified => lines.push(format!("  modified  {short}")),
            VerifyStatus::Missing => lines.push(format!("  missing   {short}")),
        }
    }

    lines.join("\n")
}

fn shorten_path(path: &str) -> String {
    // Show only the last two path components for readability
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    match parts.len() {
        0 => path.to_string(),
        1 => parts[0].to_string(),
        _ => format!("…\\{}", parts[parts.len() - 1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lodge_shared::receipt::PlacedFile;
    use tempfile::tempdir;

    fn placed(dest: &str, hash: &str) -> PlacedFile {
        PlacedFile {
            source: "src".into(),
            destination: dest.to_string(),
            hash: hash.to_string(),
        }
    }

    #[test]
    fn missing_file_detected() {
        let p = placed("/nonexistent/path/tool.exe", "sha256:abc");
        let fv = verify_file(&p);
        assert_eq!(fv.status, VerifyStatus::Missing);
    }

    #[test]
    fn correct_hash_gives_ok() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("tool.exe");
        std::fs::write(&file, b"binary content").unwrap();

        let hash = format!("sha256:{}", {
            let mut h = Sha256::new();
            h.update(b"binary content");
            format!("{:x}", h.finalize())
        });

        let p = placed(&file.to_string_lossy(), &hash);
        let fv = verify_file(&p);
        assert_eq!(fv.status, VerifyStatus::Ok);
    }

    #[test]
    fn wrong_hash_gives_modified() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("tool.exe");
        std::fs::write(&file, b"tampered content").unwrap();

        let p = placed(&file.to_string_lossy(), "sha256:originalhash");
        let fv = verify_file(&p);
        assert_eq!(fv.status, VerifyStatus::Modified);
    }

    #[test]
    fn clean_result_formats_simply() {
        let result = VerifyResult {
            id: "mytool".into(),
            version: "1.0.0".into(),
            receipt_intact: true,
            files: vec![FileVerification {
                destination: "C:\\path\\mytool.exe".into(),
                status: VerifyStatus::Ok,
            }],
        };
        assert!(result.is_clean());
        let s = format_verify_result(&result);
        assert!(s.contains("intact"));
    }

    #[test]
    fn modified_file_shows_in_output() {
        let result = VerifyResult {
            id: "mytool".into(),
            version: "1.0.0".into(),
            receipt_intact: true,
            files: vec![FileVerification {
                destination: "C:\\path\\mytool.exe".into(),
                status: VerifyStatus::Modified,
            }],
        };
        assert!(!result.is_clean());
        let s = format_verify_result(&result);
        assert!(s.contains("modified"));
    }

    #[test]
    fn no_receipt_returns_err() {
        let tmp = tempdir().unwrap();
        unsafe { std::env::set_var("LOCALAPPDATA", tmp.path()) };
        let r = verify("nonexistent-xyz-package");
        unsafe { std::env::remove_var("LOCALAPPDATA") };
        assert!(r.is_err());
    }
}
