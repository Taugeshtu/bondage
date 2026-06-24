# Bondage Project Status

tools:
- [~] `lookup` tool
	- [x] Local file lookup (anchor keyword search + radius context)
	- [x] Local directory lookup (listing folder items and recursive grep)
	- [x] Web URL scrap (fetch webpage text via HTTP)
	- [ ] Web search integration (querying search engine)
	- [x] respecting gitignore!
	- [ ] limiting output size
	- [ ] Set realistic browser User-Agent header (bypass simple bot-blocking)
	- [ ] Detect & warn on empty/boilerplate JS-rendered SPA web lookup results
- [x] `write` tool
	- [x] Full file overwrite (creating missing folders automatically)
	- [x] Substring match-and-replace patch (safety check for uniqueness)
	- [ ] Strict exact-match check for substring patches or high-bar fuzzy matching
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
	- [ ] Support reasoning_content round-tripping for thinking-mode models

bonus:
- [x] Harness utility "Kit" (extracting step_agent and ToolExecutor trait)
- [x] Recursive @file prompt injection utility in prompt_file_injector.rs
- [x] Smart stdout/stderr head/tail truncation utility (truncate_text)
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
	- [ ] What if write in patch mode fails because didn't match content?
	- [x] Fix path resolution failing on tilde `~` expansion and mergerFS mounts (e.g. `~/K/Catch-all/`)
	- [ ] Hard cap on tool calls per agent turn inside step_agent to prevent doom-looping
	- [ ] Replace hardcoded sleeps in tmux orchestration with event-driven state checks
	- [ ] Refine @file greedy injection to only trigger if @ is followed by alphanumeric characters
	- [ ] Support optional configuration to keep tmux terminal window open after command runs
	- [ ] Generate a repository structural/architecture map for agent navigation

rope:
- [x] MVP version that can call wrap Bondage into a CLI app - from call to output, including tool use
- [x] asking user's permission for tool calls
- [x] loading specified config
- [x] multiple overlaying configs
- [x] yolo mode
- [x] permissions policy in a config
- [x] pretty-printing help
- [x] Unified resource resolver (locate_resource) and template setup installer
- [x] Execution trace logging support via -l/--log flag
- [x] Quiet background execution mode for approved bash commands
- [x] Bootstrapping tmux horizontal splits in raw TTY environments
- [x] System prompts extracted out into external files (system-regular.txt, etc.)
- [ ] pretty-printing whole screen, maybe
- [x] interactive mode
	- [x] reacting to `@rope` marks
	- [x] auto-firing when invoked
	- [x] content-hash gate to ignore no-op saves
	- [ ] session-scoped "yes and remember" approval ("yy") for permissions
	- [ ] unifiend expansion pipeline
	- [ ] pipeline deduplication

