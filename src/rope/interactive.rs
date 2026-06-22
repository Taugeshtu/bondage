use std::path::{Path, PathBuf};
use std::time::Duration;
use std::io::Write;
use bondage::{Message, step_stream};
use crate::config::Config;
use crate::tmux_orchestration::execute_bash_tmux;

pub fn has_rope_trigger(content: &str) -> bool {
    let trigger = "@rope";
    let mut start = 0;
    while let Some(pos) = content[start..].find(trigger) {
        let actual_pos = start + pos;
        let left_ok = actual_pos == 0 || {
            let prev_char = content[..actual_pos].chars().last();
            prev_char.map_or(true, |c| c.is_whitespace())
        };
        let right_ok = actual_pos + trigger.len() == content.len() || {
            let next_char = content[actual_pos + trigger.len()..].chars().next();
            next_char.map_or(true, |c| c.is_whitespace())
        };
        if left_ok && right_ok {
            return true;
        }
        start = actual_pos + 1;
    }
    false
}

pub async fn run_file_sitter(
    session_file_path: PathBuf,
    config: Config,
    client: genai::Client,
    model: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let canonical_session_path = if session_file_path.exists() {
        session_file_path.canonicalize()?
    } else {
        // Create file if missing
        std::fs::write(&session_file_path, "# Rope Interactive Session\n\nWrite your instructions here. Use `@rope` to activate the agent.\n")?;
        session_file_path.canonicalize()?
    };

    println!("👁️  File-Sitter active on: {}", canonical_session_path.display());
    println!("Waiting for saves containing `@rope`...");

    let mut last_mtime = None;
    if let Ok(metadata) = std::fs::metadata(&canonical_session_path) {
        if let Ok(mtime) = metadata.modified() {
            last_mtime = Some(mtime);
        }
    }

    let poll_interval = Duration::from_millis(300);

    loop {
        tokio::time::sleep(poll_interval).await;

        let metadata = match std::fs::metadata(&canonical_session_path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let mtime = match metadata.modified() {
            Ok(t) => t,
            Err(_) => continue,
        };

        if last_mtime.is_none() {
            last_mtime = Some(mtime);
            continue;
        }

        if mtime > last_mtime.unwrap() {
            last_mtime = Some(mtime);

            let content = match std::fs::read_to_string(&canonical_session_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if has_rope_trigger(&content) {
                println!("\n⚡ [Triggered] Found `@rope` in {}", canonical_session_path.display());
                
                if let Err(e) = run_agent_turn(
                    &canonical_session_path,
                    &content,
                    &config,
                    &client,
                    &model,
                ).await {
                    println!("❌ Agent turn failed: {}", e);
                }

                // Update last_mtime after agent modification to prevent self-triggering
                if let Ok(m) = std::fs::metadata(&canonical_session_path) {
                    if let Ok(t) = m.modified() {
                        last_mtime = Some(t);
                    }
                }
                println!("⏳ Done. Waiting for next save containing `@rope`...\n");
            }
        }
    }
}

async fn run_agent_turn(
    session_file: &Path,
    file_content: &str,
    config: &Config,
    client: &genai::Client,
    model: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let current_dir = std::env::current_dir()?;
    let policy = bondage::policy::Policy::from_config(&config.policy);
    let tools = bondage::tools::get_standard_tools();

    let tools_block = bondage::util::format_tools_block(&tools);
    let system_instructions = include_str!("../../docs/system-interactive.txt")
        .replace("{SESSION_FILE}", &session_file.to_string_lossy())
        .replace("{TOOLS}", &tools_block);

    let mut history = vec![
        Message::System(system_instructions),
        Message::User(file_content.to_string()),
    ];

    println!("🤖 Invoking {}...", model);

    loop {
        let response_msgs = step_stream(client, model, &history, &tools, None, &|token| {
            print!("{}", token);
            let _ = std::io::stdout().flush();
        }).await?;

        println!();

        let mut has_tool_calls = false;
        
        for msg in response_msgs {
            history.push(msg.clone());
            
            if let Message::ModelToolRequest { id, name, arguments } = msg {
                has_tool_calls = true;
                
                let policy_mode = match name.as_str() {
                    "lookup" => {
                        if let Ok(args) = serde_json::from_str::<bondage::tools::tool_lookup::LookupArgs>(&arguments) {
                            policy.check_lookup(&args.target, &current_dir)
                        } else {
                            bondage::policy::PolicyMode::Ask
                        }
                    }
                    "write" => {
                        if let Ok(args) = serde_json::from_str::<bondage::tools::tool_write::WriteArgs>(&arguments) {
                            let resolved = resolve_path(&current_dir, &args.path);
                            // Auto-approve writes to the session file itself!
                            if resolved == session_file {
                                bondage::policy::PolicyMode::Yes
                            } else {
                                policy.check_write(&args.path, &current_dir)
                            }
                        } else {
                            bondage::policy::PolicyMode::Ask
                        }
                    }
                    "bash" => {
                        policy.check_bash()
                    }
                    _ => bondage::policy::PolicyMode::Ask,
                };

                let approved = match policy_mode {
                    bondage::policy::PolicyMode::Yes => Some(true),
                    bondage::policy::PolicyMode::No => Some(false),
                    bondage::policy::PolicyMode::Ask => {
                        if name == "bash" {
                            Some(true)
                        } else if crate::tmux_orchestration::ask_approval(&name, &arguments) {
                            Some(true)
                        } else {
                            None
                        }
                    }
                };

                if approved == Some(true) {
                    if policy_mode == bondage::policy::PolicyMode::Yes {
                        println!("\n⚡ [Auto-approved by Policy] {} ({})", name, arguments.trim());
                    }
                    println!("▶️ Executing {}...", name);
                    
                    let tool_result = if name == "bash" {
                        execute_bash_tmux(&id, &arguments, &current_dir, policy_mode, config.terminal.clone()).await
                    } else {
                        bondage::tools::execute_tool(&id, &name, &arguments, &current_dir).await
                    };
                    
                    if let Message::ToolResponse { content, is_error, .. } = &tool_result {
                        let status = if *is_error { "ERROR" } else { "SUCCESS" };
                        let preview: String = content.lines().take(5).collect::<Vec<_>>().join("\n");
                        println!("✅ [{}] Output preview:\n{}\n...", status, preview);
                    }
                    
                    history.push(tool_result);
                } else {
                    let reason = match policy_mode {
                        bondage::policy::PolicyMode::No => {
                            println!("❌ [Blocked by Policy] Rejection sent to agent.");
                            "Permission Denied: execution blocked by safety policy.".to_string()
                        }
                        _ => {
                            println!("❌ Denied.");
                            "Permission Denied by User".to_string()
                        }
                    };
                    history.push(Message::ToolResponse {
                        id,
                        name,
                        content: reason,
                        is_error: true,
                    });
                }
            }
        }

        if !has_tool_calls {
            break;
        }
    }

    Ok(())
}

fn resolve_path(base_dir: &Path, path_str: &str) -> PathBuf {
    let expanded = bondage::util::expand_tilde(Path::new(path_str));
    if expanded.is_absolute() {
        expanded
    } else {
        base_dir.join(expanded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_rope_trigger() {
        assert!(has_rope_trigger("@rope create files"));
        assert!(has_rope_trigger("do this\n@rope\nand that"));
        assert!(has_rope_trigger("some text @rope"));
        assert!(has_rope_trigger("@rope"));
        assert!(has_rope_trigger("\t@rope\t"));
        assert!(has_rope_trigger("  @rope  "));
        
        // No trigger if no whitespace around it
        assert!(!has_rope_trigger("my@rope"));
        assert!(!has_rope_trigger("@ropey"));
        assert!(!has_rope_trigger("my@ropey"));
        assert!(!has_rope_trigger("@rope?"));
        assert!(!has_rope_trigger("(@rope)"));
        assert!(!has_rope_trigger("'@rope'"));
    }
}
