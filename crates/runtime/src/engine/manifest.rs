use anyhow::{Context, Result};
use lodge_shared::manifest::Manifest;

/// Parse and validate a Lodge manifest from a JSON string.
///
/// Returns a fully validated [`Manifest`] or a descriptive error.
pub fn parse(json: &str) -> Result<Manifest> {
    let manifest: Manifest =
        serde_json::from_str(json).context("manifest is not valid JSON or has a schema error")?;
    validate(&manifest)?;
    Ok(manifest)
}

/// Validate a parsed manifest against Lodge's constraints.
fn validate(m: &Manifest) -> Result<()> {
    if m.id.is_empty() {
        anyhow::bail!("manifest.id is required and must not be empty");
    }
    if m.id.contains(|c: char| c.is_uppercase() || c == ' ') {
        anyhow::bail!(
            "manifest.id must be kebab-case (lowercase, hyphens only) — got {:?}",
            m.id
        );
    }
    if m.version.is_empty() {
        anyhow::bail!("manifest.version is required and must not be empty");
    }
    // Validate semver
    semver::Version::parse(&m.version)
        .with_context(|| format!("manifest.version {:?} is not valid semver", m.version))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lodge_shared::manifest::{PackageType, Scope};

    // ── Minimal valid manifest ────────────────────────────────────────────────

    #[test]
    fn minimal_manifest_parses() {
        let json = r#"{"id":"mytool","version":"1.0.0","type":"cli-tool"}"#;
        let m = parse(json).unwrap();
        assert_eq!(m.id, "mytool");
        assert_eq!(m.version, "1.0.0");
        assert!(matches!(m.package_type, PackageType::CliTool));
        assert!(m.description.is_none());
        assert!(m.author.is_none());
        assert!(m.overrides.is_empty());
    }

    // ── All optional fields populated ────────────────────────────────────────

    #[test]
    fn full_manifest_parses() {
        let json = r#"{
            "id": "my-tool",
            "version": "2.3.1",
            "type": "app",
            "description": "A GUI application.",
            "author": "andrew",
            "prefers": { "scope": "system", "elevation": true, "isolated": false },
            "requires": { "os": "windows", "os_version": "10.0.19041", "elevation": true },
            "as": { "command": "mt", "display_name": "My Tool" },
            "overrides": [{ "match": "*.cfg", "destination": "%APPDATA%\\mytool\\", "as": "config.cfg" }],
            "hooks": { "post_install": "scripts/post.ps1" }
        }"#;
        let m = parse(json).unwrap();
        assert_eq!(m.id, "my-tool");
        assert!(matches!(m.package_type, PackageType::App));
        assert_eq!(m.description.as_deref(), Some("A GUI application."));
        assert_eq!(m.author.as_deref(), Some("andrew"));
        assert!(matches!(m.prefers.scope, Some(Scope::System)));
        assert!(m.prefers.elevation);
        assert_eq!(m.requires.os.as_deref(), Some("windows"));
        assert!(m.requires.elevation);
        assert_eq!(m.naming.command.as_deref(), Some("mt"));
        assert_eq!(m.overrides.len(), 1);
        assert_eq!(m.overrides[0].pattern, "*.cfg");
        assert_eq!(m.hooks.post_install.as_deref(), Some("scripts/post.ps1"));
    }

    // ── Helper methods ────────────────────────────────────────────────────────

    #[test]
    fn command_name_falls_back_to_id() {
        let m = parse(r#"{"id":"lodge","version":"0.1.0","type":"cli-tool"}"#).unwrap();
        assert_eq!(m.command_name(), "lodge");
    }

    #[test]
    fn command_name_uses_as_command_when_set() {
        let m =
            parse(r#"{"id":"mytool","version":"1.0.0","type":"cli-tool","as":{"command":"mt"}}"#)
                .unwrap();
        assert_eq!(m.command_name(), "mt");
    }

    // ── Required field validation ─────────────────────────────────────────────

    #[test]
    fn missing_id_is_rejected() {
        let json = r#"{"version":"1.0.0","type":"cli-tool"}"#;
        assert!(parse(json).is_err());
    }

    #[test]
    fn empty_id_is_rejected() {
        let json = r#"{"id":"","version":"1.0.0","type":"cli-tool"}"#;
        let err = parse(json).unwrap_err();
        assert!(err.to_string().contains("id"));
    }

    #[test]
    fn missing_version_is_rejected() {
        let json = r#"{"id":"mytool","type":"cli-tool"}"#;
        assert!(parse(json).is_err());
    }

    #[test]
    fn empty_version_is_rejected() {
        let json = r#"{"id":"mytool","version":"","type":"cli-tool"}"#;
        let err = parse(json).unwrap_err();
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn missing_type_is_rejected() {
        let json = r#"{"id":"mytool","version":"1.0.0"}"#;
        assert!(parse(json).is_err());
    }

    #[test]
    fn invalid_semver_is_rejected() {
        let json = r#"{"id":"mytool","version":"not-semver","type":"cli-tool"}"#;
        let err = parse(json).unwrap_err();
        assert!(err.to_string().contains("semver"));
    }

    #[test]
    fn uppercase_id_is_rejected() {
        let json = r#"{"id":"MyTool","version":"1.0.0","type":"cli-tool"}"#;
        let err = parse(json).unwrap_err();
        assert!(err.to_string().contains("kebab-case"));
    }

    // ── Unknown fields ────────────────────────────────────────────────────────

    #[test]
    fn unknown_fields_are_ignored() {
        // serde's default deny_unknown_fields is NOT set — extra fields pass through
        let json =
            r#"{"id":"mytool","version":"1.0.0","type":"cli-tool","future_field":"whatever"}"#;
        assert!(parse(json).is_ok());
    }

    // ── All package types ─────────────────────────────────────────────────────

    #[test]
    fn all_package_types_parse() {
        let types = [
            ("cli-tool", PackageType::CliTool),
            ("ps-module", PackageType::PsModule),
            ("service", PackageType::Service),
            ("library", PackageType::Library),
            ("app", PackageType::App),
            ("config-pack", PackageType::ConfigPack),
            ("dev-tool", PackageType::DevTool),
            ("font", PackageType::Font),
        ];
        for (type_str, expected) in types {
            let json = format!(r#"{{"id":"x","version":"0.1.0","type":"{}"}}"#, type_str);
            let m = parse(&json).unwrap_or_else(|e| panic!("failed for {type_str}: {e}"));
            assert_eq!(
                std::mem::discriminant(&m.package_type),
                std::mem::discriminant(&expected)
            );
        }
    }
}
