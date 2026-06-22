**Rope** - the MVP of **Bondage** *(a.k.a. "just barely enough harness to have fun")*

  > `rope [-h|--help]`
  > `rope [-c <config_name>...] [-s <system_name>...] [-l|--log] [--no-tmux|--notmux] [<prompt...>|"<prompt...>"]`
  > `rope [-c <config_name>...] [-s <system_name>...] [-l|--log] [--no-tmux|--notmux] [-i|--interactive <file>]`

### Interactive mode (File-Sitter)
  Run with `-i <file>` or `--interactive <file>` to launch the file-sitter interactive mode.
  It watches a markdown file (e.g. `session.md`) for changes, triggers an LLM turn when the file is saved containing the `@rope` activation tag, and auto-approves write calls to the session file itself.
  To fork a session, simply copy the session markdown file and run a separate `rope -i` process on it.

### System Prompts
  `-s <name>` or `--system <name>` flag loads system prompt files (checks CWD first, then `~/.config/rope/`).
  Multiple system prompts can be specified to overlay (concatenate) them.
  If not specified, defaults to loading `system-regular.txt` (or `system-interactive.txt` in file-sitter mode) from `~/.config/rope/`.

### Headless Execution
  `--no-tmux` or `--notmux` bypasses the Tmux split-screen bootstrapping environment.

### File Injections
  `@path/to/file` inside the prompt = automatically embed their contents (supports `@spaced filenames.txt` and level-1 recursive nesting)

### Configs handling
  `.toml` extension appended automatically
  resolved in order: `CWD` local path -> `~/.config/rope/`
  overlayed: settings in later configs (like `-c yolo`) override previous ones

### Logging
  `-l` or `--log` flag will output detailed execution trace logs into `rope_debug.log` in the active CWD.

### Permissions policy
  set via config, see `~/.config/rope/yolo.toml` for all the knobs. Values: `yes`|`no`|`ask`
  defaults: read everywhere, ask to write, ask for bash

### Tricks
  > `rope -c yolo`
  This will use first-time-installed allow-everything config

  > `rope "<prompt>"`
  Wrapping in quotes protects against shell's interpretation of special characters - single quote, redirects

---

For advanced details on bash execution and full config options, see:
[AdvancedRope.md](AdvancedRope.md)
