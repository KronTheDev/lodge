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

/// Ensures the shim directory is present in the user's PATH.
///
/// On Windows: writes the value to `HKCU\Environment\PATH` via the registry
/// and broadcasts a `WM_SETTINGCHANGE` so the change takes effect in new shells
/// without a reboot.
///
/// On Unix: appends a shell export line to `~/.profile` if not already present.
/// No-op if the directory is already on PATH.
pub fn ensure_shim_dir_on_path() -> Result<()> {
    let dir = shim_dir();
    let dir_str = dir.to_string_lossy().to_string();

    // Check if already on PATH
    let current_path = std::env::var("PATH").unwrap_or_default();
    if current_path.split(path_separator()).any(|p| p == dir_str) {
        return Ok(());
    }

    #[cfg(windows)]
    {
        add_to_user_path_windows(&dir_str)
            .context("couldn't add shim directory to user PATH")?;
    }

    #[cfg(not(windows))]
    {
        add_to_profile_unix(&dir_str)
            .context("couldn't add shim directory to PATH in ~/.profile")?;
    }

    Ok(())
}

#[cfg(windows)]
fn add_to_user_path_windows(dir: &str) -> Result<()> {
    // Read the current user PATH from the registry
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let env = hkcu
        .open_subkey_with_flags("Environment", winreg::enums::KEY_READ | winreg::enums::KEY_WRITE)
        .context("couldn't open HKCU\\Environment")?;

    let current: String = env.get_value("PATH").unwrap_or_default();

    if current.split(';').any(|p| p.eq_ignore_ascii_case(dir)) {
        return Ok(());
    }

    let new_path = if current.is_empty() {
        dir.to_string()
    } else {
        format!("{};{}", current, dir)
    };

    env.set_value("PATH", &new_path)
        .context("couldn't write PATH to registry")?;

    // Notify running shells of the change
    broadcast_settings_change_windows();

    Ok(())
}

#[cfg(windows)]
fn broadcast_settings_change_windows() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let env: Vec<u16> = OsStr::new("Environment\0").encode_wide().collect();
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageTimeoutW(
            windows_sys::Win32::UI::WindowsAndMessaging::HWND_BROADCAST,
            windows_sys::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE,
            0,
            env.as_ptr() as isize,
            windows_sys::Win32::UI::WindowsAndMessaging::SMTO_ABORTIFHUNG,
            1000,
            std::ptr::null_mut(),
        );
    }
}

#[cfg(not(windows))]
fn add_to_profile_unix(dir: &str) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let profile = std::path::PathBuf::from(home).join(".profile");

    let marker = format!("# lodge shim path\nexport PATH=\"{dir}:$PATH\"");

    let existing = std::fs::read_to_string(&profile).unwrap_or_default();
    if existing.contains(dir) {
        return Ok(());
    }

    let mut content = existing;
    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&marker);
    content.push('\n');

    std::fs::write(&profile, content)
        .with_context(|| format!("couldn't write to {:?}", profile))?;

    Ok(())
}

fn path_separator() -> char {
    if cfg!(windows) { ';' } else { ':' }
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
