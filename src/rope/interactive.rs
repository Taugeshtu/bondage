use std::path::{Path, PathBuf};
use std::time::Duration;
use std::io::Write;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use bondage::Message;
use bondage::kit::step_agent;
use crate::config::Config;
use crate::executor::RopeExecutor;

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

    // Initialize to "epoch + no hash" so that the first poll cycle
    // treats the existing file content as "new" and checks for @rope.
    // This enables auto-fire on launch if the session file already
    // contains @rope triggers — no need for the user to re-save.
    let mut last_mtime = Some(std::time::SystemTime::UNIX_EPOCH);
    let mut last_content_hash: Option<u64> = None;

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

    let executor = RopeExecutor {
        terminal: config.terminal.clone(),
        session_file: Some(session_file.to_path_buf()),
    };
    step_agent(client, model, &mut history, &tools, None, &current_dir, &policy, &executor, &|token| {
        print!("{}", token);
        let _ = std::io::stdout().flush();
    }).await?;

    Ok(())
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
