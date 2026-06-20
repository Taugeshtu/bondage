**Rope** - the MVP of **Bondage** *(a.k.a. "just barely enough harness to have fun")*

  > `rope [-c <config_name>...] [-h|--help] [<prompt...>]`

### Interactive mode
  (NotImplementedException) launch with empty prompt

### File Injections
  `@path/to/file` inside the prompt = automatically embed their contents (supports `@spaced filenames.txt` and level-1 recursive nesting)

### Configs handling
  `.toml` extension appended automatically
  resolved in order: `CWD` local path -> `~/.config/rope/`
  overlayed: settings in later configs (like `-c yolo`) override previous ones

### Permissions policy
  set via config, see `~/.config/rope/yolo.toml` for all the knobs. Values: `yes`|`no`|`ask`
  defaults: read everywhere, ask to write, ask for bash

### Tricks
  > `rope -c yolo`
  This will use first-time-installed allow-everything config

  > `rope "<prompt>"`
  Wrapping in quotes protects against shell's interpretation of special characters - single quote, redirects
