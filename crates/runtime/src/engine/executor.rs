use lodge_shared::placement::PlacementPlan;

/// Executes a resolved [`PlacementPlan`], streaming step results to the TUI.
///
/// Each placement step is reported live so the sequence screen can update.
pub fn execute(_plan: &PlacementPlan) -> anyhow::Result<()> {
    todo!()
}
