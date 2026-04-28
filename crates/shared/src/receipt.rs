use serde::{Deserialize, Serialize};

/// A record of a single file placement that was executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacedFile {
    pub source: String,
    pub destination: String,
    pub hash: String,
}

/// A tamper-evident record of a completed installation.
///
/// The `receipt_hash` is SHA-256 of the entire receipt minus the hash field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
    pub version: String,
    pub installed_at: String,
    pub scope: String,
    pub placements: Vec<PlacedFile>,
    pub registrations: Vec<String>,
    pub hooks_run: Vec<String>,
    pub runtime_version: String,
    pub receipt_hash: String,
}
