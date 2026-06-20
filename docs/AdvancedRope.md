# Advanced Rope Mechanics: Bash Engine & Configuration

This guide provides a detailed operational reference for Rope's internal `bash` execution engine and its configuration options. It is designed for developers and recursive agent invocations.

---

## 🛠️ The Bash Tool Execution Model (Tmux Engine)

Rope uses a custom **Tmux-based terminal pipeline** to balance developer safety, persistent context, and rich terminal interactions.

### 1. Isolated Persistent Sessions
Every running instance of Rope spawns an isolated background tmux session named:
`rope-shell-<PID>`
* **PID Isolation:** Using the process ID prevents multiple concurrent runs of Rope from clumping into the same shell.
* **State Persistence:** Because the same session remains active across tool calls, shell state (directory changes via `cd`, exports, environment variables) persists naturally.
* **Recursive Nesting:** Running `rope` inside a parent `rope` session spawns a new session under the child PID. Because `$TMUX` is stripped during attachment, recursive splits nested within each other work automatically.

### 2. Interactive Approval Modality
When the agent executes a command in `Ask` policy mode:
1. Rope sends the command keys to the tmux session **without un-buffered carriage returns** (does not press Enter).
2. Rope pops a terminal GUI window (using the configured terminal emulator) or splits the current terminal pane horizontally (in TTY mode) attaching to the session.
3. **User Action:**
   * **Approve:** Focus the popped pane/window and press `Enter` to run.
   * **Modify:** Edit the command line inline, then press `Enter`.
   * **Cancel/Deny:** Close the window (GUI) or kill/detach the split pane (TTY). Rope detects the window/pane exit, sends `Ctrl+C` (`C-c`) to clear the prompt in the background session, and returns a `Permission Denied` tool response.
4. **Completion Detection:** Rope polls the terminal buffer (`tmux capture-pane -pJ`) and the current process status. When the active command completes and the shell returns to idle with a fresh prompt line, Rope captures the scrollback and returns the output to the agent.

### 3. Headless YOLO Modality (Auto-Yes)
If the safety policy for `bash` is set to `Yes` (auto-approve):
1. Rope types the command in the background session.
2. Rope **immediately sends the carriage return (`C-m`)** to submit the command.
3. **No Pop:** Rope does not spawn a terminal window or split pane, preventing focus stealing and visual screen noise.
4. The command runs headlessly, and Rope polls the buffer silently in the background until completion.

### 4. Hardening Fallback
If the `tmux` binary is not installed on the system, Rope automatically detects this and falls back to a standard, synchronous, one-off command process runner (running `bash -c <cmd>`).

---

## ⚙️ Configuration Reference (`config.toml`)

Configurations are loaded and overlayed in order:
`CWD/config.toml` $\rightarrow$ `~/.config/rope/config.toml`

### General Options
* **`model`** (string): The identifier of the model to use (e.g. `"gemini-3.1-flash-lite"`, `"gemini-3.5-flash"`).
* **`adapter`** (string): The service adapter to target. Supported values: `"gemini"`, `"openai"`, `"anthropic"`.
* **`api_key`** (string): The API credentials. Rope automatically exports this into the corresponding standard env var (e.g. `GEMINI_API_KEY` for gemini adapter) dynamically before starting.
* **`endpoint`** (string): A custom API endpoint URL to redirect client queries (useful for proxy relays or local models).
* **`terminal`** (string): The terminal emulator command. Defaults to `"alacritty -e"`. Rope splits this by whitespace to resolve custom configurations (e.g., `"kitty"`, `"gnome-terminal --"`).

### Policy Configuration (`[policy]`)
Defines the permissions model for tool categories. Valid values are:
* `"yes"`: Auto-approve execution.
* `"no"`: Auto-deny execution.
* `"ask"`: Ask user (popping terminal window for bash, or inline prompt for filesystem).

Fields:
* **`access_lookup_directory`** (string): Permission to read/grep files inside the active workspace directory.
* **`access_lookup_fs`** (string): Permission to read/grep files outside the workspace directory.
* **`access_lookup_web`** (string): Permission to scrape web page contents.
* **`access_write_directory`** (string): Permission to create/patch files inside the active workspace.
* **`access_write_fs`** (string): Permission to create/patch files outside the active workspace.
* **`access_bash`** (string): Permission to run shell commands.
