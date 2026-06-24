//! Bondage Kit — ergonomics layer for writing agent harnesses.
//!
//! Sits one level above `step_stream`: provides `step_agent`, a full agentic
//! turn loop (stream → extract tool calls → delegate execution → repeat) with
//! **pluggable tool execution** via the `ToolExecutor` trait.
//!
//! The kit is not the harness. It doesn't know about Config, tmux, or terminal
//! rendering. Bring your own `ToolExecutor` — or use `DefaultExecutor` for a
//! simple stdin-based CLI experience.

use std::io::{self, Write};
use std::path::Path;
use async_trait::async_trait;
use crate::{
    Message, ToolCall, ToolDefinition, ToolResponse, BondageError, step_stream,
    policy::Policy,
    tools::{execute_tool, tool_lookup::LookupArgs, tool_write::WriteArgs},
};
use crate::ChatOptions;

/// Trait: the harness provides this. It owns the entire tool lifecycle:
///   1. Compute policy mode for this tool call
///   2. Handle approval (auto-approve for Yes, block for No, ask for Ask)
///   3. Execute the tool (bring your own bash impl, or use bondage's execute_tool)
///   4. Return a `ToolResponse` (success or error)
///
/// The executor is responsible for all console output related to tool execution
/// (approval messages, status prints, output previews). `step_agent` only handles
/// streaming token output via `on_token`.
///
/// **The executor owns its policy.** Construct the executor with the desired
/// `Policy` — it is not passed per-call. This makes the executor self-contained
/// and allows different executors to enforce different policies.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute a tool call. Must always return `ToolResponse`.
    /// Catch all errors internally and return them as `is_error: true` responses.
    async fn execute(
        &self,
        call: &ToolCall,
        current_dir: &Path,
    ) -> ToolResponse;
}

/// The kit's agentic loop. Calls `step_stream` in a loop, extracts tool calls,
/// delegates each to the `executor`, appends results to `history`, and repeats
/// until the model produces no tool calls.
///
/// The `on_token` callback receives streaming text tokens (same as `step_stream`).
/// The `executor` owns the entire tool lifecycle: policy, approval, execution, output.
///
/// Pass `options: None` to use the engine's default ChatOptions, or `Some(ChatOptions{...})`
/// to control temperature, max tokens, etc.
pub async fn step_agent(
    client: &genai::Client,
    model: &str,
    history: &mut Vec<Message>,
    tools: &[ToolDefinition],
    options: Option<ChatOptions>,
    current_dir: &Path,
    executor: &dyn ToolExecutor,
    on_token: &(dyn Fn(String) + Send + Sync),
) -> Result<(), BondageError> {
    loop {
        let response_msgs = step_stream(
            client, model, history, tools, options.clone(), on_token,
        ).await?;

        let mut has_tool_calls = false;
        for msg in response_msgs {
            history.push(msg.clone());
            if let Message::ModelToolRequest(call) = msg {
                has_tool_calls = true;
                let result = executor.execute(&call, current_dir).await;
                history.push(Message::ToolResponse(result));
            }
        }
        if !has_tool_calls {
            break;
        }
    }
    Ok(())
}

/// Simple stdin-based approval prompt. Returns `true` if user types y/yes.
fn stdin_ask_approval(tool_name: &str, args: &str) -> bool {
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

/// Default executor — uses bondage's built-in `execute_tool` for all tools,
/// and a simple stdin prompt for `Ask` mode. Bash is auto-approved in `Ask`
/// mode (matching existing rope behavior where bash runs in tmux with its own
/// approval mechanism).
///
/// For a richer experience (tmux integration, session-file auto-approve, etc.),
/// implement `ToolExecutor` yourself.
pub struct DefaultExecutor {
    pub policy: Policy,
}

impl DefaultExecutor {
    /// Create a new `DefaultExecutor` with the given policy.
    pub fn new(policy: Policy) -> Self {
        DefaultExecutor { policy }
    }
}

#[async_trait]
impl ToolExecutor for DefaultExecutor {
    async fn execute(
        &self,
        call: &ToolCall,
        current_dir: &Path,
    ) -> ToolResponse {
        let mode = match call.name.as_str() {
            "lookup" => {
                if let Ok(args) = serde_json::from_str::<LookupArgs>(&call.arguments) {
                    self.policy.check_lookup(&args.target, current_dir)
                } else {
                    crate::policy::PolicyMode::Ask
                }
            }
            "write" => {
                if let Ok(args) = serde_json::from_str::<WriteArgs>(&call.arguments) {
                    self.policy.check_write(&args.path, current_dir)
                } else {
                    crate::policy::PolicyMode::Ask
                }
            }
            "bash" => self.policy.check_bash(),
            _ => crate::policy::PolicyMode::Ask,
        };

        match mode {
            crate::policy::PolicyMode::No => {
                println!("❌ [Blocked by Policy] Rejection sent to agent.");
                ToolResponse {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    content: "Permission Denied: execution blocked by safety policy.".to_string(),
                    is_error: true,
                }
            }
            crate::policy::PolicyMode::Yes => {
                println!("\n⚡ [Auto-approved by Policy] {} ({})", call.name, call.arguments.trim());
                println!("▶️ Executing {}...", call.name);
                let result = execute_tool(&call.id, &call.name, &call.arguments, current_dir).await;
                {
                    let status = if result.is_error { "ERROR" } else { "SUCCESS" };
                    let preview: String = result.content.lines().take(5).collect::<Vec<_>>().join("\n");
                    println!("✅ [{}] Output preview:\n{}\n...", status, preview);
                }
                result
            }
            crate::policy::PolicyMode::Ask => {
                // Bash auto-yes in Ask mode (matching rope behavior — bash has its
                // own approval mechanism when run through tmux).
                let approved = if call.name == "bash" {
                    true
                } else {
                    stdin_ask_approval(&call.name, &call.arguments)
                };

                if approved {
                    println!("▶️ Executing {}...", call.name);
                    let result = execute_tool(&call.id, &call.name, &call.arguments, current_dir).await;
                    {
                        let status = if result.is_error { "ERROR" } else { "SUCCESS" };
                        let preview: String = result.content.lines().take(5).collect::<Vec<_>>().join("\n");
                        println!("✅ [{}] Output preview:\n{}\n...", status, preview);
                    }
                    result
                } else {
                    println!("❌ Denied.");
                    ToolResponse {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        content: "Permission Denied by User".to_string(),
                        is_error: true,
                    }
                }
            }
        }
    }
}
