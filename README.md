# lodge

A language-agnostic installer runtime with a conversational terminal interface.

Lodge reads a developer-shipped manifest, resolves file placements against the target OS, executes each step live in a rich terminal UI, and understands natural language commands via a fully offline bundled model. It can also introspect the host machine to reason about compatibility — including systems and state entirely outside of what Lodge itself installed.

---

## What it does

- **Installs packages** from a local feed or remote URL, placing each file where it belongs on the target OS
- **Understands plain English** — `get mytool`, `remove mytool`, `what node version am I on?` all work
- **Introspects the host** — probes developer tools, system info, environment state, and answers questions about it
- **Runs fully offline** — the model, ruleset, and runtime ship together; no network required for installation
- **Manages extensions** — a registry of first-party and community tools, toggled and installed from within the command bar via `!ext`

---

## Architecture

```
crates/
├── runtime/        core installation engine + terminal UI
│   ├── engine/     manifest parsing, placement resolution, execution, receipts
│   └── tui/        ratatui screens: splash, onboarding, command bar, extension browser
├── brain/          AI integration (offline llama.cpp + online providers via genai)
├── ruleset/        OS placement rules — Windows, macOS, Linux
├── shared/         types shared across crates
└── clean-cabin/    first-party extension: suggestion-based file system cleanup
```

---

## Getting started

**Build**

```sh
cargo build
cargo build --release
```

**Run**

```sh
cargo run -p lodge
```

**Tests + lint**

```sh
cargo test
cargo clippy -- -D warnings
cargo fmt
```

---

## The command bar

Lodge opens to a persistent command bar. Type naturally:

| Input | What happens |
|-------|-------------|
| `install mytool` | Install a package from the local feed |
| `remove mytool` | Uninstall a package |
| `list` | Show installed packages |
| `do I have git?` | Probe the host and answer |
| `is port 8080 free?` | Check if a TCP port is bound |
| `will mytool run here?` | Compatibility check against package requirements |
| `!ext` | Open the extension browser |
| `!clean` | Run the Clean Cabin extension |
| `help` | Show available commands |

---

## Extensions

Extensions are optional tools that run alongside Lodge. They are listed in `extensions/registry.json`, toggled on/off from the `!ext` browser, and installed on demand.

**Official extensions** are designated in `extensions/official.json` and maintained by the Lodge team. Community extensions can be contributed via pull request to `extensions/registry.json`.

### Contributing an extension

1. Add your entry to `extensions/registry.json`
2. Open a pull request — the registry validator runs automatically and blocks merge on alias collisions, missing SHA-256, or invalid fields
3. Official designation requires a separate PR touching `extensions/official.json`, which requires maintainer approval

### Clean Cabin

Suggestion-based file system cleanup. Scans a directory, tiers findings by confidence, stages files non-destructively before any deletion occurs.

```
!clean               scan home directory
!clean <path>        scan a specific directory
!clean recover       restore staged files
!clean purge         permanently delete staged files
!clean config        show scan configuration
```

---

## The manifest format

A minimal valid manifest:

```json
{
  "id": "mytool",
  "version": "1.0.0",
  "type": "cli-tool"
}
```

The manifest is diegetic — it describes what the package *is*, not what to do with it. Lodge infers placement from the declared type and the OS ruleset.

Full schema: [`docs/manifest-spec.md`](docs/manifest-spec.md)

---

## AI integration

Lodge ships with two AI paths:

- **Offline** — llama.cpp with a bundled SmolLM2-360M model. Always available, no configuration needed. Handles intent resolution and basic natural language commands.
- **Online** — configurable via `!key set <api-key>` to use Claude, OpenAI, Ollama, or other providers through the `genai` crate. Used for richer responses and system exploration narration when available.

The offline model is never required for core installation — it only enhances the conversational interface.

---

## Aesthetic

Lodge is a cabin in the woods. Things find where they belong and settle in. The terminal should feel like a well-lit workshop, not a server room.

```
Background    #1c1510   dark walnut
Accent        #c8813a   ember orange
Success       #7a9e6a   pine green
Error         #b85c4a   hearthstone red
Warning       #c49a3a   lantern amber
```

---

## License

© 2026 KronTheDev. All rights reserved.
