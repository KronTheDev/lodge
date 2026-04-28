use serde::{Deserialize, Serialize};

/// Destination paths for user vs system scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Destination {
    pub user: String,
    pub system: String,
}

/// OS registration side effects for a rule.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Register {
    #[serde(default)]
    pub path: bool,
    #[serde(default)]
    pub env_var: bool,
    #[serde(default)]
    pub service: bool,
    #[serde(default)]
    pub start_menu: bool,
}

/// A single placement rule.
///
/// Lower `priority` value = higher precedence when multiple rules match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub r#type: String,
    pub r#match: String,
    pub destination: Destination,
    #[serde(default)]
    pub register: Register,
    #[serde(default = "default_priority")]
    pub priority: u32,
}

fn default_priority() -> u32 {
    100
}
