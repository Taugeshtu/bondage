use std::io::{self, Write};
use bondage::Message;
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

pub async fn execute_bash_tmux(
    id: &str,
    arguments: &str,
    current_dir: &std::path::Path,
    policy_mode: bondage::policy::PolicyMode,
    custom_terminal: Option<String>,
) -> Message {
    let args: Result<bondage::tools::tool_bash::BashArgs, _> = serde_json::from_str(arguments);
    let command_to_run = match args {
        Ok(a) => a.command,
        Err(e) => {
            return Message::ToolResponse {
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
                return Message::ToolResponse {
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
            Ok(content) => Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content,
                is_error: false,
            },
            Err(e) => Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: e.to_string(),
                is_error: true,
            },
        };
    }

    let pid = std::process::id();
    let session_name = format!("rope-shell-{}", pid);

    // 1. Ensure tmux session exists
    if !tmux_utils::has_session(&session_name) {
        if let Err(e) = tmux_utils::start_session(&session_name, current_dir) {
            return Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: format!("Failed to start tmux session: {}", e),
                is_error: true,
            };
        }
    }

    // 2. Send the command (without enter)
    if let Err(e) = tmux_utils::send_command_literal(&session_name, &command_to_run) {
        return Message::ToolResponse {
            id: id.to_string(),
            name: "bash".to_string(),
            content: format!("Failed to send command to tmux: {}", e),
            is_error: true,
        };
    }

    // If the policy mode is "Yes" (auto-accept / yolo mode), send "Enter" right after the command
    if policy_mode == bondage::policy::PolicyMode::Yes {
        if let Err(e) = tmux_utils::send_control_key(&session_name, "C-m") {
            return Message::ToolResponse {
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
        println!("📺 Popping interactive terminal window (Alacritty / Tmux Split)...");
        term_handle = match tmux_utils::pop_terminal(&session_name, custom_terminal.as_deref()) {
            Ok(handle) => handle,
            Err(e) => {
                return Message::ToolResponse {
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
            fallback_to_inline_approval = true;
        }
    }

    // 4. Polling / Approval loop
    if fallback_to_inline_approval {
        if ask_approval("bash", &arguments) {
            // User approved, send Enter key to run the command
            if let Err(e) = tmux_utils::send_control_key(&session_name, "C-m") {
                return Message::ToolResponse {
                    id: id.to_string(),
                    name: "bash".to_string(),
                    content: format!("Failed to execute command in tmux: {}", e),
                    is_error: true,
                };
            }
        } else {
            // User denied, send Ctrl+C to clear the session
            let _ = tmux_utils::send_control_key(&session_name, "C-c");
            return Message::ToolResponse {
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

    loop {
        tokio::time::sleep(poll_interval).await;

        let should_cancel = match &mut term_handle {
            Some(TerminalHandle::Gui(child)) => {
                child.try_wait().map(|opt| opt.is_some()).unwrap_or(false)
            }
            Some(TerminalHandle::Tty(_pane_id)) => {
                !tmux_utils::has_attached_clients(&session_name)
            }
            None => false,
        };

        if should_cancel {
            // Send Ctrl+C to clear the pending command line in the session
            let _ = tmux_utils::send_control_key(&session_name, "C-c");
            
            // Clean up display window/pane if still running
            if let Some(h) = term_handle {
                let _ = tmux_utils::close_terminal(h);
            }
            
            return Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: "Permission Denied: terminal window/split closed or cancelled by user.".to_string(),
                is_error: true,
            };
        }

        // Fetch current pane content
        if let Ok(content) = tmux_utils::get_pane_content(&session_name) {
            let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
            if let Some(&last_line) = lines.last() {
                if let Ok(idle) = tmux_utils::is_pane_idle(&session_name) {
                    let trimmed_cmd = command_to_run.trim();
                    let last_cmd_line = trimmed_cmd.lines().last().unwrap_or("");
                    if idle && !last_line.trim().ends_with(last_cmd_line) {
                        output_content = bondage::util::truncate_text(
                            &content,
                            10,    // N lines head
                            100,   // M lines tail
                            1200,  // N * 120 head chars
                            12000, // M * 120 tail chars
                        );
                        break;
                    }
                }
            }
        }
    }

    // 5. Cleanup the display interface, but KEEP the session running
    if let Some(h) = term_handle {
        let _ = tmux_utils::close_terminal(h);
    }

    Message::ToolResponse {
        id: id.to_string(),
        name: "bash".to_string(),
        content: output_content,
        is_error: false,
    }
}
