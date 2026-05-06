//! Full system scan — runs a battery of probes concurrently and returns a
//! formatted snapshot, optionally narrated by the configured AI provider.

use std::thread;

use crate::{framer, scout};

/// (display_label, probe_name, arg_key_value_pairs)
type BatteryEntry = (&'static str, &'static str, &'static [(&'static str, &'static str)]);

// Probes that run unconditionally, in display order.
const BATTERY: &[BatteryEntry] = &[
    // System
    ("OS",       "os_build",      &[]),
    ("arch",     "arch",          &[]),
    ("CPU",      "cpu_info",      &[]),
    ("RAM",      "ram_usage",     &[]),
    ("GPU",      "gpu_info",      &[]),
    ("uptime",   "uptime",        &[]),
    // Identity
    ("machine",  "hostname",      &[]),
    ("user",     "username",      &[]),
    // Network
    ("local IP", "local_ip",      &[]),
    // Storage
    ("drives",   "disk_space_all",&[]),
    // Runtimes
    ("PS",       "ps_version",    &[]),
    (".NET",     "dotnet_runtimes",&[]),
    ("Node",     "node_version",  &[]),
    ("Python",   "python_version",&[]),
    ("Git",      "git_version",   &[]),
    ("Docker",   "docker_version",&[]),
    // State
    ("exec policy", "execution_policy", &[]),
    ("WSL",      "wsl_version",   &[]),
];

/// Run the full probe battery and return a plain-text snapshot.
///
/// Probes run concurrently (one thread per probe). The call blocks until all
/// complete. If an AI provider is configured, a narration paragraph is
/// appended after the table.
pub fn run() -> String {
    // Spawn one thread per probe, collect results in order
    let handles: Vec<_> = BATTERY
        .iter()
        .map(|(label, probe, arg_pairs)| {
            let label = label.to_string();
            let probe = probe.to_string();
            let args = scout::args(arg_pairs);
            thread::spawn(move || {
                let result = scout::dispatch(&probe, &args);
                let value = match result {
                    Some(r) if r.found => {
                        framer::frame_probe_result(&probe, &r)
                    }
                    Some(_) | None => "not found".to_string(),
                };
                (label, value)
            })
        })
        .collect();

    let rows: Vec<(String, String)> = handles
        .into_iter()
        .map(|h| h.join().unwrap_or_else(|_| ("?".into(), "probe failed".into())))
        .collect();

    format_table(&rows)
}

/// Run the battery and append AI narration if a provider is available.
pub fn run_with_narration() -> String {
    let table = run();

    let narration = crate::ai::narrate_scan(&table);
    if narration.is_empty() {
        table
    } else {
        format!("{table}\n\n{narration}")
    }
}

// ── Formatting ────────────────────────────────────────────────────────────────

fn format_table(rows: &[(String, String)]) -> String {
    // Label column width — widest label + 2 padding
    let label_w = rows.iter().map(|(l, _)| l.len()).max().unwrap_or(8) + 2;

    let mut out = String::new();

    // Group by category using blank rows as separators
    let categories: &[(&str, &[&str])] = &[
        ("system",   &["OS", "arch", "CPU", "RAM", "GPU", "uptime"]),
        ("identity", &["machine", "user"]),
        ("network",  &["local IP"]),
        ("storage",  &["drives"]),
        ("runtimes", &["PS", ".NET", "Node", "Python", "Git", "Docker"]),
        ("state",    &["exec policy", "WSL"]),
    ];

    for (heading, labels) in categories {
        let group: Vec<_> = rows
            .iter()
            .filter(|(l, _)| labels.contains(&l.as_str()))
            .collect();

        // Skip categories where everything is "not found"
        let any_found = group.iter().any(|(_, v)| v != "not found");
        if !any_found {
            continue;
        }

        out.push_str(&format!("  {heading}\n"));
        for (label, value) in &group {
            if value == "not found" {
                continue;
            }
            // Multi-line values (e.g. disk_space_all) indent continuation lines
            let mut first = true;
            for line in value.lines() {
                if first {
                    out.push_str(&format!(
                        "  {label:<width$}  {line}\n",
                        width = label_w
                    ));
                    first = false;
                } else {
                    out.push_str(&format!(
                        "  {:<width$}  {line}\n",
                        "",
                        width = label_w
                    ));
                }
            }
        }
        out.push('\n');
    }

    out.trim_end().to_string()
}
