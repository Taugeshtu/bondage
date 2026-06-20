use std::process::{Command, Child};
use std::path::Path;
use std::env;

pub enum TerminalHandle {
    Gui(Child),
    Tty(String), // Tmux Pane ID
}

/// Check if a tmux session exists
pub fn has_session(session_name: &str) -> bool {
    let output = Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .output();
    
    match output {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

/// Create a new headless tmux session
pub fn start_session(session_name: &str, current_dir: &Path) -> std::io::Result<()> {
    Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-c",
            &current_dir.to_string_lossy(),
        ])
        .status()?;
    Ok(())
}

/// Send a command to the tmux session without executing it (no enter key)
pub fn send_command(session_name: &str, command: &str) -> std::io::Result<()> {
    Command::new("tmux")
        .args(["send-keys", "-t", session_name, command])
        .status()?;
    Ok(())
}

/// Capture the current buffer text of the tmux pane, logically joining wrapped lines
pub fn get_pane_content(session_name: &str) -> std::io::Result<String> {
    let output = Command::new("tmux")
        .args(["capture-pane", "-p", "-J", "-t", session_name])
        .output()?;
    
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Check if the active process in the pane is a shell (idle)
pub fn is_pane_idle(session_name: &str) -> std::io::Result<bool> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "-F", "#{pane_current_command}", "-t", session_name])
        .output()?;
    
    let cmd = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
    // Common shells
    Ok(cmd == "bash" || cmd == "zsh" || cmd == "fish" || cmd == "sh" || cmd == "tmux")
}

/// Kill the tmux session
pub fn kill_session(session_name: &str) -> std::io::Result<()> {
    Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .status()?;
    Ok(())
}

/// Check if any client is currently attached to the tmux session
pub fn has_attached_clients(session_name: &str) -> bool {
    let output = Command::new("tmux")
        .args(["list-clients", "-t", session_name])
        .output();
    
    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            !text.trim().is_empty()
        }
        Err(_) => false,
    }
}

/// Spawns a terminal GUI (Alacritty) or splits the tmux window to attach to the session
pub fn pop_terminal(session_name: &str) -> std::io::Result<Option<TerminalHandle>> {
    let is_gui = env::var("WAYLAND_DISPLAY").is_ok() || env::var("DISPLAY").is_ok();
    
    if is_gui {
        // Spawn Alacritty window and attach (unsetting TMUX to avoid nesting errors)
        let child = Command::new("alacritty")
            .args(["-e", "tmux", "attach-session", "-t", session_name])
            .env_remove("TMUX")
            .spawn()?;
        Ok(Some(TerminalHandle::Gui(child)))
    } else if env::var("TMUX").is_ok() {
        // If we are inside tmux down in a raw TTY console, split pane horizontally, print pane ID, and attach
        if let Ok(pane_id) = env::var("TMUX_PANE") {
            let output = Command::new("tmux")
                .args([
                    "split-window",
                    "-P",
                    "-F",
                    "#{pane_id}",
                    "-h",
                    "-t",
                    &pane_id,
                    &format!("env -u TMUX tmux attach-session -t {}", session_name),
                ])
                .output()?;
            let new_pane_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(Some(TerminalHandle::Tty(new_pane_id)));
        }
        Ok(None)
    } else {
        // No GUI and not in tmux, cannot pop terminal.
        println!("⚠️  Warning: Cannot pop window. No GUI display or TMUX environment detected.");
        Ok(None)
    }
}

/// Close the popped terminal GUI window or TTY split pane without killing the session itself
pub fn close_terminal(handle: TerminalHandle) -> std::io::Result<()> {
    match handle {
        TerminalHandle::Gui(mut child) => {
            let _ = child.kill();
        }
        TerminalHandle::Tty(pane_id) => {
            Command::new("tmux")
                .args(["kill-pane", "-t", &pane_id])
                .status()?;
        }
    }
    Ok(())
}
