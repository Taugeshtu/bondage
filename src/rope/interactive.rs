use std::path::{Path, PathBuf};
use std::time::Duration;
use std::io::Write;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use bondage::{Message, step_stream};
use crate::config::Config;
use crate::tmux_orchestration::execute_bash_tmux;

/// Compute a fast (non-cryptographic) hash of file content for change detection.
fn content_hash(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

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
    system_prompt_template: String,
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
    let mut last_content_hash: Option<u64> = None;
    if let Ok(metadata) = std::fs::metadata(&canonical_session_path) {
        if let Ok(mtime) = metadata.modified() {
            last_mtime = Some(mtime);
            if let Ok(content) = std::fs::read_to_string(&canonical_session_path) {
                last_content_hash = Some(content_hash(&content));
            }
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
            // mtime changed — read content and check hash to avoid
            // triggering on no-op saves or self-writes with same content.
            let content = match std::fs::read_to_string(&canonical_session_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let current_hash = content_hash(&content);

            // Update mtime regardless so we don't re-read every poll,
            // but only trigger the agent if content actually changed.
            last_mtime = Some(mtime);

            if last_content_hash.is_some() && current_hash == last_content_hash.unwrap() {
                // Content unchanged (e.g. no-op save, or metadata-only touch).
                continue;
            }

            // Content changed — update hash before running agent so we
            // have a baseline. After the agent finishes, we'll re-snapshot
            // both mtime and hash from whatever the agent wrote.
            last_content_hash = Some(current_hash);

            if has_rope_trigger(&content) {
                println!("\n⚡ [Triggered] Found `@rope` in {}", canonical_session_path.display());
                
                if let Err(e) = run_agent_turn(
                    &canonical_session_path,
                    &content,
                    &config,
                    &client,
                    &model,
                    &system_prompt_template,
                ).await {
                    println!("❌ Agent turn failed: {}", e);
                }

                // ── Post-turn snapshot ──────────────────────────────────
                // Re-read mtime AND content hash AFTER the agent is done.
                // This is critical: the agent may have written the session
                // file during its turn. We snapshot whatever state exists
                // now so that subsequent triggers only fire on *new* user
                // edits — not on the agent's own writes.
                if let Ok(m) = std::fs::metadata(&canonical_session_path) {
                    if let Ok(t) = m.modified() {
                        last_mtime = Some(t);
                    }
                }
                if let Ok(post_content) = std::fs::read_to_string(&canonical_session_path) {
                    last_content_hash = Some(content_hash(&post_content));
                }
                // ── End post-turn snapshot ──────────────────────────────
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
    system_prompt_template: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let current_dir = std::env::current_dir()?;
    let policy = bondage::policy::Policy::from_config(&config.policy);
    let tools = bondage::tools::get_standard_tools();

    let tools_block = bondage::util::format_tools_block(&tools);
    let session_file_name = session_file.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "session.md".to_string());

    let system_instructions = system_prompt_template
        .replace("{SESSION_FILE_NAME}", &session_file_name)
        .replace("{SESSION_FILE_CONTENT}", file_content)
        .replace("{TOOLS}", &tools_block);

    let user_message = format!(
        "Here is the content of the session file '{}':\n---\n{}\n---",
        session_file_name,
        file_content
    );

    let mut history = vec![
        Message::System(system_instructions),
        Message::User(user_message),
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
