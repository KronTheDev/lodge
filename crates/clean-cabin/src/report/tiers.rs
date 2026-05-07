//! Tier classification — combines heuristic and AI scores.

use crate::scanner::ai_scorer::AiScore;
use crate::scanner::heuristics::{HeuristicScore, TierHint};

/// Final confidence tier assigned to a flagged file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Strong signal — suggest removal.
    ClearOut,
    /// Moderate signal — worth examining.
    WorthALook,
    /// No clear signal — user decides.
    YouDecide,
}

/// Classify a file into a final `Tier` by combining heuristic and AI hints.
///
/// `Keep` heuristic hints are not passed here — they are filtered before
/// reaching the report layer.
pub fn classify(h: &HeuristicScore, ai: Option<&AiScore>) -> Tier {
    match &h.tier_hint {
        TierHint::ClearOut => {
            // Demote only if AI explicitly says Keep; otherwise stay ClearOut.
            match ai.map(|a| &a.tier_hint) {
                Some(TierHint::Keep) => Tier::WorthALook,
                _ => Tier::ClearOut,
            }
        }
        TierHint::WorthALook => Tier::WorthALook,
        TierHint::YouDecide => {
            // AI can promote to WorthALook but never to ClearOut by itself.
            match ai.map(|a| &a.tier_hint) {
                Some(TierHint::ClearOut) | Some(TierHint::WorthALook) => Tier::WorthALook,
                _ => Tier::YouDecide,
            }
        }
        TierHint::Keep => Tier::YouDecide, // shouldn't be passed, but safe fallback
    }
}
