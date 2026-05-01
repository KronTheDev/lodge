use lodge_shared::manifest::{Manifest, Scope};

/// The resolved installation scope and whether it was overridden from the package's preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeResolution {
    pub scope: Scope,
    /// `true` when the package preferred system scope but we fell back to user scope
    /// because elevation was unavailable and the package did not *require* it.
    pub fell_back: bool,
}

/// Resolves the effective installation scope from the manifest and runtime context.
///
/// Rules:
/// - If `requires.elevation = true` and elevation is unavailable → hard fail.
/// - If the package prefers `user` scope → always `User`, no fallback needed.
/// - If the package prefers `system` scope and elevation is available → `System`.
/// - If the package prefers `system` scope and elevation is **not** available:
///   - Fall back to `User` with `fell_back = true` (caller should warn).
///   - (`requires.elevation` already caught above, so this path is always soft.)
pub fn infer_scope(manifest: &Manifest, has_elevation: bool) -> anyhow::Result<ScopeResolution> {
    // Hard requirement: elevation absolutely needed but unavailable.
    if manifest.requires.elevation && !has_elevation {
        anyhow::bail!(
            "{} requires elevation to install, but elevation is unavailable. \
             try running as admin.",
            manifest.id
        )
    }

    let preferred = manifest.prefers.scope.as_ref().unwrap_or(&Scope::User);

    match preferred {
        Scope::User => Ok(ScopeResolution {
            scope: Scope::User,
            fell_back: false,
        }),

        Scope::System => {
            if has_elevation {
                Ok(ScopeResolution {
                    scope: Scope::System,
                    fell_back: false,
                })
            } else {
                // requires.elevation is false here (checked above), so soft fallback is safe.
                Ok(ScopeResolution {
                    scope: Scope::User,
                    fell_back: true,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lodge_shared::manifest::{Manifest, PackageType, Prefers, Requires, Scope};

    fn manifest_with(scope: Option<Scope>, requires_elevation: bool) -> Manifest {
        Manifest {
            id: "test".into(),
            version: "0.1.0".into(),
            package_type: PackageType::CliTool,
            description: None,
            author: None,
            prefers: Prefers {
                scope,
                elevation: requires_elevation,
                isolated: false,
            },
            requires: Requires {
                elevation: requires_elevation,
                ..Default::default()
            },
            naming: Default::default(),
            overrides: vec![],
            hooks: Default::default(),
        }
    }

    #[test]
    fn user_scope_always_resolves_user() {
        let m = manifest_with(Some(Scope::User), false);
        let r = infer_scope(&m, false).unwrap();
        assert_eq!(r.scope, Scope::User);
        assert!(!r.fell_back);
    }

    #[test]
    fn no_scope_preference_defaults_to_user() {
        let m = manifest_with(None, false);
        let r = infer_scope(&m, false).unwrap();
        assert_eq!(r.scope, Scope::User);
        assert!(!r.fell_back);
    }

    #[test]
    fn system_scope_with_elevation_resolves_system() {
        let m = manifest_with(Some(Scope::System), false);
        let r = infer_scope(&m, true).unwrap();
        assert_eq!(r.scope, Scope::System);
        assert!(!r.fell_back);
    }

    #[test]
    fn system_scope_without_elevation_falls_back_to_user() {
        let m = manifest_with(Some(Scope::System), false);
        let r = infer_scope(&m, false).unwrap();
        assert_eq!(r.scope, Scope::User);
        assert!(r.fell_back);
    }

    #[test]
    fn system_scope_requires_elevation_hard_fails() {
        let m = manifest_with(Some(Scope::System), true);
        let err = infer_scope(&m, false).unwrap_err();
        assert!(err.to_string().contains("elevation"));
    }
}
