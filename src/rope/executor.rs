//! Unified tool executor for Rope harnesses.
//!
//! `RopeExecutor` implements `bondage::kit::ToolExecutor` and handles the
//! rope-specific tool lifecycle: policy computation, approval flow, tmux-based
//! bash execution, and output previewing.
//!
//! In file-sitter (interactive) mode, pass `session_file: Some(path)` to
//! auto-approve writes targeting the session file itself.

use std::path::{Path, PathBuf};
use bondage::kit::ToolExecutor;
use bondage::ToolResponse;
use bondage::policy::Policy;
use crate::tmux_orchestration::{execute_bash_tmux, ask_approval};

/// Unified executor for both one-off (`main.rs`) and file-sitter (`interactive.rs`) modes.
///
/// - `session_file: None` → one-off mode: all writes go through policy
/// - `session_file: Some(path)` → file-sitter mode: writes to the session file are auto-approved
pub struct RopeExecutor {
    pub terminal: Option<String>,
    pub session_file: Option<PathBuf>,
    pub policy: Policy,
}

#[async_trait::async_trait]
impl ToolExecutor for RopeExecutor {
    async fn execute(
        &self,
        call: &bondage::ToolCall,
        current_dir: &Path,
    ) -> ToolResponse {
        let policy_mode = match call.name.as_str() {
            "lookup" => {
                if let Ok(args) = serde_json::from_str::<bondage::tools::tool_lookup::LookupArgs>(&call.arguments) {
                    self.policy.check_lookup(&args.target, current_dir)
                } else {
                    bondage::policy::PolicyMode::Ask
                }
            }
            "write" => {
                if let Ok(args) = serde_json::from_str::<bondage::tools::tool_write::WriteArgs>(&call.arguments) {
                    if let Some(ref sf) = self.session_file {
                        let resolved = bondage::util::resolve_path(current_dir, &args.path);
                        if resolved == *sf {
                            bondage::policy::PolicyMode::Yes
                        } else {
                            self.policy.check_write(&args.path, current_dir)
                        }
                    } else {
                        self.policy.check_write(&args.path, current_dir)
                    }
                } else {
                    bondage::policy::PolicyMode::Ask
                }
            }
            "bash" => self.policy.check_bash(),
            _ => bondage::policy::PolicyMode::Ask,
        };

        let approved = match policy_mode {
            bondage::policy::PolicyMode::Yes => Some(true),
            bondage::policy::PolicyMode::No => Some(false),
            bondage::policy::PolicyMode::Ask => {
                if call.name == "bash" {
                    Some(true)
                } else if ask_approval(&call.name, &call.arguments) {
                    Some(true)
                } else {
                    None
                }
            }
        };

        if approved == Some(true) {
            if policy_mode == bondage::policy::PolicyMode::Yes {
                println!("\n⚡ [Auto-approved by Policy] {} ({})", call.name, call.arguments.trim());
            }
            println!("▶️ Executing {}...", call.name);

            let tool_result = if call.name == "bash" {
                execute_bash_tmux(&call.id, &call.arguments, current_dir, policy_mode, self.terminal.clone()).await
            } else {
                bondage::tools::execute_tool(&call.id, &call.name, &call.arguments, current_dir).await
            };

            let tr = &tool_result;
            let status = if tr.is_error { "ERROR" } else { "SUCCESS" };
            let preview: String = tr.content.lines().take(5).collect::<Vec<_>>().join("\n");
            println!("✅ [{}] Output preview:\n{}\n...", status, preview);

            tool_result
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
            ToolResponse {
                id: call.id.clone(),
                name: call.name.clone(),
                content: reason,
                is_error: true,
            }
        }
    }
}
