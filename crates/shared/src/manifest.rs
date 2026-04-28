use serde::{Deserialize, Serialize};

/// Installation scope — restricts file destinations and registration side-effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    User,
    System,
}

/// The package type, which maps to a default placement strategy per OS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageType {
    CliTool,
    PsModule,
    Service,
    Library,
    App,
    ConfigPack,
    DevTool,
    Font,
}

/// Soft installation preferences expressed by the package.
///
/// These are hints, not requirements. Lodge may override them if the
/// system cannot satisfy them (e.g. elevation unavailable → fall back to user scope).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Prefers {
    /// Preferred scope. Defaults to `user` if absent.
    pub scope: Option<Scope>,
    /// Request admin elevation if needed. Default: false.
    #[serde(default)]
    pub elevation: bool,
    /// Install into its own isolated folder rather than shared paths. Default: false.
    #[serde(default)]
    pub isolated: bool,
}

/// Hard requirements checked before installation begins.
///
/// A failed `requires` check is a hard stop — Lodge will not proceed.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Requires {
    /// Target OS: `"windows"`, `"macos"`, or `"linux"`.
    pub os: Option<String>,
    /// Minimum OS version (semver). Example: `"10.0.19041"`.
    pub os_version: Option<String>,
    /// Whether admin elevation is mandatory (not just preferred).
    #[serde(default)]
    pub elevation: bool,
    /// Minimum PowerShell version required (Windows only).
    pub ps_version: Option<String>,
}

/// Naming and alias declarations for the installed package.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct As {
    /// CLI command name. Defaults to the package `id`.
    pub command: Option<String>,
    /// Environment variable name for the install path.
    pub env_var: Option<String>,
    /// Service or daemon name.
    pub service: Option<String>,
    /// Human-readable name for Start Menu / application lists.
    pub display_name: Option<String>,
}

/// An explicit placement override for files matching a glob pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Override {
    /// Glob pattern relative to the package root.
    #[serde(rename = "match")]
    pub pattern: String,
    /// Explicit destination path. Environment variables are expanded.
    pub destination: String,
    /// Rename the file on placement. Optional.
    #[serde(rename = "as")]
    pub rename: Option<String>,
}

/// Lifecycle hook scripts run around installation events.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Hooks {
    pub pre_install: Option<String>,
    pub post_install: Option<String>,
    pub pre_uninstall: Option<String>,
    pub post_uninstall: Option<String>,
}

/// The diegetic manifest describing what a package *is*.
///
/// Every field reads as the package narrating itself:
/// `"type": "cli-tool"` → "I am a CLI tool".
/// `"prefers": {"scope":"user"}` → "I'd rather install for the current user".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Unique identifier, kebab-case. Required.
    pub id: String,
    /// Semver version string. Required.
    pub version: String,
    /// Package type — governs default placement strategy. Required.
    #[serde(rename = "type")]
    pub package_type: PackageType,
    /// One-sentence description shown in the flashcard. Optional.
    pub description: Option<String>,
    /// Author string shown in the flashcard. Optional.
    pub author: Option<String>,
    /// Soft preferences. Optional — defaults applied if absent.
    #[serde(default)]
    pub prefers: Prefers,
    /// Hard requirements. Optional — if absent, no restrictions apply.
    #[serde(default)]
    pub requires: Requires,
    /// Naming declarations. Optional.
    #[serde(rename = "as", default)]
    pub naming: As,
    /// Explicit placement overrides. Optional.
    #[serde(default)]
    pub overrides: Vec<Override>,
    /// Lifecycle hook scripts. Optional.
    #[serde(default)]
    pub hooks: Hooks,
}

impl Manifest {
    /// Returns the effective CLI command name: `as.command` if set, otherwise `id`.
    pub fn command_name(&self) -> &str {
        self.naming.command.as_deref().unwrap_or(&self.id)
    }

    /// Returns the effective installation scope from `prefers.scope`, defaulting to `User`.
    pub fn preferred_scope(&self) -> &Scope {
        self.prefers.scope.as_ref().unwrap_or(&Scope::User)
    }
}
