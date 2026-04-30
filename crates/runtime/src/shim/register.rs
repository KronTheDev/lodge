use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Returns the shim directory managed by Lodge.
///
/// - Windows: `%LOCALAPPDATA%\Programs\lodge\shims\`
/// - Unix:    `~/.local/bin/` (assumed to be on PATH per XDG convention)
pub fn shim_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base)
            .join("Programs")
            .join("lodge")
            .join("shims")
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".local").join("bin")
    }
}

/// Registers a command shim for an installed CLI tool.
///
/// - Windows: writes a `.cmd` forwarding script in [`shim_dir`].
/// - Unix:    creates a symlink from [`shim_dir`] to `target`.
pub fn register(command_name: &str, target: &Path) -> Result<()> {
    let dir = shim_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("couldn't create shim directory {:?}", dir))?;

    #[cfg(windows)]
    {
        let shim = dir.join(format!("{}.cmd", command_name));
        let content = format!(
            "@echo off\r\n\"{target}\" %*\r\n",
            target = target.display()
        );
        std::fs::write(&shim, content)
            .with_context(|| format!("couldn't write shim {:?}", shim))?;
    }

    #[cfg(not(windows))]
    {
        let link = dir.join(command_name);
        if link.exists() || link.is_symlink() {
            std::fs::remove_file(&link)
                .with_context(|| format!("couldn't remove existing shim {:?}", link))?;
        }
        std::os::unix::fs::symlink(target, &link)
            .with_context(|| format!("couldn't create symlink {:?} → {:?}", link, target))?;
    }

    Ok(())
}

/// Removes the shim for `command_name`.
#[allow(dead_code)]
pub fn unregister(command_name: &str) -> Result<()> {
    let dir = shim_dir();

    #[cfg(windows)]
    let path = dir.join(format!("{}.cmd", command_name));
    #[cfg(not(windows))]
    let path = dir.join(command_name);

    if path.exists() || path.is_symlink() {
        std::fs::remove_file(&path).with_context(|| format!("couldn't remove shim {:?}", path))?;
    }
    Ok(())
}

/// Updates an existing shim to point at a new `target` (for `use <id>@<version>`).
#[allow(dead_code)]
pub fn update(command_name: &str, new_target: &Path) -> Result<()> {
    unregister(command_name)?;
    register(command_name, new_target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Override shim_dir for tests using a temp directory.
    fn test_register(command_name: &str, target: &Path, shim_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(shim_dir)?;

        #[cfg(windows)]
        {
            let shim = shim_dir.join(format!("{}.cmd", command_name));
            let content = format!(
                "@echo off\r\n\"{target}\" %*\r\n",
                target = target.display()
            );
            std::fs::write(&shim, content)?;
        }
        #[cfg(not(windows))]
        {
            let link = shim_dir.join(command_name);
            if link.exists() || link.is_symlink() {
                std::fs::remove_file(&link)?;
            }
            std::os::unix::fs::symlink(target, &link)?;
        }
        Ok(())
    }

    #[test]
    #[cfg(windows)]
    fn cmd_shim_contains_target_path() {
        let shim_dir = tempdir().unwrap();
        let target = Path::new("C:\\Programs\\mytool\\mt.exe");
        test_register("mt", target, shim_dir.path()).unwrap();

        let shim_path = shim_dir.path().join("mt.cmd");
        let content = std::fs::read_to_string(&shim_path).unwrap();
        assert!(content.contains("mt.exe"), "shim must reference the target");
        assert!(
            content.starts_with("@echo off"),
            "shim must start with @echo off"
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn unix_symlink_points_to_target() {
        let shim_dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let target = target_dir.path().join("lodge");
        std::fs::write(&target, b"").unwrap();

        test_register("lodge", &target, shim_dir.path()).unwrap();

        let link = shim_dir.path().join("lodge");
        assert!(link.is_symlink(), "shim must be a symlink");
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn shim_dir_is_non_empty_path() {
        let dir = shim_dir();
        assert!(dir.to_string_lossy().len() > 0);
    }
}
