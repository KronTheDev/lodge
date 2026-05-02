use std::path::Path;

use anyhow::Result;
use lodge_shared::{manifest::PackageType, receipt::Receipt};

use super::{attester, executor, inference, manifest as manifest_parser, resolver};
use crate::shim;

/// Runs the full install pipeline (resolve → execute → receipt → shim) without a TUI.
///
/// Returns the written [`Receipt`] on success. Used by update and rollback,
/// which operate silently without an interactive terminal.
pub fn silent_install(pkg_path: &Path, runtime_version: &str) -> Result<Receipt> {
    let json = std::fs::read_to_string(pkg_path.join("lodge.json"))
        .map_err(|e| anyhow::anyhow!("couldn't read lodge.json in {:?}: {e}", pkg_path))?;

    let manifest = manifest_parser::parse(&json)?;
    let os = resolver::current_os();
    let plan = resolver::resolve(pkg_path, &manifest, os, false)?;

    executor::execute(&plan, pkg_path, &mut |_| {})?;

    let scope = inference::infer_scope(&manifest, false)?.scope;
    let receipt = attester::write_receipt(&manifest, &plan, &scope, vec![], runtime_version)?;

    if matches!(manifest.package_type, PackageType::CliTool) {
        if let Some(first_entry) = plan.entries.first() {
            shim::register::register(manifest.command_name(), &first_entry.destination)?;
        }
        let _ = shim::register::ensure_shim_dir_on_path();
    }

    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_minimal_pkg(dir: &std::path::Path) {
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(
            dir.join("lodge.json"),
            r#"{"id":"silentpkg","version":"1.0.0","type":"cli-tool"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("bin").join("silentpkg.exe"), b"MZ").unwrap();
    }

    #[test]
    fn silent_install_missing_manifest_errors() {
        let dir = tempdir().unwrap();
        let result = silent_install(dir.path(), "0.1.0");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("lodge.json"));
    }

    #[test]
    fn silent_install_returns_receipt() {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _lock = LOCK.lock().unwrap();

        let pkg = tempdir().unwrap();
        make_minimal_pkg(pkg.path());

        let receipt_root = tempdir().unwrap();
        let original = std::env::var("LOCALAPPDATA").ok();
        unsafe { std::env::set_var("LOCALAPPDATA", receipt_root.path()) };

        let result = silent_install(pkg.path(), "0.1.0");

        unsafe {
            match &original {
                Some(v) => std::env::set_var("LOCALAPPDATA", v),
                None => std::env::remove_var("LOCALAPPDATA"),
            }
        }

        let receipt = result.unwrap();
        assert_eq!(receipt.id, "silentpkg");
        assert_eq!(receipt.version, "1.0.0");
    }
}
