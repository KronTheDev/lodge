//! File system scanner — walks, scores, and classifies entries.

pub mod ai_scorer;
pub mod heuristics;
pub mod receipt_guard;
pub mod walker;

#[allow(unused_imports)]
pub use walker::{walk, walk_with_progress};
