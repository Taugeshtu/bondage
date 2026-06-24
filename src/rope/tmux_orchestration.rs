use std::io::{self, Write};
use bondage::ToolResponse;
use crate::tmux_utils::{self, TerminalHandle};

pub fn ask_approval(tool_name: &str, args: &str) -> bool {
    print!("\n⚠️  [Agent wants to execute: {} ({})] Approve? (y/N): ", tool_name, args.trim());
    let _ = io::stdout().flush();
    
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let trimmed = input.trim().to_lowercase();
        trimmed == "y" || trimmed == "yes"
    } else {
        false
    }
}

pub static ENABLE_LOGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn log_debug(msg: &str) {
    if !ENABLE_LOGGING.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("rope_debug.log")
    {
        use std::io::Write as _;
        if let Ok(elapsed) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            let secs = elapsed.as_secs() % 86400;
            let millis = elapsed.subsec_millis();
            let _ = writeln!(file, "[{:02}:{:02}:{:02}.{:03}] {}", secs / 3600, (secs % 3600) / 60, secs % 60, millis, msg);
        } else {
            let _ = writeln!(file, "{}", msg);
        }
    }
}

pub async fn execute_bash_tmux(
    id: &str,
    arguments: &str,
    current_dir: &std::path::Path,
    policy_mode: bondage::policy::PolicyMode,
    custom_terminal: Option<String>,
) -> ToolResponse {
    let args: Result<bondage::tools::tool_bash::BashArgs, _> = serde_json::from_str(arguments);
    let command_to_run = match args {
        Ok(a) => a.command,
        Err(e) => {
            return ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: format!("Failed to parse arguments: {}", e),
                is_error: true,
            };
        }
    };

    // 0. Hardening Fallback: If tmux is not available on the host system, run command directly via tokio one-off process
    if !tmux_utils::is_tmux_available() {
        println!("⚠️  Warning: tmux is not installed. Falling back to raw one-off command execution.");
        
        // If the safety policy is Ask, we must prompt the user inline because we can't use the tmux-attach approval mechanism
        if policy_mode == bondage::policy::PolicyMode::Ask {
            if !ask_approval("bash", arguments) {
                return ToolResponse {
                    id: id.to_string(),
                    name: "bash".to_string(),
                    content: "Permission Denied: command execution cancelled by user.".to_string(),
                    is_error: true,
                };
            }
        }

        let baseline_res = bondage::tools::tool_bash::execute(
            bondage::tools::tool_bash::BashArgs { command: command_to_run },
            current_dir,
        ).await;
        
        return match baseline_res {
            Ok(content) => ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content,
                is_error: false,
            },
            Err(e) => ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: e.to_string(),
                is_error: true,
            },
        };
    }

    let pid = std::process::id();
    let session_name = format!("rope-shell-{}", pid);
    
    log_debug(&format!("--- Starting execute_bash_tmux for command: {} ---", command_to_run.trim()));

    // 1. Ensure tmux session exists
    let session_existed = tmux_utils::has_session(&session_name);
    log_debug(&format!("Session existed check: {}", session_existed));
    if !session_existed {
        log_debug("Starting new tmux session...");
        if let Err(e) = tmux_utils::start_session(&session_name, current_dir) {
            log_debug(&format!("Failed to start session: {}", e));
            return ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: format!("Failed to start tmux session: {}", e),
                is_error: true,
            };
        }
        // Let the shell initialize and display prompt before sending keystrokes
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        log_debug("New session started and slept 500ms");
    } else {
        log_debug("Printing separator inside existing session...");
        // Send C-c to abort any partial command, then print separator
        let _ = tmux_utils::send_control_key(&session_name, "C-c");
        let _ = tmux_utils::send_command_literal(&session_name, "echo \"\" && echo \"---\" && echo \"\"");
        let _ = tmux_utils::send_control_key(&session_name, "C-m");
        // Sleep to let the shell start executing the command, then poll until it returns to idle
        log_debug("Sent separator echo, waiting for shell idle...");
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let poll_interval = std::time::Duration::from_millis(50);
        let mut waited_ms = 150;
        for _ in 0..40 { // wait up to 2 seconds max
            if let Ok(idle) = tmux_utils::is_pane_idle(&session_name) {
                log_debug(&format!("Separator idle poll at {}ms: {}", waited_ms, idle));
                if idle {
                    break;
                }
            }
            tokio::time::sleep(poll_interval).await;
            waited_ms += 50;
        }
    }

    let mut command_submitted = false;
    let mut initial_val = 0;

    // 2. Send the command (without enter)
    log_debug(&format!("Sending command literal: {}", command_to_run));
    if let Err(e) = tmux_utils::send_command_literal(&session_name, &command_to_run) {
        log_debug(&format!("Failed to send command literal: {}", e));
        return ToolResponse {
            id: id.to_string(),
            name: "bash".to_string(),
            content: format!("Failed to send command to tmux: {}", e),
            is_error: true,
        };
    }

    // If the policy mode is "Yes" (auto-accept / yolo mode), send "Enter" right after the command
    if policy_mode == bondage::policy::PolicyMode::Yes {
        log_debug("Policy mode is Yes. Sending C-m (Enter) key...");
        command_submitted = true;
        if let Err(e) = tmux_utils::send_control_key(&session_name, "C-m") {
            log_debug(&format!("Failed to send C-m: {}", e));
            return ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: format!("Failed to automatically execute command in tmux: {}", e),
                is_error: true,
            };
        }
    }

    // 3. Pop the terminal (Only if policy_mode != Yes / we are NOT in auto-accept YOLO mode)
    let mut term_handle = None;
    let mut fallback_to_inline_approval = false;
    if policy_mode != bondage::policy::PolicyMode::Yes {
        log_debug("Policy mode is not Yes. Popping terminal...");
        println!("📺 Popping interactive terminal window (Alacritty / Tmux Split)...");
        term_handle = match tmux_utils::pop_terminal(&session_name, custom_terminal.as_deref()) {
            Ok(handle) => {
                log_debug(&format!("Terminal popped. Handle exists: {}", handle.is_some()));
                handle
            }
            Err(e) => {
                log_debug(&format!("Failed to pop terminal: {}", e));
                return ToolResponse {
                    id: id.to_string(),
                    name: "bash".to_string(),
                    content: format!(
                        "Error: Failed to spawn the configured terminal command: {}\n\
                         Please verify that the terminal emulator is installed and correctly configured in your config.toml.",
                        e
                    ),
                    is_error: true,
                };
            }
        };
        if term_handle.is_none() {
            log_debug("No terminal popped, falling back to inline approval.");
            fallback_to_inline_approval = true;
        }

        // Wait for terminal connection to attach and screen resize to settle before capturing baseline
        log_debug("Waiting for terminal client to attach...");
        let mut attached = false;
        let attach_poll_interval = std::time::Duration::from_millis(50);
        let mut waited_ms = 0;
        for _ in 0..60 { // wait up to 3 seconds max
            if tmux_utils::has_attached_clients(&session_name) {
                attached = true;
                log_debug(&format!("Client attachment detected after {}ms", waited_ms));
                break;
            }
            tokio::time::sleep(attach_poll_interval).await;
            waited_ms += 50;
        }

        if !attached {
            log_debug("Warning: Timeout waiting for client attachment. Capture baseline anyway.");
        }

        // Give tmux a tiny moment (50ms) to finish redraw/resize after attachment
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let initial_state = tmux_utils::get_pane_cursor_state(&session_name).unwrap_or((0, 0));
        initial_val = initial_state.0 + initial_state.1;
        log_debug(&format!("Captured initial state (post-resize): history={}, cursor_y={}, total={}", initial_state.0, initial_state.1, initial_val));
    }

    // 4. Polling / Approval loop
    if fallback_to_inline_approval {
        log_debug("Prompting for inline approval...");
        if ask_approval("bash", &arguments) {
            log_debug("Inline approval granted. Sending C-m (Enter) key...");
            // User approved, send Enter key to run the command
            if let Err(e) = tmux_utils::send_control_key(&session_name, "C-m") {
                log_debug(&format!("Failed to send C-m: {}", e));
                return ToolResponse {
                    id: id.to_string(),
                    name: "bash".to_string(),
                    content: format!("Failed to execute command in tmux: {}", e),
                    is_error: true,
                };
            }
        } else {
            log_debug("Inline approval denied. Sending C-c and exiting...");
            // User denied, send Ctrl+C to clear the session
            let _ = tmux_utils::send_control_key(&session_name, "C-c");
            return ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: "Permission Denied: command execution cancelled by user.".to_string(),
                is_error: true,
            };
        }
    } else if policy_mode != bondage::policy::PolicyMode::Yes {
        println!("⏳ Waiting for user to press Enter in terminal to run command...");
    }

    let poll_interval = std::time::Duration::from_millis(200);
    let output_content;
    let mut consecutive_detached_ticks = 0;

    log_debug("Starting command execution polling loop...");

    loop {
        tokio::time::sleep(poll_interval).await;

        let should_cancel = match &mut term_handle {
            Some(TerminalHandle::Gui(child)) => {
                child.try_wait().map(|opt| opt.is_some()).unwrap_or(false)
            }
            Some(TerminalHandle::Tty(_pane_id)) => {
                let has_clients = tmux_utils::has_attached_clients(&session_name);
                if !has_clients {
                    consecutive_detached_ticks += 1;
                    log_debug(&format!("TTY Polling: detached detected. Ticks={}", consecutive_detached_ticks));
                    consecutive_detached_ticks >= 3 // require 3 consecutive empty polls (600ms)
                } else {
                    if consecutive_detached_ticks > 0 {
                        log_debug("TTY Polling: clients attached again. Resetting ticks.");
                    }
                    consecutive_detached_ticks = 0;
                    false
                }
            }
            None => false,
        };

        if should_cancel {
            log_debug("Polling loop: cancellation detected! Sending C-c and cleaning up...");
            // Send Ctrl+C to clear the pending command line in the session
            let _ = tmux_utils::send_control_key(&session_name, "C-c");
            
            // Clean up display window/pane if still running
            if let Some(h) = term_handle {
                let _ = tmux_utils::close_terminal(h);
            }
            
            return ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: "Permission Denied: terminal window/split closed or cancelled by user.".to_string(),
                is_error: true,
            };
        }

        // Check if the command has been submitted by comparing state to initial state
        if !command_submitted {
            if let Ok(state) = tmux_utils::get_pane_cursor_state(&session_name) {
                let current_val = state.0 + state.1;
                if current_val > initial_val {
                    command_submitted = true;
                    log_debug(&format!("Polling loop: command submission detected! State: history={}, cursor_y={}, total={}", state.0, state.1, current_val));
                }
            }
        }

        // If the command is submitted, wait for the pane to return to idle
        if command_submitted {
            if let Ok(idle) = tmux_utils::is_pane_idle(&session_name) {
                if idle {
                    log_debug("Polling loop: shell is idle again! Capturing output...");
                    // Fetch current pane content
                    if let Ok(content) = tmux_utils::get_pane_content(&session_name) {
                        output_content = bondage::util::truncate_text(
                            &content,
                            10,    // N lines head
                            100,   // M lines tail
                            1200,  // N * 120 head chars
                            12000, // M * 120 tail chars
                        );
                        log_debug(&format!("Output captured ({} bytes before truncation). Loop breaking.", content.len()));
                        break;
                    }
                }
            }
        }
    }

    // 5. Cleanup the display interface, but KEEP the session running
    if let Some(h) = term_handle {
        log_debug("Cleaning up display interface...");
        let _ = tmux_utils::close_terminal(h);
    }

    log_debug("Tool execution successfully completed.");

    ToolResponse {
        id: id.to_string(),
        name: "bash".to_string(),
        content: output_content,
        is_error: false,
    }
}
