# Bondage Project Status

tools:
- [~] `lookup` tool
	- [x] Local file lookup (anchor keyword search + radius context)
	- [x] Local directory lookup (listing folder items and recursive grep)
	- [x] Web URL scrap (fetch webpage text via HTTP)
	- [ ] Web search integration (querying search engine)
	- [x] respecting gitignore!
	- [ ] limiting output size
- [x] `write` tool
	- [x] Full file overwrite (creating missing folders automatically)
	- [x] Substring match-and-replace patch (safety check for uniqueness)
- [x] `bash` tool
	- [x] Shell command execution
	- [x] Output capture (binding stdout/stderr and exit status codes)
	- [x] limiting output size

runtime:
- [x] GenAI Integration
	- [x] Use direct GenAI client structs without wrappers
	- [x] Implement conversion utilities in `util.rs`
	- [x] Implement standard `step` logic
	- [x] Implement step_stream logic with text delta filtering

bonus:
- [x] Policies helper functions (managing allowances and security rules)
- [~] hardening (looking for things that may break, and putting padding there)
	- [x] Fail when requested config missing
	- [x] Fail/notify when configured terminal command is missing or broken
	- [x] Harden tmux send-keys against command/keystroke injection and flag misinterpretation
	- [x] Fix security policy bypass when tmux is missing
	- [x] Prevent circular directory symlink loops in grep search (stack overflow crash fix)
	- [x] Fallback to inline approval in headless/non-interactive TTY environments to prevent hangs
	- [x] Support multi-line command output polling in tmux pipeline
	- [ ] Retry tool calls (4 attempts, progressive decay)
	- [ ] Retry LLM calls?

rope:
- [x] MVP version that can call wrap Bondage into a CLI app - from call to output, including tool use
- [x] asking user's permission for tool calls
- [x] loading specified config
- [x] multiple overlaying configs
- [x] yolo mode
- [x] permissions policy in a config
- [x] pretty-printing help
- [ ] pretty-printing whole screen, maybe
- [ ] interactive mode

