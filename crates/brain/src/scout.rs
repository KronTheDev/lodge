use std::collections::HashMap;
use std::net::TcpListener;

/// Structured result from a single probe execution.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub probe: &'static str,
    pub found: bool,
    pub value: Option<String>,
    pub raw: Option<String>,
    pub error: Option<String>,
}

/// Arguments passed to a probe function.
pub type ProbeArgs = HashMap<String, String>;

/// A registered system probe.
pub struct Probe {
    pub name: &'static str,
    /// Shown to the model in the system prompt so it knows which probes exist.
    pub description: &'static str,
    pub args: &'static [&'static str],
    pub run: fn(args: &ProbeArgs) -> ProbeResult,
}

/// Static dispatch table of all built-in probes.
pub static PROBES: &[Probe] = &[
    Probe {
        name: "ps_version",
        description: "PowerShell version installed on this machine",
        args: &[],
        run: probes::ps_version,
    },
    Probe {
        name: "dotnet_runtimes",
        description: ".NET runtime versions present",
        args: &[],
        run: probes::dotnet_runtimes,
    },
    Probe {
        name: "node_version",
        description: "Node.js version installed",
        args: &[],
        run: probes::node_version,
    },
    Probe {
        name: "python_version",
        description: "Python version installed",
        args: &[],
        run: probes::python_version,
    },
    Probe {
        name: "port_in_use",
        description: "Whether a TCP port is currently bound",
        args: &["port"],
        run: probes::port_in_use,
    },
    Probe {
        name: "service_status",
        description: "Whether a named service exists and is running",
        args: &["name"],
        run: probes::service_status,
    },
    Probe {
        name: "env_var",
        description: "Value of an environment variable",
        args: &["name"],
        run: probes::env_var,
    },
    Probe {
        name: "execution_policy",
        description: "PowerShell execution policy on this machine",
        args: &[],
        run: probes::execution_policy,
    },
    Probe {
        name: "disk_space",
        description: "Free disk space at a path",
        args: &["path"],
        run: probes::disk_space,
    },
    Probe {
        name: "os_build",
        description: "OS version and build number",
        args: &[],
        run: probes::os_build,
    },
    Probe {
        name: "process_running",
        description: "Whether a named process is currently active",
        args: &["name"],
        run: probes::process_running,
    },
    Probe {
        name: "path_exists",
        description: "Whether a path exists and what type it is",
        args: &["path"],
        run: probes::path_exists,
    },
    Probe {
        name: "path_writable",
        description: "Whether a path is writable by the current user",
        args: &["path"],
        run: probes::path_writable,
    },
    Probe {
        name: "arch",
        description: "CPU architecture of this machine",
        args: &[],
        run: probes::arch,
    },
];

/// Dispatches a probe by name with the given args.
pub fn dispatch(name: &str, args: &ProbeArgs) -> Option<ProbeResult> {
    PROBES
        .iter()
        .find(|p| p.name == name)
        .map(|p| (p.run)(args))
}

/// Builds a probe args map from key-value pairs.
pub fn args(pairs: &[(&str, &str)]) -> ProbeArgs {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

mod probes {
    use super::{ProbeArgs, ProbeResult, TcpListener};

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Run an external command and return trimmed stdout, or None on failure.
    fn run_cmd(program: &str, args: &[&str]) -> Option<String> {
        std::process::Command::new(program)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Run a PowerShell one-liner and return output, or None on failure.
    #[cfg(windows)]
    fn ps(script: &str) -> Option<String> {
        run_cmd(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", script],
        )
    }

    #[cfg(not(windows))]
    fn ps(script: &str) -> Option<String> {
        run_cmd(
            "pwsh",
            &["-NoProfile", "-NonInteractive", "-Command", script],
        )
    }

    fn ok(probe: &'static str, value: String, raw: Option<String>) -> ProbeResult {
        ProbeResult {
            probe,
            found: true,
            value: Some(value),
            raw,
            error: None,
        }
    }

    fn not_found(probe: &'static str) -> ProbeResult {
        ProbeResult {
            probe,
            found: false,
            value: None,
            raw: None,
            error: None,
        }
    }

    fn err(probe: &'static str, msg: String) -> ProbeResult {
        ProbeResult {
            probe,
            found: false,
            value: None,
            raw: None,
            error: Some(msg),
        }
    }

    // ── Probe implementations ─────────────────────────────────────────────────

    pub fn ps_version(_args: &ProbeArgs) -> ProbeResult {
        match ps("$PSVersionTable.PSVersion.ToString()") {
            Some(v) => ok("ps_version", v.clone(), Some(v)),
            None => not_found("ps_version"),
        }
    }

    pub fn dotnet_runtimes(_args: &ProbeArgs) -> ProbeResult {
        match run_cmd("dotnet", &["--list-runtimes"]) {
            Some(raw) => {
                let versions: Vec<&str> = raw
                    .lines()
                    .filter_map(|l| l.split_whitespace().nth(1))
                    .collect();
                let value = if versions.is_empty() {
                    return not_found("dotnet_runtimes");
                } else {
                    versions.join(", ")
                };
                ok("dotnet_runtimes", value, Some(raw))
            }
            None => not_found("dotnet_runtimes"),
        }
    }

    pub fn node_version(_args: &ProbeArgs) -> ProbeResult {
        match run_cmd("node", &["--version"]) {
            Some(v) => ok("node_version", v.clone(), Some(v)),
            None => not_found("node_version"),
        }
    }

    pub fn python_version(_args: &ProbeArgs) -> ProbeResult {
        // Try python, then python3
        let raw = run_cmd("python", &["--version"]).or_else(|| run_cmd("python3", &["--version"]));
        match raw {
            Some(v) => ok("python_version", v.clone(), Some(v)),
            None => not_found("python_version"),
        }
    }

    pub fn port_in_use(args: &ProbeArgs) -> ProbeResult {
        let port_str = match args.get("port") {
            Some(p) => p.as_str(),
            None => return err("port_in_use", "port argument required".into()),
        };
        let port: u16 = match port_str.parse() {
            Ok(p) => p,
            Err(_) => return err("port_in_use", format!("invalid port: {port_str}")),
        };
        // If we can bind, the port is free. If bind fails, it's in use.
        let in_use = TcpListener::bind(("0.0.0.0", port)).is_err();
        ProbeResult {
            probe: "port_in_use",
            found: in_use,
            value: Some(if in_use {
                "in use".into()
            } else {
                "free".into()
            }),
            raw: None,
            error: None,
        }
    }

    pub fn service_status(args: &ProbeArgs) -> ProbeResult {
        let name = match args.get("name") {
            Some(n) => n.clone(),
            None => return err("service_status", "name argument required".into()),
        };

        #[cfg(windows)]
        {
            match run_cmd("sc", &["query", &name]) {
                Some(raw) => {
                    let running = raw.contains("RUNNING");
                    ProbeResult {
                        probe: "service_status",
                        found: true,
                        value: Some(if running {
                            "running".into()
                        } else {
                            "stopped".into()
                        }),
                        raw: Some(raw),
                        error: None,
                    }
                }
                None => not_found("service_status"),
            }
        }

        #[cfg(not(windows))]
        {
            match run_cmd("systemctl", &["is-active", &name]) {
                Some(state) => ProbeResult {
                    probe: "service_status",
                    found: true,
                    value: Some(state.clone()),
                    raw: Some(state),
                    error: None,
                },
                None => not_found("service_status"),
            }
        }
    }

    pub fn env_var(args: &ProbeArgs) -> ProbeResult {
        let name = args.get("name").map(|s| s.as_str()).unwrap_or("");
        match std::env::var(name) {
            Ok(val) => ok("env_var", val.clone(), Some(val)),
            Err(_) => not_found("env_var"),
        }
    }

    pub fn execution_policy(_args: &ProbeArgs) -> ProbeResult {
        match ps("Get-ExecutionPolicy") {
            Some(policy) => ok("execution_policy", policy.clone(), Some(policy)),
            None => not_found("execution_policy"),
        }
    }

    pub fn disk_space(args: &ProbeArgs) -> ProbeResult {
        let path = args.get("path").map(|s| s.as_str()).unwrap_or(".");

        #[cfg(windows)]
        {
            let script = format!(
                "$d = Get-Item '{}' -ErrorAction SilentlyContinue; \
                 if ($d) {{ $drive = Split-Path -Qualifier $d.FullName; \
                 (Get-PSDrive ($drive.TrimEnd(':'))).Free }} else {{ 'not found' }}",
                path
            );
            match ps(&script) {
                Some(raw) => {
                    if raw == "not found" {
                        return not_found("disk_space");
                    }
                    let bytes: u64 = raw.parse().unwrap_or(0);
                    let human = format_bytes(bytes);
                    ok("disk_space", human, Some(raw))
                }
                None => not_found("disk_space"),
            }
        }

        #[cfg(not(windows))]
        {
            match run_cmd("df", &["-k", "--output=avail", path]) {
                Some(raw) => {
                    let kb: u64 = raw
                        .lines()
                        .nth(1)
                        .and_then(|l| l.trim().parse().ok())
                        .unwrap_or(0);
                    let human = format_bytes(kb * 1024);
                    ok("disk_space", human, Some(raw))
                }
                None => not_found("disk_space"),
            }
        }
    }

    fn format_bytes(bytes: u64) -> String {
        const GB: u64 = 1_073_741_824;
        const MB: u64 = 1_048_576;
        if bytes >= GB {
            format!("{:.1} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.0} MB", bytes as f64 / MB as f64)
        } else {
            format!("{bytes} bytes")
        }
    }

    pub fn os_build(_args: &ProbeArgs) -> ProbeResult {
        #[cfg(windows)]
        {
            match ps("[System.Environment]::OSVersion.VersionString") {
                Some(v) => ok("os_build", v.clone(), Some(v)),
                None => not_found("os_build"),
            }
        }

        #[cfg(target_os = "macos")]
        {
            match run_cmd("sw_vers", &[]) {
                Some(raw) => {
                    let v = raw
                        .lines()
                        .find(|l| l.starts_with("ProductVersion"))
                        .and_then(|l| l.split(':').nth(1))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| raw.clone());
                    ok("os_build", v, Some(raw))
                }
                None => not_found("os_build"),
            }
        }

        #[cfg(target_os = "linux")]
        {
            match run_cmd("uname", &["-sr"]) {
                Some(v) => ok("os_build", v.clone(), Some(v)),
                None => not_found("os_build"),
            }
        }

        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        not_found("os_build")
    }

    pub fn process_running(args: &ProbeArgs) -> ProbeResult {
        let name = match args.get("name") {
            Some(n) => n.clone(),
            None => return err("process_running", "name argument required".into()),
        };

        #[cfg(windows)]
        {
            let raw = run_cmd("tasklist", &["/FI", &format!("IMAGENAME eq {name}"), "/NH"]);
            let running = raw
                .as_deref()
                .map(|r| r.to_lowercase().contains(&name.to_lowercase()))
                .unwrap_or(false);
            ProbeResult {
                probe: "process_running",
                found: running,
                value: Some(if running {
                    "running".into()
                } else {
                    "not found".into()
                }),
                raw,
                error: None,
            }
        }

        #[cfg(not(windows))]
        {
            let running = run_cmd("pgrep", &["-x", &name]).is_some();
            ProbeResult {
                probe: "process_running",
                found: running,
                value: Some(if running {
                    "running".into()
                } else {
                    "not found".into()
                }),
                raw: None,
                error: None,
            }
        }
    }

    pub fn path_exists(args: &ProbeArgs) -> ProbeResult {
        let path = args.get("path").map(|s| s.as_str()).unwrap_or("");
        let meta = std::fs::metadata(path);
        ProbeResult {
            probe: "path_exists",
            found: meta.is_ok(),
            value: meta.ok().map(|m| {
                if m.is_dir() {
                    "directory".into()
                } else {
                    "file".into()
                }
            }),
            raw: None,
            error: None,
        }
    }

    pub fn path_writable(args: &ProbeArgs) -> ProbeResult {
        let path = args.get("path").map(|s| s.as_str()).unwrap_or("");
        let writable = std::fs::metadata(path)
            .map(|m| !m.permissions().readonly())
            .unwrap_or(false);
        ProbeResult {
            probe: "path_writable",
            found: writable,
            value: Some(if writable {
                "writable".into()
            } else {
                "not writable".into()
            }),
            raw: None,
            error: None,
        }
    }

    pub fn arch(_args: &ProbeArgs) -> ProbeResult {
        ProbeResult {
            probe: "arch",
            found: true,
            value: Some(std::env::consts::ARCH.to_string()),
            raw: None,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arch_always_returns_value() {
        let result = dispatch("arch", &ProbeArgs::new()).unwrap();
        assert!(result.found);
        assert!(result.value.is_some());
    }

    #[test]
    fn env_var_finds_path() {
        let result = dispatch("env_var", &args(&[("name", "PATH")])).unwrap();
        assert!(result.found);
    }

    #[test]
    fn env_var_missing_returns_not_found() {
        let result = dispatch("env_var", &args(&[("name", "LODGE_TEST_VAR_MISSING_XYZ")])).unwrap();
        assert!(!result.found);
    }

    #[test]
    fn port_in_use_detects_free_port() {
        // Port 0 is always free (OS assigns ephemeral)
        // Use a known-free high port instead
        let result = dispatch("port_in_use", &args(&[("port", "49999")]));
        assert!(result.is_some());
        // Result depends on whether port is actually in use — just check it runs
        assert!(result.unwrap().value.is_some());
    }

    #[test]
    fn path_exists_for_temp() {
        let tmp = std::env::temp_dir();
        let result = dispatch(
            "path_exists",
            &args(&[("path", tmp.to_string_lossy().as_ref())]),
        )
        .unwrap();
        assert!(result.found);
        assert_eq!(result.value.as_deref(), Some("directory"));
    }

    #[test]
    fn path_exists_missing() {
        let result = dispatch("path_exists", &args(&[("path", "/nonexistent/path/xyz")])).unwrap();
        assert!(!result.found);
    }

    #[test]
    fn unknown_probe_returns_none() {
        let result = dispatch("does_not_exist", &ProbeArgs::new());
        assert!(result.is_none());
    }

    #[test]
    fn all_probes_have_unique_names() {
        let mut names = std::collections::HashSet::new();
        for probe in PROBES {
            assert!(
                names.insert(probe.name),
                "duplicate probe name: {}",
                probe.name
            );
        }
    }

    #[test]
    fn all_probes_have_descriptions() {
        for probe in PROBES {
            assert!(
                !probe.description.is_empty(),
                "probe {} has empty description",
                probe.name
            );
        }
    }
}
