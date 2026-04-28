use std::collections::HashMap;

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
    PROBES.iter().find(|p| p.name == name).map(|p| (p.run)(args))
}

mod probes {
    use super::{ProbeArgs, ProbeResult};

    pub fn ps_version(_args: &ProbeArgs) -> ProbeResult {
        todo!()
    }

    pub fn dotnet_runtimes(_args: &ProbeArgs) -> ProbeResult {
        todo!()
    }

    pub fn node_version(_args: &ProbeArgs) -> ProbeResult {
        todo!()
    }

    pub fn python_version(_args: &ProbeArgs) -> ProbeResult {
        todo!()
    }

    pub fn port_in_use(_args: &ProbeArgs) -> ProbeResult {
        todo!()
    }

    pub fn service_status(_args: &ProbeArgs) -> ProbeResult {
        todo!()
    }

    pub fn env_var(args: &ProbeArgs) -> ProbeResult {
        let name = args.get("name").map(|s| s.as_str()).unwrap_or("");
        match std::env::var(name) {
            Ok(val) => ProbeResult {
                probe: "env_var",
                found: true,
                value: Some(val),
                raw: None,
                error: None,
            },
            Err(_) => ProbeResult {
                probe: "env_var",
                found: false,
                value: None,
                raw: None,
                error: None,
            },
        }
    }

    pub fn execution_policy(_args: &ProbeArgs) -> ProbeResult {
        todo!()
    }

    pub fn disk_space(_args: &ProbeArgs) -> ProbeResult {
        todo!()
    }

    pub fn os_build(_args: &ProbeArgs) -> ProbeResult {
        todo!()
    }

    pub fn process_running(_args: &ProbeArgs) -> ProbeResult {
        todo!()
    }

    pub fn path_exists(args: &ProbeArgs) -> ProbeResult {
        let path = args.get("path").map(|s| s.as_str()).unwrap_or("");
        let meta = std::fs::metadata(path);
        ProbeResult {
            probe: "path_exists",
            found: meta.is_ok(),
            value: meta.ok().map(|m| if m.is_dir() { "directory".into() } else { "file".into() }),
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
            value: None,
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
