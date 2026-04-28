use std::path::PathBuf;

use anyhow::{Context, Result};

/// Expands a destination path template into a concrete [`PathBuf`].
///
/// Token replacement order:
/// 1. `{id}` → package ID string
/// 2. `~` prefix → home directory (`$HOME` on Unix, `%USERPROFILE%` on Windows)
/// 3. `%VARNAME%` → value of environment variable `VARNAME` (Windows-style, case-insensitive)
pub fn expand(template: &str, id: &str) -> Result<PathBuf> {
    let s = template.replace("{id}", id);
    let s = expand_tilde(&s)?;
    let s = expand_percent_vars(&s)?;
    Ok(PathBuf::from(s))
}

fn expand_tilde(s: &str) -> Result<String> {
    if s == "~" || s.starts_with("~/") || s.starts_with("~\\") {
        let home = home_dir().context("could not determine home directory for ~ expansion")?;
        let home_str = home.to_string_lossy();
        Ok(s.replacen('~', &home_str, 1))
    } else {
        Ok(s.to_string())
    }
}

fn expand_percent_vars(s: &str) -> Result<String> {
    if !s.contains('%') {
        return Ok(s.to_string());
    }

    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '%' {
            result.push(ch);
            continue;
        }
        // Collect until next '%'
        let mut var_name = String::new();
        let mut closed = false;
        for inner in chars.by_ref() {
            if inner == '%' {
                closed = true;
                break;
            }
            var_name.push(inner);
        }
        if !closed || var_name.is_empty() {
            // Lone % — pass through literally
            result.push('%');
            result.push_str(&var_name);
            continue;
        }
        let value = std::env::var(&var_name)
            .with_context(|| format!("environment variable {var_name:?} is not set"))?;
        result.push_str(&value);
    }

    Ok(result)
}

/// Returns the current user's home directory.
fn home_dir() -> Result<PathBuf> {
    if let Ok(h) = std::env::var("HOME") {
        return Ok(PathBuf::from(h));
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        return Ok(PathBuf::from(h));
    }
    anyhow::bail!("neither HOME nor USERPROFILE is set")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_token_is_replaced() {
        std::env::set_var("HOME", "/home/testuser");
        let p = expand("~/.config/{id}/", "mytool").unwrap();
        assert!(p.to_string_lossy().contains("mytool"));
    }

    #[test]
    fn tilde_expands_to_home() {
        std::env::set_var("HOME", "/home/testuser");
        let p = expand("~/.local/bin/", "x").unwrap();
        assert!(p.to_string_lossy().starts_with("/home/testuser"));
    }

    #[test]
    fn percent_var_expands() {
        std::env::set_var("TESTVAR_LODGE", "C:\\Test");
        let p = expand("%TESTVAR_LODGE%\\sub\\", "x").unwrap();
        assert!(p.to_string_lossy().contains("C:\\Test"));
    }

    #[test]
    fn missing_percent_var_errors() {
        std::env::remove_var("DEFINITELY_NOT_SET_LODGE_TEST");
        let err = expand("%DEFINITELY_NOT_SET_LODGE_TEST%\\foo", "x").unwrap_err();
        assert!(err.to_string().contains("DEFINITELY_NOT_SET_LODGE_TEST"));
    }

    #[test]
    fn no_tokens_passes_through() {
        let p = expand("/usr/local/bin/", "x").unwrap();
        assert_eq!(p, PathBuf::from("/usr/local/bin/"));
    }
}
