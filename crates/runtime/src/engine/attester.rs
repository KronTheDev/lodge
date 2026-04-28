use lodge_shared::{manifest::Manifest, placement::PlacementPlan, receipt::Receipt};

/// Writes a signed execution receipt for a completed installation.
///
/// The `receipt_hash` field is SHA-256 of the serialised receipt minus the hash itself.
/// Receipt path: `%LOCALAPPDATA%\lodge\receipts\{id}-{version}-{timestamp}.json` (Windows)
///               `~/.local/share/lodge/receipts/{id}-{version}-{timestamp}.json` (Unix)
pub fn write_receipt(
    _manifest: &Manifest,
    _plan: &PlacementPlan,
    _runtime_version: &str,
) -> anyhow::Result<Receipt> {
    todo!()
}
