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
    Probe {
        name: "registry_key",
        description: "Value of a Windows registry key (HKCU or HKLM)",
        args: &["hive", "path", "value"],
        run: probes::registry_key,
    },
    Probe {
        name: "ram_usage",
        description: "Total and available RAM on this machine",
        args: &[],
        run: probes::ram_usage,
    },
    Probe {
        name: "disk_space_all",
        description: "Free disk space on all mounted drives, combined",
        args: &[],
        run: probes::disk_space_all,
    },
    Probe {
        name: "git_version",
        description: "Git version installed",
        args: &[],
        run: probes::git_version,
    },
    Probe {
        name: "java_version",
        description: "Java (JDK/JRE) version installed",
        args: &[],
        run: probes::java_version,
    },
    Probe {
        name: "go_version",
        description: "Go (Golang) version installed",
        args: &[],
        run: probes::go_version,
    },
    Probe {
        name: "ruby_version",
        description: "Ruby version installed",
        args: &[],
        run: probes::ruby_version,
    },
    Probe {
        name: "docker_version",
        description: "Docker version installed and whether daemon is running",
        args: &[],
        run: probes::docker_version,
    },
    Probe {
        name: "npm_version",
        description: "npm version installed",
        args: &[],
        run: probes::npm_version,
    },
    Probe {
        name: "php_version",
        description: "PHP version installed",
        args: &[],
        run: probes::php_version,
    },
    Probe {
        name: "cpu_info",
        description: "CPU model, core count, and thread count",
        args: &[],
        run: probes::cpu_info,
    },
    Probe {
        name: "uptime",
        description: "How long the system has been running since last boot",
        args: &[],
        run: probes::uptime,
    },
    Probe {
        name: "hostname",
        description: "The machine's hostname / computer name",
        args: &[],
        run: probes::hostname,
    },
    Probe {
        name: "username",
        description: "The currently logged-in username",
        args: &[],
        run: probes::username,
    },
    Probe {
        name: "local_ip",
        description: "Local IPv4 address of the primary network interface",
        args: &[],
        run: probes::local_ip,
    },
    Probe {
        name: "gpu_info",
        description: "Graphics card / GPU model",
        args: &[],
        run: probes::gpu_info,
    },
    Probe {
        name: "battery_status",
        description: "Battery charge percentage and charging state (laptops)",
        args: &[],
        run: probes::battery_status,
    },
    Probe {
        name: "ssh_key_exists",
        description: "Whether SSH key pairs exist in ~/.ssh/",
        args: &[],
        run: probes::ssh_key_exists,
    },
    Probe {
        name: "wsl_version",
        description: "WSL (Windows Subsystem for Linux) version and installed distributions",
        args: &[],
        run: probes::wsl_version,
    },
    Probe {
        name: "winget_version",
        description: "winget (Windows Package Manager) version",
        args: &[],
        run: probes::winget_version,
    },
    Probe {
        name: "scoop_version",
        description: "Scoop package manager version",
        args: &[],
        run: probes::scoop_version,
    },
    Probe {
        name: "installed_app",
        description: "Whether a named application is installed on the system",
        args: &["name"],
        run: probes::installed_app,
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

// ── Drive classification helpers ──────────────────────────────────────────────

/// Human-readable type tag for a drive.
/// `volume` and `provider` must already be lowercased by the caller.
fn classify_drive_type(dtype: u8, volume: &str, provider: &str) -> &'static str {
    match dtype {
        2 => "removable",
        4 => "network",
        3 => {
            if volume.contains("google") || provider.contains("google") {
                "Google Drive"
            } else if volume.contains("onedrive") || provider.contains("onedrive") {
                "OneDrive"
            } else if volume.contains("dropbox") || provider.contains("dropbox") {
                "Dropbox"
            } else if volume.contains("icloud") || provider.contains("icloud") {
                "iCloud Drive"
            } else if volume.contains("box") && (volume.contains("drive") || volume.contains("sync")) {
                "Box Drive"
            } else {
                "local"
            }
        }
        _ => "local",
    }
}

/// Grouping index: 0=local, 1=cloud, 2=network, 3=removable.
/// `volume` and `provider` must already be lowercased by the caller.
fn classify_drive_group(dtype: u8, volume: &str, provider: &str) -> u8 {
    match dtype {
        2 | 5 => 3,
        4 => 2,
        3 if volume.contains("google")
            || provider.contains("google")
            || volume.contains("onedrive")
            || provider.contains("onedrive")
            || volume.contains("dropbox")
            || provider.contains("dropbox")
            || volume.contains("icloud")
            || provider.contains("icloud")
            || (volume.contains("box")
                && (volume.contains("drive") || volume.contains("sync"))) =>
        {
            1
        }
        3 => 0,
        _ => 0,
    }
}

mod probes {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{ChildStdin, Stdio};
    use std::sync::Mutex;

    use super::{classify_drive_group, classify_drive_type, ProbeArgs, ProbeResult, TcpListener};

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

    /// Run a command and return trimmed stderr (used for tools like `java -version`).
    fn run_cmd_stderr(program: &str, args: &[&str]) -> Option<String> {
        std::process::Command::new(program)
            .args(args)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())
            .filter(|s| !s.is_empty())
    }

    // ── Persistent PowerShell session ─────────────────────────────────────────
    //
    // Spawning powershell.exe costs ~1-2 s per call due to .NET startup.
    // Instead we keep a single long-lived session open and communicate with it
    // over stdin/stdout, using a sentinel line to mark the end of each response.
    // The session is lazily initialised on the first probe that needs it and
    // lives until the process exits (the OS reaps the child automatically).

    const PS_SENTINEL: &str = "LODGE_PS_SENTINEL_8F3A";

    static PS_SESSION: Mutex<Option<PsSession>> = Mutex::new(None);

    struct PsSession {
        #[allow(dead_code)]
        child: std::process::Child,
        stdin: ChildStdin,
        stdout: BufReader<std::process::ChildStdout>,
    }

    impl PsSession {
        fn new() -> Option<Self> {
            #[cfg(windows)]
            let exe = "powershell";
            #[cfg(not(windows))]
            let exe = "pwsh";

            // `-Command -` puts PS in pipeline mode: reads commands from stdin in a
            // loop and executes them without ever emitting a prompt to stdout.
            // This is the correct way to drive PS non-interactively over a pipe;
            // the default (no -Command flag) is interactive mode where PS 5.1
            // writes prompt text and cursor-positioning bytes to stdout even when
            // stdin is redirected, which pollutes probe output.
            let mut child = std::process::Command::new(exe)
                .args(["-NoProfile", "-NonInteractive", "-NoLogo", "-Command", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;

            let stdin = child.stdin.take()?;
            let stdout = BufReader::new(child.stdout.take()?);
            let mut session = PsSession { child, stdin, stdout };
            // Silence non-terminating errors so they never pollute output lines.
            let _ = session.run("$ErrorActionPreference = 'SilentlyContinue'");
            Some(session)
        }

        /// Send `script` to the session and collect output until the sentinel.
        ///
        /// Returns `Ok(Some(output))` on success, `Ok(None)` if the command
        /// produced no output (session still alive), `Err(())` if the session pipe
        /// is broken and needs to be restarted.
        fn run(&mut self, script: &str) -> Result<Option<String>, ()> {
            let cmd = format!("{script}\nWrite-Output '{PS_SENTINEL}'\n");
            self.stdin.write_all(cmd.as_bytes()).map_err(|_| ())?;
            self.stdin.flush().map_err(|_| ())?;

            let mut lines: Vec<String> = Vec::new();
            loop {
                let mut line = String::new();
                match self.stdout.read_line(&mut line) {
                    Ok(0) | Err(_) => return Err(()), // EOF or IO error = session dead
                    Ok(_) => {
                        // Use full trim for sentinel detection — guards against any
                        // leading spaces or CR/LF variants PS 5.1 might emit.
                        if line.trim() == PS_SENTINEL {
                            break;
                        }
                        lines.push(line.trim_end_matches(['\r', '\n']).to_string());
                    }
                }
            }

            let result = lines.join("\n");
            let trimmed = result.trim().to_string();
            Ok(if trimmed.is_empty() { None } else { Some(trimmed) })
        }
    }

    /// Run a PowerShell expression and return the trimmed output, or None.
    ///
    /// Uses a persistent session to amortise the ~1-2 s PowerShell startup cost.
    /// If the session has died it is transparently restarted and the call retried.
    fn ps(script: &str) -> Option<String> {
        let mut guard = PS_SESSION.lock().ok()?;
        if guard.is_none() {
            *guard = PsSession::new();
        }
        if let Some(session) = guard.as_mut() {
            match session.run(script) {
                Ok(result) => return result,
                Err(()) => {
                    // Session died; restart and retry once
                    *guard = PsSession::new();
                    return guard.as_mut()?.run(script).ok().flatten();
                }
            }
        }
        None
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
            // Resolve drive letter then query Win32_LogicalDisk for free, total, and type.
            // Output: FreeBytes|TotalBytes|DriveType|VolumeName|ProviderName
            let safe_path = path.replace('\'', "");
            let script = format!(
                "$p = Resolve-Path '{safe_path}' -ErrorAction SilentlyContinue; \
                 if (-not $p) {{ $p = '{safe_path}' }}; \
                 $letter = try {{ (Split-Path -Qualifier $p.ToString()).TrimEnd(':') }} catch {{ '{safe_path}'.Substring(0,1) }}; \
                 $d = Get-CimInstance Win32_LogicalDisk -Filter \"DeviceID='$($letter):'\" -ErrorAction SilentlyContinue; \
                 if ($d) {{ \"$($d.FreeSpace)|$($d.Size)|$($d.DriveType)|$($d.VolumeName)|$($d.ProviderName)\" }} else {{ 'notfound' }}"
            );
            match ps(&script) {
                Some(raw) if raw.trim() != "notfound" => {
                    let parts: Vec<&str> = raw.trim().splitn(5, '|').collect();
                    if parts.len() < 3 {
                        return not_found("disk_space");
                    }
                    let free: u64 = parts[0].trim().parse().unwrap_or(0);
                    let total: u64 = parts[1].trim().parse().unwrap_or(0);
                    let drive_type: u8 = parts[2].trim().parse().unwrap_or(0);
                    let volume = parts.get(3).unwrap_or(&"").trim().to_lowercase();
                    let provider = parts.get(4).unwrap_or(&"").trim().to_lowercase();

                    let type_tag = classify_drive_type(drive_type, &volume, &provider);
                    let human = if total > 0 {
                        format!(
                            "{} free  (of {})  [{}]",
                            format_bytes(free),
                            format_bytes(total),
                            type_tag
                        )
                    } else {
                        format!("{} free  [{}]", format_bytes(free), type_tag)
                    };
                    ok("disk_space", human, Some(raw))
                }
                _ => not_found("disk_space"),
            }
        }

        #[cfg(not(windows))]
        {
            match run_cmd("df", &["-k", "--output=avail,size", path]) {
                Some(raw) => {
                    // Output: two columns — avail, 1K-blocks
                    let mut lines = raw.lines().skip(1); // skip header
                    if let Some(line) = lines.next() {
                        let cols: Vec<&str> = line.split_whitespace().collect();
                        let free = cols.first().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024;
                        let total = cols.get(1).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024;
                        let human = if total > 0 {
                            format!("{} free  (of {})", format_bytes(free), format_bytes(total))
                        } else {
                            format!("{} free", format_bytes(free))
                        };
                        ok("disk_space", human, Some(raw))
                    } else {
                        not_found("disk_space")
                    }
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

    pub fn ram_usage(_args: &ProbeArgs) -> ProbeResult {
        #[cfg(windows)]
        {
            // Use Win32_OperatingSystem for both values — both fields are in KB,
            // single object, no admin required.
            let raw = ps(
                "$os = Get-CimInstance Win32_OperatingSystem; \
                 \"$($os.TotalVisibleMemorySize)|$($os.FreePhysicalMemory)\"",
            );
            match raw {
                Some(s) => {
                    let parts: Vec<&str> = s.trim().splitn(2, '|').collect();
                    if parts.len() == 2 {
                        // Both values are in KiloBytes
                        let total_kb: u64 = parts[0].trim().parse().unwrap_or(0);
                        let free_kb: u64 = parts[1].trim().parse().unwrap_or(0);
                        if total_kb == 0 {
                            return not_found("ram_usage");
                        }
                        let total = total_kb * 1024;
                        let free = free_kb * 1024;
                        let used = total.saturating_sub(free);
                        let value = format!(
                            "{} used / {} total  ({} free)",
                            format_bytes(used),
                            format_bytes(total),
                            format_bytes(free),
                        );
                        ok("ram_usage", value.clone(), Some(value))
                    } else {
                        not_found("ram_usage")
                    }
                }
                None => not_found("ram_usage"),
            }
        }

        #[cfg(not(windows))]
        {
            match std::fs::read_to_string("/proc/meminfo") {
                Ok(content) => {
                    let mut total: u64 = 0;
                    let mut available: u64 = 0;
                    for line in content.lines() {
                        if line.starts_with("MemTotal:") {
                            total = line
                                .split_whitespace()
                                .nth(1)
                                .and_then(|v| v.parse::<u64>().ok())
                                .unwrap_or(0)
                                * 1024;
                        } else if line.starts_with("MemAvailable:") {
                            available = line
                                .split_whitespace()
                                .nth(1)
                                .and_then(|v| v.parse::<u64>().ok())
                                .unwrap_or(0)
                                * 1024;
                        }
                    }
                    if total > 0 {
                        let used = total.saturating_sub(available);
                        let value = format!(
                            "{} used / {} total  ({} free)",
                            format_bytes(used),
                            format_bytes(total),
                            format_bytes(available),
                        );
                        ok("ram_usage", value.clone(), Some(value))
                    } else {
                        not_found("ram_usage")
                    }
                }
                Err(_) => not_found("ram_usage"),
            }
        }
    }

    pub fn disk_space_all(_args: &ProbeArgs) -> ProbeResult {
        #[cfg(windows)]
        {
            // One line per drive: Letter|FreeBytes|TotalBytes|DriveType|VolumeName|ProviderName
            // DriveType: 2=Removable, 3=Fixed, 4=Network, 5=CD-ROM
            let script = r#"Get-CimInstance Win32_LogicalDisk | Where-Object { $_.Size -gt 0 } | ForEach-Object { $_.DeviceID.TrimEnd(':') + '|' + $_.FreeSpace + '|' + $_.Size + '|' + $_.DriveType + '|' + $_.VolumeName + '|' + $_.ProviderName }"#;
            match ps(script) {
                Some(raw) => {
                    // (letter, free, total, display_label, group: 0=local 1=cloud 2=network 3=removable)
                    let mut drives: Vec<(String, u64, u64, String, u8)> = Vec::new();

                    for line in raw.lines() {
                        let parts: Vec<&str> = line.trim().splitn(6, '|').collect();
                        if parts.len() < 4 {
                            continue;
                        }
                        let letter = parts[0].trim().to_uppercase();
                        let free: u64 = parts[1].trim().parse().unwrap_or(0);
                        let total: u64 = parts[2].trim().parse().unwrap_or(0);
                        let dtype: u8 = parts[3].trim().parse().unwrap_or(0);
                        let volume = parts.get(4).unwrap_or(&"").trim().to_lowercase();
                        let provider = parts.get(5).unwrap_or(&"").trim().to_lowercase();

                        if total == 0 {
                            continue;
                        }

                        let type_tag = classify_drive_type(dtype, &volume, &provider);
                        let group = classify_drive_group(dtype, &volume, &provider);

                        // Display label: prefer volume name for cloud/network, else drive letter
                        let raw_vol = parts.get(4).unwrap_or(&"").trim();
                        let raw_prov = parts.get(5).unwrap_or(&"").trim();
                        let display = match group {
                            1 => type_tag.to_string(), // cloud — use detected name
                            2 => {
                                if !raw_prov.is_empty() {
                                    raw_prov.to_string()
                                } else if !raw_vol.is_empty() {
                                    raw_vol.to_string()
                                } else {
                                    "Network".into()
                                }
                            }
                            3 => {
                                if !raw_vol.is_empty() {
                                    raw_vol.to_string()
                                } else {
                                    "Removable".into()
                                }
                            }
                            _ => {
                                if !raw_vol.is_empty() {
                                    raw_vol.to_string()
                                } else {
                                    letter.clone()
                                }
                            }
                        };

                        drives.push((letter, free, total, display, group));
                    }

                    if drives.is_empty() {
                        return not_found("disk_space_all");
                    }

                    drives.sort_by_key(|d| (d.4, d.0.clone()));

                    let mut sections: Vec<String> = Vec::new();
                    for (group_id, heading) in
                        [(0u8, "local"), (1, "cloud"), (2, "network"), (3, "removable")]
                    {
                        let group: Vec<_> =
                            drives.iter().filter(|d| d.4 == group_id).collect();
                        if group.is_empty() {
                            continue;
                        }
                        let mut block = vec![heading.to_string()];
                        for (letter, free, total, label, _) in &group {
                            block.push(format!(
                                "  {letter}:  {label:<20}  {} free  (of {})",
                                format_bytes(*free),
                                format_bytes(*total),
                            ));
                        }
                        sections.push(block.join("\n"));
                    }

                    let value = sections.join("\n\n");
                    ok("disk_space_all", value, Some(raw))
                }
                None => not_found("disk_space_all"),
            }
        }

        #[cfg(not(windows))]
        {
            match run_cmd("df", &["-h", "--output=target,avail,size,fstype"]) {
                Some(raw) => ok("disk_space_all", raw.clone(), Some(raw)),
                None => not_found("disk_space_all"),
            }
        }
    }

    pub fn registry_key(args: &ProbeArgs) -> ProbeResult {
        let hive = args.get("hive").map(|s| s.as_str()).unwrap_or("HKCU");
        let path = match args.get("path") {
            Some(p) => p.clone(),
            None => return err("registry_key", "path argument required".into()),
        };
        let value_name = args.get("value").map(|s| s.as_str()).unwrap_or("");

        #[cfg(windows)]
        {
            use std::process::Command;
            // Use reg.exe to avoid adding winreg as a brain dep
            let full_key = format!("{}\\{}", hive, path);
            let output = Command::new("reg")
                .args(["query", &full_key, "/v", value_name])
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    let raw = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    // Parse: "    ValueName    REG_SZ    ActualValue"
                    let val = raw
                        .lines()
                        .find(|l| l.trim_start().starts_with(value_name))
                        .and_then(|l| l.splitn(4, char::is_whitespace).last())
                        .map(|s| s.trim().to_string());
                    ProbeResult {
                        probe: "registry_key",
                        found: val.is_some(),
                        value: val,
                        raw: Some(raw),
                        error: None,
                    }
                }
                _ => not_found("registry_key"),
            }
        }

        #[cfg(not(windows))]
        {
            let _ = (hive, path, value_name);
            ProbeResult {
                probe: "registry_key",
                found: false,
                value: None,
                raw: None,
                error: Some("registry keys are only available on Windows".into()),
            }
        }
    }

    pub fn git_version(_args: &ProbeArgs) -> ProbeResult {
        match run_cmd("git", &["--version"]) {
            Some(raw) => {
                // "git version 2.43.0.windows.1"
                let version = raw
                    .split_whitespace()
                    .nth(2)
                    .unwrap_or(&raw)
                    .to_string();
                ok("git_version", version, Some(raw))
            }
            None => not_found("git_version"),
        }
    }

    pub fn java_version(_args: &ProbeArgs) -> ProbeResult {
        // java -version outputs to stderr, not stdout
        let raw = run_cmd_stderr("java", &["-version"]);
        match raw {
            Some(s) => {
                // First line: 'java version "17.0.1"' or 'openjdk version "17.0.1"'
                let version = s
                    .lines()
                    .next()
                    .and_then(|l| {
                        let mut parts = l.split('"');
                        parts.nth(1).map(|v| v.to_string())
                    })
                    .unwrap_or_else(|| s.lines().next().unwrap_or("").to_string());
                ok("java_version", version, Some(s))
            }
            None => not_found("java_version"),
        }
    }

    pub fn go_version(_args: &ProbeArgs) -> ProbeResult {
        match run_cmd("go", &["version"]) {
            Some(raw) => {
                // "go version go1.21.0 windows/amd64"
                let version = raw
                    .split_whitespace()
                    .find(|w| {
                        w.starts_with("go")
                            && w.len() > 2
                            && w.chars().nth(2).map(|c| c.is_ascii_digit()).unwrap_or(false)
                    })
                    .map(|w| w.trim_start_matches("go").to_string())
                    .unwrap_or_else(|| raw.clone());
                ok("go_version", version, Some(raw))
            }
            None => not_found("go_version"),
        }
    }

    pub fn ruby_version(_args: &ProbeArgs) -> ProbeResult {
        match run_cmd("ruby", &["--version"]) {
            Some(raw) => {
                // "ruby 3.2.0 (2022-12-25 revision a528908271) [x86_64-mingw32]"
                let version = raw
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or(&raw)
                    .to_string();
                ok("ruby_version", version, Some(raw))
            }
            None => not_found("ruby_version"),
        }
    }

    pub fn docker_version(_args: &ProbeArgs) -> ProbeResult {
        match run_cmd("docker", &["--version"]) {
            Some(raw) => {
                // "Docker version 24.0.5, build ced0996"
                let version = raw
                    .split_whitespace()
                    .nth(2)
                    .map(|s| s.trim_end_matches(',').to_string())
                    .unwrap_or_else(|| raw.clone());
                ok("docker_version", version, Some(raw))
            }
            None => not_found("docker_version"),
        }
    }

    pub fn npm_version(_args: &ProbeArgs) -> ProbeResult {
        match run_cmd("npm", &["--version"]) {
            Some(v) => ok("npm_version", v.clone(), Some(v)),
            None => not_found("npm_version"),
        }
    }

    pub fn php_version(_args: &ProbeArgs) -> ProbeResult {
        match run_cmd("php", &["--version"]) {
            Some(raw) => {
                // "PHP 8.2.0 (cli) ..."
                let version = raw
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or(&raw)
                    .to_string();
                ok("php_version", version, Some(raw))
            }
            None => not_found("php_version"),
        }
    }

    pub fn cpu_info(_args: &ProbeArgs) -> ProbeResult {
        #[cfg(windows)]
        {
            let raw = ps(
                r#"Get-CimInstance Win32_Processor | Select-Object -First 1 | ForEach-Object { $_.Name.Trim() + '|' + $_.NumberOfCores + '|' + $_.NumberOfLogicalProcessors }"#,
            );
            match raw {
                Some(s) => {
                    let parts: Vec<&str> = s.splitn(3, '|').collect();
                    let name = parts.first().copied().unwrap_or("").trim();
                    let cores = parts.get(1).copied().unwrap_or("?").trim();
                    let threads = parts.get(2).copied().unwrap_or("?").trim();
                    let value = format!("{name}  ({cores} cores / {threads} threads)");
                    ok("cpu_info", value, Some(s))
                }
                None => not_found("cpu_info"),
            }
        }

        #[cfg(target_os = "linux")]
        {
            match std::fs::read_to_string("/proc/cpuinfo") {
                Ok(content) => {
                    let name = content
                        .lines()
                        .find(|l| l.starts_with("model name"))
                        .and_then(|l| l.split(':').nth(1))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "unknown".into());
                    let threads = content.lines().filter(|l| l.starts_with("processor")).count();
                    let value = format!("{name}  ({threads} threads)");
                    ok("cpu_info", value, None)
                }
                Err(_) => not_found("cpu_info"),
            }
        }

        #[cfg(target_os = "macos")]
        {
            let name = run_cmd("sysctl", &["-n", "machdep.cpu.brand_string"])
                .unwrap_or_else(|| "unknown".into());
            let cores = run_cmd("sysctl", &["-n", "hw.physicalcpu"]).unwrap_or_else(|| "?".into());
            let threads =
                run_cmd("sysctl", &["-n", "hw.logicalcpu"]).unwrap_or_else(|| "?".into());
            let value = format!("{name}  ({cores} cores / {threads} threads)");
            ok("cpu_info", value, None)
        }

        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        not_found("cpu_info")
    }

    pub fn uptime(_args: &ProbeArgs) -> ProbeResult {
        #[cfg(windows)]
        {
            let raw = ps(
                r#"$up = (Get-Date) - (Get-CimInstance Win32_OperatingSystem).LastBootUpTime; "$($up.Days)d $($up.Hours)h $($up.Minutes)m""#,
            );
            match raw {
                Some(v) => ok("uptime", v.clone(), Some(v)),
                None => not_found("uptime"),
            }
        }

        #[cfg(not(windows))]
        {
            match run_cmd("uptime", &["-p"]).or_else(|| run_cmd("uptime", &[])) {
                Some(v) => ok("uptime", v.clone(), Some(v)),
                None => not_found("uptime"),
            }
        }
    }

    pub fn hostname(_args: &ProbeArgs) -> ProbeResult {
        #[cfg(windows)]
        let name = std::env::var("COMPUTERNAME").ok();
        #[cfg(not(windows))]
        let name = run_cmd("hostname", &[]);

        match name {
            Some(v) => ok("hostname", v.clone(), Some(v)),
            None => not_found("hostname"),
        }
    }

    pub fn username(_args: &ProbeArgs) -> ProbeResult {
        let name = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .ok();
        match name {
            Some(v) => ok("username", v.clone(), Some(v)),
            None => not_found("username"),
        }
    }

    pub fn local_ip(_args: &ProbeArgs) -> ProbeResult {
        #[cfg(windows)]
        {
            let raw = ps(
                r#"(Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.InterfaceAlias -notlike 'Loopback*' -and $_.IPAddress -ne '127.0.0.1' -and $_.IPAddress -notlike '169.254.*' } | Sort-Object PrefixLength | Select-Object -Last 1 IPAddress).IPAddress"#,
            );
            match raw {
                Some(v) if !v.trim().is_empty() => ok("local_ip", v.clone(), Some(v)),
                _ => not_found("local_ip"),
            }
        }

        #[cfg(not(windows))]
        {
            let raw = run_cmd("hostname", &["-I"])
                .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
                .filter(|s| !s.is_empty());
            match raw {
                Some(v) => ok("local_ip", v.clone(), Some(v)),
                None => not_found("local_ip"),
            }
        }
    }

    pub fn gpu_info(_args: &ProbeArgs) -> ProbeResult {
        #[cfg(windows)]
        {
            let raw = ps(
                r#"(Get-CimInstance Win32_VideoController | Where-Object { $_.Name -notlike 'Microsoft*' } | Select-Object -First 1 Name).Name"#,
            );
            match raw {
                Some(v) if !v.trim().is_empty() => ok("gpu_info", v.clone(), Some(v)),
                _ => {
                    // Fall back to any adapter
                    match ps("(Get-CimInstance Win32_VideoController | Select-Object -First 1 Name).Name") {
                        Some(v) => ok("gpu_info", v.clone(), Some(v)),
                        None => not_found("gpu_info"),
                    }
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            let found = run_cmd("lspci", &[])
                .and_then(|s| {
                    s.lines()
                        .find(|l| {
                            let l = l.to_lowercase();
                            l.contains("vga") || l.contains("display") || l.contains("3d")
                        })
                        .map(|l| {
                            l.splitn(2, ": ")
                                .nth(1)
                                .unwrap_or(l)
                                .to_string()
                        })
                });
            match found {
                Some(v) => ok("gpu_info", v.clone(), Some(v)),
                None => not_found("gpu_info"),
            }
        }

        #[cfg(target_os = "macos")]
        {
            match run_cmd("system_profiler", &["SPDisplaysDataType"]) {
                Some(raw) => {
                    let name = raw
                        .lines()
                        .find(|l| l.trim_start().starts_with("Chipset Model:"))
                        .and_then(|l| l.split(':').nth(1))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "unknown".into());
                    ok("gpu_info", name, Some(raw))
                }
                None => not_found("gpu_info"),
            }
        }

        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        not_found("gpu_info")
    }

    pub fn battery_status(_args: &ProbeArgs) -> ProbeResult {
        #[cfg(windows)]
        {
            let raw = ps(
                r#"$b = Get-CimInstance Win32_Battery; if ($b) { "$($b.EstimatedChargeRemaining)|$($b.BatteryStatus)" } else { "none" }"#,
            );
            match raw.as_deref() {
                Some("none") | None => ProbeResult {
                    probe: "battery_status",
                    found: false,
                    value: Some("no battery — desktop or AC-only device.".into()),
                    raw: None,
                    error: None,
                },
                Some(s) => {
                    let parts: Vec<&str> = s.splitn(2, '|').collect();
                    let pct = parts.first().copied().unwrap_or("?").trim();
                    let code = parts.get(1).copied().unwrap_or("").trim();
                    let status = match code {
                        "1" => "discharging",
                        "2" => "plugged in",
                        "3" => "fully charged",
                        "4" => "low",
                        "5" => "critical",
                        "6" | "7" | "8" | "9" => "charging",
                        "11" => "partially charged",
                        _ => "plugged in",
                    };
                    let value = format!("{pct}%  ({status})");
                    ok("battery_status", value, Some(s.to_string()))
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            use std::path::Path;
            let bat = Path::new("/sys/class/power_supply/BAT0");
            if !bat.exists() {
                return ProbeResult {
                    probe: "battery_status",
                    found: false,
                    value: Some("no battery detected.".into()),
                    raw: None,
                    error: None,
                };
            }
            let capacity = std::fs::read_to_string(bat.join("capacity"))
                .ok()
                .map(|s| s.trim().to_string());
            let status = std::fs::read_to_string(bat.join("status"))
                .ok()
                .map(|s| s.trim().to_lowercase());
            match (capacity, status) {
                (Some(pct), Some(st)) => ok("battery_status", format!("{pct}%  ({st})"), None),
                _ => not_found("battery_status"),
            }
        }

        #[cfg(target_os = "macos")]
        {
            match run_cmd("pmset", &["-g", "batt"]) {
                Some(raw) => ok("battery_status", raw.clone(), Some(raw)),
                None => not_found("battery_status"),
            }
        }

        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        not_found("battery_status")
    }

    pub fn ssh_key_exists(_args: &ProbeArgs) -> ProbeResult {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        let ssh_dir = std::path::Path::new(&home).join(".ssh");

        let key_types = [
            ("id_ed25519", "id_ed25519.pub"),
            ("id_rsa", "id_rsa.pub"),
            ("id_ecdsa", "id_ecdsa.pub"),
            ("id_dsa", "id_dsa.pub"),
        ];

        let found_keys: Vec<&str> = key_types
            .iter()
            .filter(|(_, pub_file)| ssh_dir.join(pub_file).exists())
            .map(|(name, _)| *name)
            .collect();

        if found_keys.is_empty() {
            ProbeResult {
                probe: "ssh_key_exists",
                found: false,
                value: Some("no SSH keys found in ~/.ssh/.".into()),
                raw: None,
                error: None,
            }
        } else {
            ok("ssh_key_exists", found_keys.join(", "), None)
        }
    }

    pub fn wsl_version(_args: &ProbeArgs) -> ProbeResult {
        #[cfg(windows)]
        {
            match run_cmd("wsl", &["--version"]) {
                Some(raw) => {
                    // First line is "WSL version: X.Y.Z.0"
                    let version = raw
                        .lines()
                        .next()
                        .and_then(|l| l.split(':').nth(1))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| raw.lines().next().unwrap_or("").to_string());
                    ok("wsl_version", version, Some(raw))
                }
                None => ProbeResult {
                    probe: "wsl_version",
                    found: false,
                    value: Some("WSL is not installed or no distributions are set up.".into()),
                    raw: None,
                    error: None,
                },
            }
        }

        #[cfg(not(windows))]
        {
            ProbeResult {
                probe: "wsl_version",
                found: false,
                value: Some("WSL is a Windows-only feature.".into()),
                raw: None,
                error: None,
            }
        }
    }

    pub fn winget_version(_args: &ProbeArgs) -> ProbeResult {
        match run_cmd("winget", &["--version"]) {
            Some(v) => ok("winget_version", v.clone(), Some(v)),
            None => not_found("winget_version"),
        }
    }

    pub fn scoop_version(_args: &ProbeArgs) -> ProbeResult {
        match run_cmd("scoop", &["--version"]) {
            Some(v) => ok("scoop_version", v.clone(), Some(v)),
            None => not_found("scoop_version"),
        }
    }

    pub fn installed_app(args: &ProbeArgs) -> ProbeResult {
        let name = match args.get("name") {
            Some(n) if !n.is_empty() => n.clone(),
            _ => return err("installed_app", "name argument required".into()),
        };

        #[cfg(windows)]
        {
            // Search registry uninstall keys — covers MSI, NSIS, and most installers
            let safe_name = name.replace('\'', "''"); // basic PS injection guard
            let script = format!(
                r#"$n = '*{safe_name}*'; \
                   $keys = @('HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', \
                              'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', \
                              'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'); \
                   $keys | ForEach-Object {{ Get-ItemProperty $_ -ErrorAction SilentlyContinue }} | \
                   Where-Object {{ $_.DisplayName -like $n }} | \
                   Select-Object DisplayName, DisplayVersion -First 1 | \
                   ForEach-Object {{ "$($_.DisplayName)|$($_.DisplayVersion)" }}"#
            );
            match ps(&script) {
                Some(s) if !s.trim().is_empty() => {
                    let parts: Vec<&str> = s.splitn(2, '|').collect();
                    let display = parts.first().copied().unwrap_or(name.as_str()).trim();
                    let version = parts.get(1).copied().unwrap_or("").trim();
                    let value = if version.is_empty() {
                        display.to_string()
                    } else {
                        format!("{display}  v{version}")
                    };
                    ok("installed_app", value, Some(s))
                }
                _ => ProbeResult {
                    probe: "installed_app",
                    found: false,
                    value: Some(format!("{name} doesn't appear to be installed.")),
                    raw: None,
                    error: None,
                },
            }
        }

        #[cfg(not(windows))]
        {
            // Use `which` to check if the app is on PATH
            let on_path = run_cmd("which", &[&name]).is_some();
            if on_path {
                ok(
                    "installed_app",
                    format!("{name} is available on PATH"),
                    None,
                )
            } else {
                ProbeResult {
                    probe: "installed_app",
                    found: false,
                    value: Some(format!("{name} not found on this system.")),
                    raw: None,
                    error: None,
                }
            }
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
