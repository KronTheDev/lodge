# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> This file is the authoritative context document for Claude Code sessions on this project.
> Read it fully before touching any code, file, or configuration.

---

## Common Commands

```sh
# Build all crates
cargo build

# Build release binary
cargo build --release

# Run the runtime
cargo run -p lodge

# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p lodge-shared
cargo test -p lodge-ruleset
cargo test -p lodge-brain
cargo test -p lodge

# Lint (must pass with zero warnings before any code is considered done)
cargo clippy -- -D warnings

# Format
cargo fmt

# Regenerate splash art from assets/cabin.jpg (requires Pillow)
python3 scripts/gen_splash_art.py
```

---

## Project Identity

**Name:** Lodge
**Type:** Cross-platform installation runtime with conversational command interface
**Runtime target:** Windows primary, macOS and Linux secondary
**Implementation language:** Rust
**Model integration:** llama.cpp + SmolLM2-360M Q4_K_M (bundled, offline)
**Distribution target:** Single binary + bundled model, ~288MB total

### One-sentence pitch

> A language-agnostic installer runtime that reads a developer-shipped manifest,
> resolves file placements intelligently against the target OS, executes and
> displays each step live in a rich terminal UI, understands natural language
> commands via a fully offline bundled model, and can introspect the host
> machine to reason about compatibility — including systems and state entirely
> outside of what Lodge itself installed.

---

## Core Design Principles

These are non-negotiable constraints that govern every implementation decision:

1. **Offline-first, always.** No network calls during installation. No telemetry.
   No cloud dependency at any layer. The model, the ruleset, and the runtime
   all ship together.

2. **Zero mandatory configuration.** A package with no manifest at all should
   still install correctly via inference. The manifest is an override layer,
   not a required specification.

3. **Diegetic manifest.** The manifest describes what the package *is*, not
   instructions for the installer. It reads as the package narrating itself.

4. **Visible execution.** Every placement step renders live in the terminal.
   Nothing happens silently. Failures appear inline with context, not as
   generic error codes.

5. **Respect the target system.** Override-aware path resolution means the
   runtime adapts to the machine's preferences without breaking the package's
   intent. Never trample existing conventions.

6. **No Electron. No bloat.** The runtime is a Rust binary. The TUI is
   terminal-native. The total footprint including the bundled model is ~288MB —
   smaller than a single Electron Hello World plus a model.

---

## Repository Structure

```
project-root/
├── CLAUDE.md                  ← this file
├── Cargo.toml                 ← workspace root
├── Cargo.lock
│
├── crates/
│   ├── runtime/               ← core installation engine
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── engine/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── manifest.rs      ← manifest parsing + validation
│   │   │   │   ├── inference.rs     ← file type → placement inference
│   │   │   │   ├── resolver.rs      ← path resolution + override handling
│   │   │   │   ├── executor.rs      ← placement execution
│   │   │   │   └── attester.rs      ← execution receipt writing
│   │   │   ├── tui/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── flashcard.rs     ← pre-install summary screen
│   │   │   │   ├── sequence.rs      ← live installation sequence display
│   │   │   │   └── bar.rs           ← command bar UI
│   │   │   └── shim/
│   │   │       ├── mod.rs
│   │   │       └── register.rs      ← command shim registration
│   │   └── Cargo.toml
│   │
│   ├── brain/                 ← llama.cpp integration + intent layer
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── inference.rs         ← llama.cpp bindings wrapper
│   │   │   ├── intent.rs            ← input → canonical command resolution
│   │   │   ├── framer.rs            ← plain-language output framing
│   │   │   ├── context.rs           ← conversation state management
│   │   │   └── scout.rs             ← system introspection + compatibility reasoning
│   │   └── Cargo.toml
│   │
│   ├── ruleset/               ← OS placement ruleset engine
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── loader.rs            ← loads built-in + community rules
│   │   │   ├── matcher.rs           ← file signature → rule matching
│   │   │   └── types.rs             ← rule type definitions
│   │   ├── rules/
│   │   │   ├── windows.json         ← built-in Windows placement rules
│   │   │   ├── macos.json           ← built-in macOS placement rules
│   │   │   └── linux.json           ← built-in Linux placement rules
│   │   └── Cargo.toml
│   │
│   └── shared/                ← shared types across crates
│       ├── src/
│       │   ├── lib.rs
│       │   ├── manifest.rs          ← manifest schema types
│       │   ├── placement.rs         ← placement result types
│       │   └── receipt.rs           ← execution receipt types
│       └── Cargo.toml
│
├── model/
│   └── smollm2-360m-q4_k_m.gguf    ← bundled model (git-lfs or fetched at build)
│
├── assets/
│   └── cabin.jpg              ← source image for splash art generation
│
├── scripts/
│   └── gen_splash_art.py      ← regenerates crates/runtime/src/tui/splash.rs
│                                 from assets/cabin.jpg
│
│   ├── integration/
│   └── fixtures/
│       ├── packages/                ← sample packages for testing
│       └── manifests/               ← sample manifests
│
└── docs/
    ├── manifest-spec.md
    ├── ruleset-spec.md
    └── command-reference.md
```

---

## The Manifest Format

### Philosophy

The manifest is diegetic — it describes what the package *is*, not what
to do with it. Every field should pass the "narration test": read it aloud
as a sentence the package says about itself.

### Schema

```json
{
  "id": "string",           // required — unique identifier, kebab-case
  "version": "string",      // required — semver
  "type": "string",         // required — see Package Types below
  "description": "string",  // optional — one sentence, shown in flashcard
  "author": "string",       // optional — shown in flashcard

  "prefers": {              // optional — soft preferences
    "scope": "user|system", // default: user
    "elevation": false,     // default: false — request admin if needed
    "isolated": false       // default: false — own folder vs shared paths
  },

  "requires": {             // optional — hard requirements, checked pre-install
    "os": "windows|macos|linux",
    "os_version": "string", // semver min, e.g. "10.0.19041"
    "elevation": false,
    "ps_version": "string"  // PowerShell version min, Windows only
  },

  "as": {                   // optional — naming and alias declarations
    "command": "string",    // CLI command name (default: id)
    "env_var": "string",    // environment variable name for install path
    "service": "string",    // service/daemon name
    "display_name": "string"// human-readable name for Start Menu / app lists
  },

  "overrides": [            // optional — explicit placement overrides
    {
      "match": "string",    // glob pattern relative to package root
      "destination": "string", // explicit destination path (env vars expanded)
      "as": "string"        // rename the file on placement
    }
  ],

  "hooks": {                // optional — lifecycle scripts
    "pre_install": "string",  // path to script, relative to package root
    "post_install": "string",
    "pre_uninstall": "string",
    "post_uninstall": "string"
  }
}
```

### Minimal valid manifest

```json
{
  "id": "mytool",
  "version": "1.0.0",
  "type": "cli-tool"
}
```

### Narration test examples

```
"type": "cli-tool"          → "I am a CLI tool"
"prefers": {"scope":"user"} → "I'd rather install for the current user"
"requires": {"elevation":true} → "I need admin rights to install"
"as": {"command":"mt"}      → "Call me 'mt' on the command line"
```

---

## Package Types

Each type maps to a default placement strategy per OS. The ruleset engine
implements these. New types are added via community ruleset contributions.

### Built-in types

| Type | Description | Windows default | macOS default | Linux default |
|------|-------------|-----------------|---------------|---------------|
| `cli-tool` | Command-line executable | `%LOCALAPPDATA%\Programs\{id}\` or `Program Files\{id}\` | `~/.local/bin/` | `~/.local/bin/` |
| `ps-module` | PowerShell module | PS module path (user or system) | PS module path | PS module path |
| `service` | Background daemon | SCM registration | LaunchAgent/LaunchDaemon | systemd user/system |
| `library` | Shared library | Alongside dependent binary | `/usr/local/lib/` | `/usr/local/lib/` |
| `app` | GUI application | `Program Files\{id}\` + Start Menu | `/Applications/` | `~/.local/share/applications/` |
| `config-pack` | Config files only | `%APPDATA%\{id}\` | `~/.config/{id}/` | `~/.config/{id}/` |
| `dev-tool` | Developer tooling | `%LOCALAPPDATA%\Dev\{id}\` | `~/.dev/{id}/` | `~/.local/dev/{id}/` |
| `font` | Font files | Windows font registry | `~/Library/Fonts/` | `~/.local/share/fonts/` |

### File-level inference (within a package)

When no override is declared, individual files are placed by extension
and directory name within the package:

```
Windows:
  *.ps1, *.psm1, *.psd1   → PS module path (if type is ps-module)
                             or alongside binary (if type is cli-tool)
  *.exe                    → type-determined binary destination
  *.dll                    → alongside parent *.exe
  *.json, *.yaml, *.toml  → %APPDATA%\{id}\ (config)
  *.md, *.txt (docs/)     → ignored (not placed)
  *.lnk                    → requires explicit override declaration
  service descriptor       → SCM (if type is service)

macOS:
  *.dylib                  → /usr/local/lib/ or alongside binary
  *.plist (LaunchAgent)    → ~/Library/LaunchAgents/
  *.plist (LaunchDaemon)   → /Library/LaunchDaemons/ (requires elevation)

Linux:
  bin/*                    → ~/.local/bin/ (user) or /usr/local/bin/ (system)
  lib/*                    → ~/.local/lib/ (user) or /usr/local/lib/ (system)
  *.service                → ~/.config/systemd/user/ (user) or
                             /etc/systemd/system/ (system, requires elevation)
  share/*                  → ~/.local/share/ (user) or /usr/local/share/ (system)
```

---

## The Ruleset System

### Structure

Rules live in `crates/ruleset/rules/` as JSON files, one per OS.
Each rule has:

```json
{
  "id": "string",               // unique rule identifier
  "type": "string",             // package type this rule applies to
  "match": "string",            // glob pattern for files this rule covers
  "destination": {
    "user": "string",           // path when scope = user
    "system": "string"          // path when scope = system (may need elevation)
  },
  "register": {                 // optional — OS registration side effects
    "path": false,              // add to PATH
    "env_var": false,           // set env var (uses as.env_var name)
    "service": false,           // register as service (uses as.service name)
    "start_menu": false         // create Start Menu entry (Windows)
  },
  "priority": 100               // lower = higher priority when multiple rules match
}
```

### Community extension

Community rules live in a separate registry (details TBD — likely a GitHub
repo with a curated PR process). The runtime can pull updated rulesets
on demand:

```
> update rulesets
```

This is the only network operation the runtime ever performs, and it is
always explicit and user-initiated.

---

## The Placement Resolution Algorithm

```
resolve(package, manifest, overrides) → PlacementPlan

1. Determine scope
   - Check manifest.prefers.scope
   - Check if system scope requires elevation
   - If elevation needed and not available: warn, fall back to user scope
     (unless manifest.requires.elevation = true, in which case: hard fail)

2. For each file in package:
   a. Check manifest.overrides for explicit match → use if found
   b. Check ruleset for type + file pattern match → use highest priority match
   c. If no rule matches → place in %APPDATA%\{id}\ catch-all (Windows)
                            ~/.local/share/{id}/ catch-all (Unix)

3. Expand all destination paths
   - Resolve env vars (%APPDATA%, $HOME, etc.)
   - Resolve {id} tokens
   - Resolve {resolved:X.destination} cross-references

4. Check for conflicts
   - Does destination already contain a file with this name?
   - If yes: check version, prompt if downgrade, skip if same

5. Return PlacementPlan
   - List of (source, destination) pairs
   - List of registration side effects
   - List of hooks in execution order
   - Elevation requirement (bool)
```

---

## The TUI

### Library

Use `ratatui` (Rust). It is the mature, actively maintained successor to `tui-rs`.
Do not use any other TUI library.

### Screens

**1. Flashcard (pre-install)**

Rendered before any installation begins. Generated entirely from the
manifest + inferred PlacementPlan. The developer never authors the flashcard
content — the runtime generates it.

```
┌──────────────────────────────────────────────────────┐
│                                                      │
│  mytool  v1.0.0                                      │
│  by andrew                              cli-tool     │
│                                                      │
│  A CLI tool that does X.                             │
│                                                      │
│  ─────────────────────────────────────────────────  │
│                                                      │
│  installs as    mt                                   │
│  scope          current user                         │
│  location       C:\Users\andrew\AppData\Local\       │
│                 Programs\mytool\                     │
│  touches        AppData\Roaming\mytool\              │
│  needs admin    no                                   │
│  hooks          post-install script                  │
│                                                      │
│  ─────────────────────────────────────────────────  │
│                                                      │
│  [I] settle in          [C] leave it                 │
│                                                      │
└──────────────────────────────────────────────────────┘
```

**2. Installation sequence**

Live-updating display of each placement step as it executes.

```
mytool  v1.0.0
────────────────────────────────────────────────────────

  finding a place for everything...

  ✔  binary          → AppData\Local\Programs\mytool\mt.exe
  ✔  config          → AppData\Roaming\mytool\
  ✔  env var         → MYTOOL_HOME noted
  ◐  PATH            → making mt reachable...
  ·  post-install    →
  ·  shim            →

────────────────────────────────────────────────────────
  3 / 6        ▓▓▓▓▓▓▓░░░░░░░░░░░░░░
```

States: `✔` done, `✖` failed, `◐` in progress, `·` pending, `!` warning

**3. Command bar**

Always-open persistent interface. Single-line input with rich response area.

```
────────────────────────────────────────────────────────
  > _
────────────────────────────────────────────────────────
```

After input:

```
────────────────────────────────────────────────────────
  > install mytool

  found mytool v1.0.0 on local feed.
  press enter to see where it would settle.

────────────────────────────────────────────────────────
  > _
```

### Aesthetic direction

Lodge is a cabin in the woods. Things find where they belong and settle
in. The aesthetic is warm, unhurried, and tactile — aged timber, firelight,
worn leather. Not dark and cold. Not neon. The terminal should feel like
a well-lit workshop, not a server room.

Every visual decision should reinforce the core metaphor: *a place where
things are put away properly*.

### Colour palette

```
Background:     #1c1510  (dark walnut — deep warm brown, not black)
Surface:        #26190f  (worn timber — panel/card background)
Border:         #3d2b1a  (wood grain — dividers and frames)
Text primary:   #f0e6d3  (warm parchment — main readable text)
Text secondary: #a08060  (faded ink — muted labels, hints)
Accent:         #c8813a  (ember orange — primary interactive element)
Success:        #7a9e6a  (pine green — completions, confirmations)
Error:          #b85c4a  (hearthstone red — failures, hard stops)
Warning:        #c49a3a  (lantern amber — cautions, soft warnings)
In-progress:    #7a9ab0  (morning frost — active steps, spinners)
Highlight:      #e8c98a  (candlelight — focused element, cursor)
```

### Typography and symbols

Use box-drawing characters that feel structural, not decorative.
Prefer `─` `│` `┌` `┐` `└` `┘` over heavier double-line variants.
Step states use earthy symbols:

```
✔  done          (warm, settled)
✖  failed        (clear, not alarming)
◐  in progress   (turning, not urgent)
·  pending       (quiet, waiting)
!  warning       (alert, not panic)
```

No spinner animations that feel frantic. A slow pulse or a simple
rotating `◐ ◑ ◒ ◓` is enough. The install is not an emergency.

### Splash screen

When the user runs `lodge` with no arguments to open the command bar,
the first thing rendered is a splash screen before the prompt appears.

It consists of three elements, vertically stacked and horizontally centred:

1. **The cabin art** — a circular bracket-art rendering of a log cabin in
   the woods, 40 bracket-pairs wide × 21 rows tall (80 terminal columns).
   Generated from truecolor ANSI escape sequences. Three bracket types
   zone the image by region:
   - `{}` — upper third (forest canopy)
   - `[]` — middle band (cabin structure)
   - `()` — lower third (forest floor, fallen leaves)

2. **The wordmark** — `lodge` in large ASCII lettering, in accent colour
   (`#c8813a` ember orange), centred below the art. Single blank line
   between art and wordmark.

3. **The version line** — `v0.1.0  ·  a place for everything` in secondary
   text colour (`#a08060`), centred below the wordmark.

Layout:

```
[blank line]
[cabin bracket art — 21 rows, 80 cols wide]
[blank line]
  l o d g e
  v0.1.0  ·  a place for everything
[blank line]
────────────────────────────────────────────────────────
  > _
```

The tagline `a place for everything` is fixed — it is not configurable.
It captures the entire product philosophy in four words.

#### Cabin art — Rust source constant

The art is stored as a `const &str` in `crates/runtime/src/tui/splash.rs`.
The string contains raw ANSI truecolor escape sequences (`\x1b[38;2;R;G;Bm`)
and must be written with a raw string literal or with explicit `\x1b` escapes.

The art data (copy exactly, including leading spaces for circular crop):

```
Row format per pixel: \x1b[38;2;R;G;Bm{BRACKET_PAIR}\x1b[0m
Outside circle: two spaces "  "
Bracket pairs by zone: "{}" (top), "[]" (middle), "()" (bottom)
Dimensions: 40 pairs wide × 21 rows = 80 terminal columns × 21 rows
```

Claude Code should regenerate the art constant from the source image
`assets/cabin.jpg` using the script at `scripts/gen_splash_art.py` if
the art needs updating. Do not hardcode colour values by hand.

#### ASCII wordmark

```
 ██╗      ██████╗ ██████╗  ██████╗ ███████╗
 ██║     ██╔═══██╗██╔══██╗██╔════╝ ██╔════╝
 ██║     ██║   ██║██║  ██║██║  ███╗█████╗
 ██║     ██║   ██║██║  ██║██║   ██║██╔══╝
 ███████╗╚██████╔╝██████╔╝╚██████╔╝███████╗
 ╚══════╝ ╚═════╝ ╚═════╝  ╚═════╝ ╚══════╝
```

Rendered in accent colour `#c8813a`. Centred in the terminal width.
If terminal is narrower than 80 columns, fall back to plain text `lodge`
in the same colour.

### Tone of copy

All strings the runtime produces — flashcard labels, error messages,
confirmations, command bar responses — should be calm and direct.
No exclamation marks. No "Done! 🎉". No "Uh oh!".

Good: `mytool settled in.`
Bad:  `✅ Installation complete!`

Good: `couldn't place config — AppData isn't writable. try running as admin.`
Bad:  `ERROR: Permission denied (os error 5)`

The runtime speaks like someone who knows what they're doing and
isn't in a hurry.

---

## The Brain (llama.cpp Integration)

### Architecture

The brain crate wraps llama.cpp via its C API using Rust FFI bindings
(`llama-cpp-rs` crate or direct bindgen — evaluate at implementation time).

The model loads once at startup and stays resident. Every command bar
input goes through the intent resolver before reaching the execution layer.

### Intent resolution

Input → Brain → Canonical command + structured arguments

The model is prompted with a system prompt that defines the command
vocabulary and returns structured JSON output via function calling:

```
System prompt (abbreviated):
  You are the intent resolver for an installation runtime.
  You receive user input and return a JSON object with:
    { "command": string, "args": object, "confidence": float }
  
  Known commands: install, uninstall, update, search, list,
                  info, verify, rollback, update-rulesets, help
  
  If confidence < 0.6, return:
    { "command": "clarify", "prompt": "what did you mean?" }
  
  Never generate text outside the JSON structure.
```

The structured output means the model never produces free-form text
that the runtime has to parse — it returns machine-readable intent
directly.

### Framer

For responses that need human-readable explanation (errors, info,
confirmations), the framer constructs the output from templates
enriched by model-generated context:

```rust
// Pseudo-code
fn frame_error(error: InstallError, context: &PackageContext) -> String {
    // Template provides structure
    // Model fills in plain-language explanation of why + what to do
}
```

### Conversation state

The brain maintains a short rolling context window (last 4 exchanges)
so follow-up questions work:

```
> install mytool
[flashcard shown]
> what does it touch?        ← "it" resolved from context
[expanded detail shown]
> actually cancel that
[installation aborted]
```

Context is in-memory only. Nothing persists between sessions.

---

## The Command Shim

When a `cli-tool` package installs, the runtime registers a shim so the
tool's command name resolves globally without PATH pollution.

### Mechanism (Windows)

The runtime maintains a single directory in the user PATH:
`%LOCALAPPDATA%\Programs\lodge\shims\`

For each installed CLI tool, it writes a tiny shim `.cmd` file:

```batch
@echo off
"%LOCALAPPDATA%\Programs\mytool\mt.exe" %*
```

The shim directory is added to PATH once at runtime install time.
Individual tools are added/removed by writing/deleting shim files.
No PATH modification required per tool install.

### Mechanism (Unix)

Symlinks in `~/.local/bin/` pointing to the actual binary.
`~/.local/bin/` is assumed to be on PATH per XDG convention.

### Version switching

The shim can point to a specific version:

```
> use mytool@1.0
Shim updated → mt now resolves to mytool v1.0.0
```

---

## Execution Receipts

Every installation writes a signed receipt to:
- Windows: `%LOCALAPPDATA%\lodge\receipts\{id}-{version}-{timestamp}.json`
- Unix: `~/.local/share/lodge/receipts/{id}-{version}-{timestamp}.json`

Receipt schema:

```json
{
  "id": "mytool",
  "version": "1.0.0",
  "installed_at": "2026-04-28T12:00:00Z",
  "scope": "user",
  "placements": [
    {
      "source": "bin/mt.exe",
      "destination": "C:\\Users\\andrew\\AppData\\Local\\Programs\\mytool\\mt.exe",
      "hash": "sha256:abc123..."
    }
  ],
  "registrations": ["PATH", "MYTOOL_HOME"],
  "hooks_run": ["post_install"],
  "runtime_version": "0.1.0",
  "receipt_hash": "sha256:def456..."
}
```

The `receipt_hash` is a SHA-256 of the entire receipt minus the hash field itself.
This makes receipts independently verifiable and tamper-evident.

Receipts are used for:
- Clean uninstall (know exactly what to reverse)
- Rollback (reinstall from receipt)
- Audit (`list --history`)
- Integration with PsyPunker attest layer (future)

---

## System Exploration

### What it is

System exploration is the brain's ability to **introspect the host machine
and reason about what it finds** — not just what Lodge installed, but the
full observable state of the system. The user can ask questions in plain
language and get grounded, accurate answers derived from real system calls,
not from model training data or guesswork.

This is distinct from the intent resolver. Intent resolution maps input to
a Lodge command. System exploration maps input to a *question about the
machine*, executes the appropriate probe, and lets the model reason over
the real result.

### The problem it solves

Without this, the brain is blind to anything outside Lodge's own install
records. A user asking:

> *"will this package run on my machine?"*
> *"do I already have node installed?"*
> *"is something already listening on port 3000?"*
> *"what version of .NET do I have?"*
> *"is my PowerShell execution policy going to block this?"*

...would get either silence or a fabricated answer based on training data.
With system exploration, the brain probes first, then reasons over ground
truth.

### Architecture — `scout.rs`

The scout is a collection of **probe functions** — pure Rust functions that
query OS state and return structured results. The brain calls probes when
the intent resolver classifies an input as a system query rather than a
Lodge command.

```rust
// Probe return type — always structured, never raw strings
pub struct ProbeResult {
    pub probe: &'static str,    // which probe ran
    pub found: bool,            // did it find what it looked for
    pub value: Option<String>,  // the actual value if found
    pub raw: Option<String>,    // raw output for model context
    pub error: Option<String>,  // if probe itself failed
}
```

### Built-in probes (Windows primary, cross-platform where noted)

| Probe | What it queries | Method |
|-------|----------------|--------|
| `ps_version` | PowerShell version installed | `$PSVersionTable` via PS invocation |
| `dotnet_runtimes` | .NET runtime versions present | `dotnet --list-runtimes` |
| `node_version` | Node.js version | `node --version` |
| `python_version` | Python version(s) | `python --version`, `python3 --version` |
| `port_in_use` | Whether a port is bound | TCP bind attempt or `netstat` parse |
| `service_status` | Whether a named service exists/runs | SCM query (Windows) / systemctl (Linux) |
| `env_var` | Value of an environment variable | `std::env::var` |
| `execution_policy` | PowerShell execution policy | `Get-ExecutionPolicy` |
| `disk_space` | Free space on a drive or path | `statvfs` / `GetDiskFreeSpaceEx` |
| `os_build` | OS version and build number | `ver` / `uname -r` / `sw_vers` |
| `registry_key` | Windows registry key value | `winreg` crate |
| `process_running` | Whether a named process is active | process list scan |
| `path_exists` | Whether a path exists and its type | `std::fs::metadata` |
| `path_writable` | Whether a path is writable | probe write attempt |
| `arch` | CPU architecture | `std::env::consts::ARCH` |

### How the brain selects and runs probes

When the intent resolver returns `{ "command": "explore", ... }`, the brain:

1. Passes the user's input + available probe list to the model
2. Model returns a structured probe invocation:
   ```json
   { "probe": "port_in_use", "args": { "port": 3000 } }
   ```
3. Scout executes the probe → `ProbeResult`
4. Model receives the result and generates a plain-language response
5. Response rendered in the command bar

Multi-probe queries (e.g. "is my machine ready to run this package?") trigger
a **compatibility check** — the manifest's `requires` block is diffed against
a battery of probes, and the model narrates the result:

```
> will mytool run on here?

  checking your setup...

  ✔  Windows 11  (build 22631)   — required: 10.0.19041+
  ✔  PowerShell 7.4.1            — required: 7.2+
  ✔  admin rights available      — required: yes
  ✖  port 8080 is in use         — mytool's default port

  mostly ready. port 8080 is occupied — something else is
  already running there. mytool lets you change the port
  at first run.
```

### What the scout does NOT do

- Does not execute arbitrary shell commands on the user's behalf
- Does not read file contents (only existence and writability)
- Does not access the network
- Does not cache results — every probe runs fresh against current state
- Does not expose probe results outside the brain's context window

The scout is read-only and scoped. It answers questions about the system;
it does not act on it.

### Probe dispatch table

Probes are registered in a static dispatch table in `scout.rs`. Adding a
new probe means adding one entry to the table and implementing the function.
The model learns about available probes from the system prompt, which is
generated from the dispatch table at startup — the prompt is always in sync
with what probes actually exist.

```rust
pub struct Probe {
    pub name: &'static str,
    pub description: &'static str,   // shown to model in system prompt
    pub args: &'static [&'static str],
    pub run: fn(args: &ProbeArgs) -> ProbeResult,
}
```

### Command reference additions

System exploration queries route through the command bar naturally —
no special syntax required. The intent resolver classifies them:

| Example input | Classified as |
|--------------|--------------|
| `do I have node installed?` | `explore → node_version` |
| `what PS version am I running?` | `explore → ps_version` |
| `is port 8080 free?` | `explore → port_in_use { port: 8080 }` |
| `will mytool run on this machine?` | `explore → compatibility_check { package: mytool }` |
| `is my execution policy going to be a problem?` | `explore → execution_policy` |
| `how much space do I have on C:?` | `explore → disk_space { path: "C:\\" }` |
| `is the lodgehelper service running?` | `explore → service_status { name: lodgehelper }` |

---

### System exploration queries

These are not commands with fixed syntax — they are natural language
questions routed to the scout by the intent resolver. Any phrasing
that implies a question about the host machine will trigger exploration.

| Example input | Probe invoked |
|--------------|--------------|
| `do I have node installed?` | `node_version` |
| `what PS version am I on?` | `ps_version` |
| `is port 8080 free?` | `port_in_use` |
| `will mytool run on this machine?` | `compatibility_check` |
| `is my execution policy going to block this?` | `execution_policy` |
| `how much space on C:?` | `disk_space` |
| `is lodgehelper running?` | `service_status` |
| `what OS build am I on?` | `os_build` |
| `is AppData writable?` | `path_writable` |

### Installation commands

| Input | Canonical | Description |
|-------|-----------|-------------|
| `install <id>` | `install` | Install a package |
| `install <id>@<version>` | `install` | Install specific version |
| `install <path>` | `install` | Install from local path |
| `uninstall <id>` | `uninstall` | Remove a package |
| `update <id>` | `update` | Update to latest version |
| `update all` | `update-all` | Update all installed packages |
| `rollback <id>` | `rollback` | Revert to previous version |

### Discovery commands

| Input | Canonical | Description |
|-------|-----------|-------------|
| `search <query>` | `search` | Search available packages |
| `list` | `list` | List installed packages |
| `info <id>` | `info` | Show package details |
| `what's installed` | `list` | Natural language alias |

### Runtime commands

| Input | Canonical | Description |
|-------|-----------|-------------|
| `verify <id>` | `verify` | Verify installed package integrity |
| `update rulesets` | `update-rulesets` | Pull latest community rulesets |
| `use <id>@<version>` | `use` | Switch active version via shim |
| `history` | `history` | Show installation history |
| `help` | `help` | Show help |

---

## Build Configuration

### Cargo workspace

```toml
[workspace]
members = [
  "crates/runtime",
  "crates/brain",
  "crates/ruleset",
  "crates/shared",
]
resolver = "2"
```

### Runtime crate dependencies (indicative)

```toml
[dependencies]
ratatui = "0.28"
crossterm = "0.28"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
glob = "0.3"
semver = "1"
sha2 = "0.10"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
tokio = { version = "1", features = ["full"] }

[dependencies.brain]
path = "../brain"

[dependencies.ruleset]
path = "../ruleset"

[dependencies.shared]
path = "../shared"
```

### Brain crate dependencies (indicative)

```toml
[dependencies]
llama-cpp-2 = "0.1"   # evaluate: llama-cpp-rs, llama-cpp-2, or direct bindgen
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"

[dependencies.shared]
path = "../shared"
```

### Build flags

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
```

`strip = true` is important — keeps the binary lean. LTO enables
cross-crate inlining which is significant for a project of this structure.

---

## Model Integration Notes

### Model file

`SmolLM2-360M-Instruct-Q4_K_M.gguf`
- Size: ~271MB
- Source: `bartowski/SmolLM2-360M-Instruct-GGUF` on HuggingFace
- License: Apache 2.0
- Supports: instruction following, function calling, structured output

### Bundling strategy

During build, the model is embedded or placed alongside the binary.
Two options — decide at implementation time:

**Option A — Alongside (preferred):** Ship as `runtime.exe` + `model.gguf`
in the same directory. Simple, no decompression step, model is replaceable
by the user. Installer package places both files.

**Option B — Embedded:** Use `include_bytes!` macro to embed the model
directly into the binary. Single-file distribution. Increases compile time
significantly. Model not user-replaceable without recompile.

Recommendation: Option A. The user can swap a better model by replacing
the `.gguf` file. This is a feature, not a limitation.

### Model path resolution

```rust
fn model_path() -> PathBuf {
    // 1. Check env var LODGE_MODEL_PATH
    // 2. Check alongside executable
    // 3. Check %LOCALAPPDATA%\lodge\model.gguf (Windows)
    //    ~/.local/share/lodge/model.gguf (Unix)
    // 4. Fail with helpful error message
}
```

### Context window

Use a 2048 token context. The command bar inputs are short and the
rolling context of 4 exchanges fits well within this. Do not increase
unless a specific need arises — larger context = higher memory usage
at inference time.

---

## Testing Strategy

### Unit tests

- Manifest parsing: valid, minimal, missing required fields, unknown fields
- Placement resolver: each file type on each OS, override precedence,
  elevation fallback, conflict detection
- Ruleset loader: valid rules, conflicting priorities, malformed JSON
- Receipt writer: hash correctness, schema validity
- Shim writer: correct `.cmd` content, correct symlink targets

### Integration tests

Located in `tests/integration/`. Each test uses a fixture package from
`tests/fixtures/packages/`.

Fixture packages to create:
- `minimal/` — id, version, type only
- `cli-full/` — all manifest fields populated
- `ps-module/` — PowerShell module with correct structure
- `service/` — service descriptor, requires elevation
- `with-overrides/` — explicit placement overrides
- `with-hooks/` — pre and post install hooks
- `conflict/` — would overwrite existing file

### TUI tests

Use `ratatui`'s built-in testing utilities for terminal buffer snapshots.
Test flashcard rendering for each package type and the sequence display
for each step state.

### Brain tests

Mock the llama.cpp layer. Test intent resolution with a lookup table of
known inputs → expected canonical commands. Test confidence threshold
behaviour. Test context window rollover.

---

## Phase 1 Milestones

### Week 1–2: Foundation
- Cargo workspace setup
- `shared` crate: manifest types, placement types, receipt types
- Manifest parser with full validation
- Basic CLI entry point (`clap`)

### Week 3–4: Ruleset engine
- `ruleset` crate: rule types, loader, matcher
- Windows built-in ruleset (complete)
- Placement resolver (no overrides yet)
- Unit tests for resolver

### Week 5–6: Execution engine
- Full placement executor (file copy, directory creation)
- Override handling in resolver
- Registration side effects (PATH, env var, shim)
- Receipt writer
- macOS + Linux rulesets (initial)

### Week 7–8: TUI
- `ratatui` integration
- Flashcard screen
- Installation sequence screen (live updates)
- Colour palette applied
- TUI snapshot tests

### Week 9–10: Brain integration
- `brain` crate: llama.cpp bindings
- Model loading + path resolution
- Intent resolver with system prompt
- Command bar UI in TUI
- Context state management
- `scout.rs`: probe dispatch table + built-in Windows probes
- Compatibility check against package manifest `requires` block
- Plain-language probe result framing

### Week 11–12: Polish + distribution
- Shim registration (Windows `.cmd`, Unix symlinks)
- Version switching
- `update-rulesets` command
- Full integration test suite
- Release build optimisation
- Installation package for the runtime itself (dog-fooding)

---

## Known Open Questions

These are decisions deferred to implementation time:

1. **Binary name** — the tool is called `lodge` at the command line.
   The crate name is `lodge`. The shim directory on Windows is
   `%LOCALAPPDATA%\Programs\lodge\shims\`. Receipts live under
   `%LOCALAPPDATA%\lodge\` (Windows) and `~/.local/share/lodge/` (Unix).

2. **Package registry** — where do packages live? Local feed only for Phase 1.
   Remote registry design is out of scope until Phase 2.

3. **llama.cpp Rust bindings** — evaluate `llama-cpp-2` vs `llama-cpp-rs`
   vs direct bindgen at implementation start. Document choice here.

4. **Model bundling** — Option A (alongside) vs Option B (embedded).
   Recommendation is Option A but confirm before implementation.

5. **Code signing** — the runtime binary should be signed for Windows
   SmartScreen. Deferred to distribution phase.

6. **Community ruleset registry** — GitHub repo structure, PR process,
   and versioning. Out of scope for Phase 1.

---



## Relationship to PsyPunker / MitoData

This project is **entirely independent** of the PsyPunker initiative.
It shares no code, no repository, no release cycle.

The execution receipt format is designed to be compatible with
PsyPunker's attest layer as a future integration point, but this
is not a dependency in either direction. Do not import PsyPunker
types or reference PsyPunker architecture in this codebase.

---

## Claude Code Behaviour Notes

When working in this project:

- Always read this file first at the start of each session
- Prefer `anyhow` for error handling throughout — no `unwrap()` in
  non-test code
- All public functions must have doc comments
- Commit messages follow conventional commits:
  `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`
- Never add dependencies without noting the reason in the PR/commit message
- Run `cargo clippy -- -D warnings` before considering any code done
- Run `cargo test` before considering any feature done
- If a design decision contradicts this document, flag it explicitly
  rather than silently diverging
