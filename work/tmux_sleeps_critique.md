# Architectural Critique: Tmux Polling & Hardcoded Sleeps

During the implementation of the TTY/tmux interactive approval loop, two hardcoded delays were introduced to resolve race conditions:
1. **Shell Boot Delay (500ms):** Waiting for `bash` to load profile files and print its initial prompt before typing the command literal.
2. **Terminal Resize Delay (250ms):** Waiting for the popped terminal client (Alacritty split/pane) to attach and trigger the session redraw before capturing the baseline cursor position.

While these sleeps make the system "usable" under normal conditions, they are classic code smells (**time-dependent polling races**), analogous to Unity's `yield return new WaitForEndOfFrame()`. 

---

## ⚠️ The Smell and the Risk
* **Flakiness under load:** If the host machine is under heavy CPU load, spawning the terminal process and attaching to tmux might take longer than 250ms. The resize redraw happens *after* the sleep, causing a false-positive cursor delta check and triggering a premature auto-close.
* **Wasted latency:** If the system is running fast, we block the main thread waiting for arbitrary durations, degrading execution responsiveness.
* **Silent failures:** If a terminal emulator fails to launch entirely, the loop might hang or exhibit undefined state transitions.

---

## 🛠️ The Deterministic (Event-Driven) Refactoring Plan

Instead of sleeping for fixed durations, we can transition to **state-driven invariants** to detect when asynchronous OS events have completed:

### 1. Eliminating the 250ms Resize Sleep
**The Problem:** We need to wait until the terminal window/pane has popped and attached to the session (updating the session geometry) before capturing the initial state.
**The Solution:** Poll tmux for attached clients instead of sleeping:
1. Call `pop_terminal`.
2. Poll `tmux list-clients -t <session>` in a tight loop (e.g., every 20ms) until it returns at least one attached client (or times out after 2s).
3. Once `has_clients` is `true`, we *know* the client has attached and the terminal has resized. 
4. Capture `initial_state` immediately.

```rust
// Draft:
log_debug("Waiting for terminal client attachment...");
let mut client_attached = false;
for _ in 0..100 { // 2 seconds max
    if tmux_utils::has_attached_clients(&session_name) {
        client_attached = true;
        break;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
}
// Now it is safe to capture baseline cursor coordinates
let initial_state = tmux_utils::get_pane_cursor_state(&session_name)?;
```

### 2. Eliminating the 500ms Shell Boot Sleep
**The Problem:** We need to wait until `bash` has finished printing its prompt and is ready for stdin before we type the command.
**The Solution:** Check for cursor resting position or prompt detection:
* When a headless session starts, the cursor is at `(x=0, y=0)` before the shell prints anything.
* Once the shell prints the prompt, the cursor moves to the right (e.g. `cursor_x > 0`).
* We can poll `cursor_x` in a tight loop until it is greater than `0` (or matches a standard prompt structure in the pane text) before sending the command literal.

---

## 📋 Status & Next Steps
We are keeping the current sleep strategy in place to ensure near-term usability, but this document serves as a blueprint for refactoring the tmux orchestration code to be 100% deterministic.
