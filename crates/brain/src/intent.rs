use serde::{Deserialize, Serialize};

/// The set of commands the intent resolver can return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Command {
    Install,
    Uninstall,
    Update,
    UpdateAll,
    Search,
    List,
    Info,
    Verify,
    Rollback,
    UpdateRulesets,
    Use,
    History,
    Help,
    Explore,
    Clarify,
}

/// Structured output from the intent resolver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub command: Command,
    pub args: serde_json::Value,
    pub confidence: f32,
    /// Set when `command == Clarify` — the question to ask the user.
    pub prompt: Option<String>,
}

/// Maps raw user input to a canonical [`Intent`].
///
/// Runs the input through the brain's system prompt + model, then parses
/// the returned JSON. Falls back to `Clarify` if confidence < 0.6.
pub fn resolve(_input: &str) -> anyhow::Result<Intent> {
    todo!()
}
