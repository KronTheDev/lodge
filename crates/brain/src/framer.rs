use crate::scout::ProbeResult;

/// Static help text for the command bar.
pub const HELP: &str = "\
commands:
  install <path>      install a package from a local path
  uninstall <id>      remove an installed package
  update <id>         update a package
  list                show installed packages
  info <id>           show package details
  verify <id>         check installation integrity
  history             show installation history
  help                show this message

system questions (just ask naturally):
  do I have node installed?
  is port 8080 free?
  what powershell version am I on?
  how much disk space on C:?
  is my execution policy going to be a problem?
  what OS build am I running?";

/// Renders a [`ProbeResult`] as calm, plain-language output.
pub fn frame_probe_result(probe: &str, result: &ProbeResult) -> String {
    match probe {
        "node_version" => {
            if result.found {
                format!("node {}.", result.value.as_deref().unwrap_or("(unknown version)"))
            } else {
                "node is not installed.".into()
            }
        }

        "ps_version" => {
            if result.found {
                format!("PowerShell {}.", result.value.as_deref().unwrap_or("(unknown version)"))
            } else {
                "PowerShell isn't installed, or isn't on PATH.".into()
            }
        }

        "dotnet_runtimes" => {
            if result.found {
                format!(".NET runtimes: {}.", result.value.as_deref().unwrap_or("none"))
            } else {
                ".NET isn't installed.".into()
            }
        }

        "python_version" => {
            if result.found {
                format!("{}.", result.value.as_deref().unwrap_or("Python (unknown version)"))
            } else {
                "Python isn't installed, or isn't on PATH.".into()
            }
        }

        "port_in_use" => {
            let port = ""; // will be caller's context — probe result has "in use" or "free"
            let _ = port;
            match result.value.as_deref() {
                Some("in use") => "that port is in use.".into(),
                Some("free") => "that port is free.".into(),
                _ => "couldn't determine port status.".into(),
            }
        }

        "service_status" => {
            if let Some(val) = &result.value {
                format!("service is {val}.")
            } else {
                "service not found.".into()
            }
        }

        "env_var" => {
            if result.found {
                format!("{}.", result.value.as_deref().unwrap_or("(empty)"))
            } else {
                "that variable isn't set.".into()
            }
        }

        "execution_policy" => {
            if result.found {
                let policy = result.value.as_deref().unwrap_or("unknown");
                let note = match policy.to_lowercase().as_str() {
                    "restricted" => " — scripts won't run. use Set-ExecutionPolicy RemoteSigned to allow signed scripts.",
                    "allsigned" => " — only signed scripts will run.",
                    "remotesigned" | "unrestricted" | "bypass" => " — scripts should run fine.",
                    _ => "",
                };
                format!("execution policy: {policy}{note}")
            } else {
                "couldn't read execution policy.".into()
            }
        }

        "disk_space" => {
            if result.found {
                format!("{} free.", result.value.as_deref().unwrap_or("unknown"))
            } else {
                "couldn't read disk space.".into()
            }
        }

        "os_build" => {
            if result.found {
                result.value.clone().unwrap_or_else(|| "OS version unknown.".into())
            } else {
                "couldn't read OS version.".into()
            }
        }

        "process_running" => {
            match result.value.as_deref() {
                Some("running") => "that process is running.".into(),
                Some("not found") => "that process isn't running.".into(),
                _ => "couldn't check process list.".into(),
            }
        }

        "path_exists" => {
            if result.found {
                format!("exists ({}).", result.value.as_deref().unwrap_or("unknown type"))
            } else {
                "nothing there.".into()
            }
        }

        "path_writable" => {
            match result.value.as_deref() {
                Some("writable") => "writable.".into(),
                Some("not writable") => "not writable.".into(),
                _ => "couldn't check permissions.".into(),
            }
        }

        "arch" => {
            result.value.clone().unwrap_or_else(|| "unknown architecture.".into())
        }

        _ => {
            if let Some(err) = &result.error {
                format!("probe error: {err}")
            } else if result.found {
                result.value.clone().unwrap_or_else(|| "found.".into())
            } else {
                "not found.".into()
            }
        }
    }
}

/// Frames an error with calm, direct plain language.
pub fn frame_error(error: &str, context: &str) -> String {
    if context.is_empty() {
        error.to_string()
    } else {
        format!("{context} — {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scout::ProbeResult;

    fn found(probe: &'static str, value: &str) -> ProbeResult {
        ProbeResult {
            probe,
            found: true,
            value: Some(value.into()),
            raw: None,
            error: None,
        }
    }

    fn not_found(probe: &'static str) -> ProbeResult {
        ProbeResult { probe, found: false, value: None, raw: None, error: None }
    }

    #[test]
    fn node_found() {
        let r = frame_probe_result("node_version", &found("node_version", "v20.11.0"));
        assert!(r.contains("v20.11.0"));
    }

    #[test]
    fn node_not_found() {
        let r = frame_probe_result("node_version", &not_found("node_version"));
        assert!(r.contains("not installed"));
    }

    #[test]
    fn port_in_use() {
        let r = frame_probe_result("port_in_use", &found("port_in_use", "in use"));
        assert!(r.contains("in use"));
    }

    #[test]
    fn port_free() {
        let r = frame_probe_result("port_in_use", &found("port_in_use", "free"));
        assert!(r.contains("free"));
    }

    #[test]
    fn execution_policy_restricted_gets_note() {
        let r = frame_probe_result("execution_policy", &found("execution_policy", "Restricted"));
        assert!(r.contains("Restricted"));
        assert!(r.contains("RemoteSigned"));
    }

    #[test]
    fn disk_space_formats_value() {
        let r = frame_probe_result("disk_space", &found("disk_space", "42.3 GB"));
        assert!(r.contains("42.3 GB"));
    }

    #[test]
    fn arch_returns_value() {
        let r = frame_probe_result("arch", &found("arch", "x86_64"));
        assert_eq!(r, "x86_64");
    }
}
