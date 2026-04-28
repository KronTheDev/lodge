use std::path::Path;
use lodge_shared::manifest::{Manifest, Scope};

/// Infers the effective installation scope from the manifest and runtime context.
///
/// Falls back to `user` if `system` scope is requested but elevation is unavailable,
/// unless `requires.elevation = true`, in which case the caller must hard-fail.
pub fn infer_scope(manifest: &Manifest, has_elevation: bool) -> anyhow::Result<Scope> {
    let _ = (manifest, has_elevation);
    todo!()
}

/// Infers the destination path for a single file when no override is declared.
pub fn infer_destination(
    _file: &Path,
    _manifest: &Manifest,
    _scope: &Scope,
    _os: &str,
) -> anyhow::Result<std::path::PathBuf> {
    todo!()
}
