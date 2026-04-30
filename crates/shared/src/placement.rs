use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A resolved source → destination pair for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementEntry {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub rename: Option<String>,
}

/// Side effects to register after file placement.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegistrationEffects {
    pub add_to_path: bool,
    pub env_var: Option<String>,
    pub service_name: Option<String>,
    pub start_menu_entry: bool,
}

/// The fully resolved plan for installing a package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementPlan {
    pub entries: Vec<PlacementEntry>,
    pub registrations: RegistrationEffects,
    pub hooks_order: Vec<String>,
    pub requires_elevation: bool,
}
