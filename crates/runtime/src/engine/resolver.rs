use lodge_shared::{manifest::Manifest, placement::PlacementPlan};

/// Resolves a full [`PlacementPlan`] for `package_root` given its `manifest`.
///
/// See CLAUDE.md §"The Placement Resolution Algorithm" for the full five-step spec.
pub fn resolve(
    _package_root: &std::path::Path,
    _manifest: &Manifest,
    _os: &str,
    _has_elevation: bool,
) -> anyhow::Result<PlacementPlan> {
    todo!()
}
