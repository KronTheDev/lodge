//! Selection state for the interactive report TUI.

use crate::report::tiers::Tier;
use crate::scanner::walker::FileEntry;

/// A file that has been flagged and is potentially selectable for staging.
#[derive(Debug, Clone)]
pub struct FlaggedFile {
    /// The underlying file entry from the walker.
    pub entry: FileEntry,
    /// Confidence tier.
    pub tier: Tier,
    /// Combined reason string (heuristic + AI).
    pub reason: String,
    /// True if this file is protected by a Lodge receipt.
    pub guarded: bool,
    /// True if the file is currently selected for staging.
    pub selected: bool,
}

/// Interactive selection state for the report TUI.
pub struct SelectionState {
    /// All flagged files, sorted by tier then path.
    pub files: Vec<FlaggedFile>,
    /// Index of the currently focused file in the visible list.
    pub cursor: usize,
    /// Which tier section the cursor is active in.
    pub tier_focus: Tier,
    /// Whether the YouDecide section is expanded (files shown).
    pub you_decide_expanded: bool,
    /// If `Some(idx)`, a detail panel is shown for the file at visible index `idx`.
    pub detail_file: Option<usize>,
}

impl SelectionState {
    /// Create a new selection state from a list of flagged files.
    ///
    /// ClearOut non-guarded files start pre-selected.
    /// Files are sorted by tier then path.
    pub fn new(mut files: Vec<FlaggedFile>) -> Self {
        files.sort_by(|a, b| {
            a.tier
                .cmp(&b.tier)
                .then_with(|| a.entry.path.cmp(&b.entry.path))
        });

        // Auto-select ClearOut, non-guarded files.
        for f in &mut files {
            if f.tier == Tier::ClearOut && !f.guarded {
                f.selected = true;
            }
        }

        // Default tier_focus to the first tier present, or ClearOut.
        let tier_focus = files
            .first()
            .map(|f| f.tier)
            .unwrap_or(Tier::ClearOut);

        Self {
            files,
            cursor: 0,
            tier_focus,
            you_decide_expanded: false,
            detail_file: None,
        }
    }

    /// Toggle selection on the currently focused file (no-op for guarded files).
    pub fn toggle_current(&mut self) {
        if let Some(f) = self.files.get_mut(self.cursor) {
            if !f.guarded {
                f.selected = !f.selected;
            }
        }
    }

    /// Select all non-guarded files in `tier`.
    pub fn select_all_in_tier(&mut self, tier: Tier) {
        for f in &mut self.files {
            if f.tier == tier && !f.guarded {
                f.selected = true;
            }
        }
    }

    /// Deselect all files in `tier`.
    pub fn deselect_all_in_tier(&mut self, tier: Tier) {
        for f in &mut self.files {
            if f.tier == tier {
                f.selected = false;
            }
        }
    }

    /// Move the cursor up, staying within the visible (non-YouDecide or expanded) range.
    pub fn move_up(&mut self) {
        let visible = self.visible_count();
        if visible == 0 {
            return;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        self.sync_tier_focus();
    }

    /// Move the cursor down, staying within the visible range.
    pub fn move_down(&mut self) {
        let visible = self.visible_count();
        if visible == 0 {
            return;
        }
        if self.cursor + 1 < visible {
            self.cursor += 1;
        }
        self.sync_tier_focus();
    }

    /// Cycle the tier_focus through available tiers.
    pub fn cycle_tier(&mut self) {
        self.tier_focus = match self.tier_focus {
            Tier::ClearOut => Tier::WorthALook,
            Tier::WorthALook => Tier::YouDecide,
            Tier::YouDecide => Tier::ClearOut,
        };
        // Move cursor to the first file in the newly focused tier.
        if let Some(pos) = self
            .visible_files()
            .iter()
            .position(|(_, f)| f.tier == self.tier_focus)
        {
            self.cursor = pos;
        }
    }

    /// Files currently selected for staging.
    pub fn selected_files(&self) -> Vec<&FlaggedFile> {
        self.files.iter().filter(|f| f.selected).collect()
    }

    /// Total byte size of selected files.
    pub fn selected_size(&self) -> u64 {
        self.files
            .iter()
            .filter(|f| f.selected)
            .map(|f| f.entry.size)
            .sum()
    }

    /// Count of files currently visible in the list (respects you_decide_expanded).
    pub fn visible_count(&self) -> usize {
        self.visible_files().len()
    }

    /// Files visible in the list with their original index.
    pub fn visible_files(&self) -> Vec<(usize, &FlaggedFile)> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.tier != Tier::YouDecide || self.you_decide_expanded)
            .collect()
    }

    /// Sync `tier_focus` with the file at the current cursor position.
    fn sync_tier_focus(&mut self) {
        let visible = self.visible_files();
        if let Some((_, f)) = visible.get(self.cursor) {
            self.tier_focus = f.tier;
        }
    }

    /// Count of files in a given tier.
    pub fn tier_count(&self, tier: Tier) -> usize {
        self.files.iter().filter(|f| f.tier == tier).count()
    }

    /// Total size of files in a given tier.
    pub fn tier_size(&self, tier: Tier) -> u64 {
        self.files
            .iter()
            .filter(|f| f.tier == tier)
            .map(|f| f.entry.size)
            .sum()
    }
}
