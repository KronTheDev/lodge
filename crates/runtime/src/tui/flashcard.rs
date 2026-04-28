#![allow(dead_code)]

use lodge_shared::{manifest::Manifest, placement::PlacementPlan};

/// Renders the pre-install flashcard screen.
///
/// Content is generated entirely from the manifest + resolved PlacementPlan.
/// The developer never authors flashcard text — the runtime generates it.
///
/// See CLAUDE.md §"The TUI / Screens / 1. Flashcard" for the visual spec.
pub fn render(_manifest: &Manifest, _plan: &PlacementPlan) -> anyhow::Result<()> {
    todo!()
}
