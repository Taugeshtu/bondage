# Rope: The Lightweight TTY Agent Scalpel

Rope is a minimalist TTY CLI companion for the **Bondage** stateless actor library. It provides a quick, lightweight shell interface to execute targeted, local agentic tasks.

---

## 🎯 What it IS vs. What it ISN'T

* **It IS a Scalpel**: It is designed for small, highly targeted, and constrained agentic work. It is your "emergency bailout" tool. You get in, get a scoped amount of work done, and get out.
* **It ISN'T a Bulldozer**: It is **not** a "do-it-all" autonomous harness, nor is it a multi-agent meta-orchestrator. It does not native-fork inside a complex terminal layout, because terminal layouts make for poor IDE workspaces. Deep work belongs in a dedicated agent workstation; Rope is for surgical strikes.

---

## 🛠️ Current Capabilities

### 1. Minimalist Prompt Parsing
You don't need to wrap your prompts in quotes unless your prompt contains shell-interpreted characters (like the single quote in `what's` or redirects):
```bash
rope -c gemini please inspect the files in src and tell me what they do
```

### 2. Smart Configuration Loading
Config targets passed to `-c` are automatically resolved:
* Tries appending `.toml` if omitted (e.g. `-c gemini` -> `gemini.toml`).
* Searches CWD, then resolves relative paths, then falls back to check in `~/.config/rope/`.

### 3. Greedy Prompt File Injection (`@` Syntax)
Any files referenced with `@` in the prompt are automatically read and embedded:
* Matches spaces greedily against existing files on disk (e.g. `@My Spaced File.txt` matches without quotes).
* Supports **Level-1 Recursive Nesting** (files referenced inside an embedded file are pulled in, capped at depth 1 to prevent runaway token consumption).

### 4. YOLO Mode via Policy Config (`-c yolo`)
Disables interactive confirmation prompts by overriding safety policies to `"yes"` (auto-allow). Runs tool executions automatically—ideal for headless scripting, automated test suites, or fast iterations:
```bash
rope -c gemini -c yolo "Update the version in Cargo.toml to 0.1.1"
```
On first launch, Rope automatically generates a default `yolo.toml` file under `~/.config/rope/` pre-configured to allow all local and network tools.

### 5. Configurable Safety Policies
Rope maps tools to three access modes: `yes` (auto-allow), `no` (auto-deny), and `ask` (interactive prompt). Configs can be chained (e.g., `-c gemini -c no_network`) to dynamically override policies like `access_lookup_web` or `access_write_fs` for sandboxed runs.

### 6. XML-Structured Provenance
All tool outputs (directory trees, file reads, grep searches, web requests) are returned to the model wrapped in explicit XML tags (e.g. `<dir>`, `<file>`, `<fragment>`).
* This provides structured provenance for model context.
* It remains robust under aggressive model quantization (unlike space-indentation trees, XML tags cannot be flattened or lost in translation).
* File contents are strictly inlined inside `<content>` tags without artificial newlines or formatting indentation, preventing the model from generating whitespace-distorted patches.

---

## 🗺️ Roadmap & Design Notes (Planned)

### 1. Interactive & Persisting Sessions (`-i` / `--interactive`)
For tasks that require more than one turn, Rope will support dropping into a persisting shell:
* Saves the state of the message rope inside a `.rope/` directory in CWD (or `~/.rope/` if permission is denied).
* Allows restoring a session or recovering after closing the terminal via `rope -i @.rope/session-id`.

### 2. Surface Utility
A `surface` overview builder to extract the structural overview (signatures, headers, APIs) of large text/code files, avoiding token blowouts.
