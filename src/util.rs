use crate::{Message, ToolDefinition, ToolCall as BondageToolCall};
use genai::chat::{ChatMessage, Tool, ToolCall as GenaiToolCall, ToolResponse as GenaiToolResponse, MessageContent, ContentPart};

/// Group consecutive ModelToolRequests into a single assistant message with parallel tool calls,
/// and map all other message variants to their genai equivalents.
pub fn to_genai_messages(messages: &[Message]) -> Vec<ChatMessage> {
    let mut chat_msgs = Vec::new();
    let mut pending_tool_calls = Vec::new();

    for msg in messages {
        // If we have pending tool calls from a previous iteration and the current message 
        // is NOT another tool call, we must commit them as one assistant message first.
        if !matches!(msg, Message::ModelToolRequest(_)) && !pending_tool_calls.is_empty() {
            let calls = std::mem::take(&mut pending_tool_calls);
            chat_msgs.push(ChatMessage::from(calls));
        }

        match msg {
            Message::System(text) => {
                chat_msgs.push(ChatMessage::system(text.clone()));
            }
            Message::User(text) => {
                chat_msgs.push(ChatMessage::user(text.clone()));
            }
            Message::ModelText(text) => {
                chat_msgs.push(ChatMessage::assistant(text.clone()));
            }
            Message::ModelToolRequest(call) => {
                let args_val = serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
                pending_tool_calls.push(GenaiToolCall {
                    call_id: call.id.clone(),
                    fn_name: call.name.clone(),
                    fn_arguments: args_val,
                    thought_signatures: None,
                });
            }
            Message::ToolResponse(tr) => {
                let tool_res = GenaiToolResponse::new(tr.id.clone(), tr.content.clone());
                chat_msgs.push(ChatMessage::from(tool_res));
            }
            Message::Error(text) => {
                // Represent generation failure as an assistant message containing the error
                chat_msgs.push(ChatMessage::assistant(format!("Error: {}", text)));
            }
        }
    }

    // Commit any trailing parallel tool calls
    if !pending_tool_calls.is_empty() {
        chat_msgs.push(ChatMessage::from(pending_tool_calls));
    }

    chat_msgs
}

/// Translate our clean ToolDefinitions to genai's Tool metadata format
pub fn to_genai_tools(tools: &[ToolDefinition]) -> Vec<Tool> {
    tools
        .iter()
        .map(|t| {
            Tool::new(t.name.clone())
                .with_description(t.description.clone())
                .with_schema(t.parameters.clone())
        })
        .collect()
}

/// Convert genai's MessageContent enum variant back into our stateless Message rope
pub fn from_genai_content(content: MessageContent) -> Vec<Message> {
    let mut messages = Vec::new();
    let mut text_acc = String::new();
    let mut tool_calls = Vec::new();

    for part in content.into_parts() {
        match part {
            ContentPart::Text(text) => {
                text_acc.push_str(&text);
            }
            ContentPart::ToolCall(call) => {
                tool_calls.push(call);
            }
            _ => {}
        }
    }

    if !text_acc.is_empty() {
        messages.push(Message::ModelText(text_acc));
    }

    for call in tool_calls {
        messages.push(Message::ModelToolRequest(BondageToolCall {
            id: call.call_id,
            name: call.fn_name,
            arguments: call.fn_arguments.to_string(),
        }));
    }

    messages
}

/// Convert genai's response object back into our stateless Message rope
pub fn from_genai_response(res: genai::chat::ChatResponse) -> Vec<Message> {
    from_genai_content(res.content)
}

/// Truncate text by keeping the first N lines (and head_chars) and the last M lines (and tail_chars).
/// Returns the text as-is if it's within both limits or if limits overlap.
pub fn truncate_text(
    content: &str,
    head_lines: usize,
    tail_lines: usize,
    head_chars: usize,
    tail_chars: usize,
) -> String {
    let total_chars = content.chars().count();
    let total_bytes = content.len();

    // Find byte indices of line starts
    // A line starts at index 0, and right after every '\n'
    let mut line_starts = vec![0];
    for (idx, c) in content.char_indices() {
        if c == '\n' {
            let next_idx = idx + 1;
            if next_idx < total_bytes {
                line_starts.push(next_idx);
            }
        }
    }
    let total_lines = line_starts.len();

    // If total size is within both limits, return as-is
    if total_lines <= head_lines + tail_lines && total_chars <= head_chars + tail_chars {
        return content.to_string();
    }

    // 1. Determine Head End Byte Index (Strictest of line vs char limits)
    let head_line_end_byte = if head_lines < total_lines {
        line_starts[head_lines]
    } else {
        total_bytes
    };

    let head_char_end_byte = if head_chars < total_chars {
        content.char_indices().nth(head_chars).map(|(idx, _)| idx).unwrap_or(total_bytes)
    } else {
        total_bytes
    };

    let head_end_byte = head_line_end_byte.min(head_char_end_byte);

    // 2. Determine Tail Start Byte Index (Strictest of line vs char limits)
    let tail_line_start_byte = if tail_lines < total_lines {
        line_starts[total_lines - tail_lines]
    } else {
        0
    };

    let tail_char_start_byte = if tail_chars < total_chars {
        let skip_chars = total_chars - tail_chars;
        content.char_indices().nth(skip_chars).map(|(idx, _)| idx).unwrap_or(0)
    } else {
        0
    };

    let tail_start_byte = tail_line_start_byte.max(tail_char_start_byte);

    // If limits overlap, no gap exists to truncate
    if head_end_byte >= tail_start_byte {
        return content.to_string();
    }

    // Calculate omitted statistics
    let omitted_part = &content[head_end_byte..tail_start_byte];
    let omitted_lines = omitted_part.chars().filter(|&c| c == '\n').count();
    let omitted_bytes = omitted_part.len();

    let head = &content[..head_end_byte];
    let tail = &content[tail_start_byte..];

    // Reassemble cleanly (avoiding duplicate newlines)
    let mut result = String::with_capacity(head.len() + tail.len() + 64);
    result.push_str(head);
    if !head.is_empty() && !head.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(&format!(
        "[... {} lines and {} bytes omitted ...]",
        omitted_lines, omitted_bytes
    ));
    if !tail.is_empty() && !tail.starts_with('\n') {
        result.push('\n');
    }
    result.push_str(tail);

    result
}

/// Helper to construct a tools block for injection into system prompts.
pub fn format_tools_block(tools: &[crate::ToolDefinition]) -> String {
    let mut block = String::new();
    for tool in tools {
        block.push_str(&format!("- {}: {}\n", tool.name, tool.description));
    }
    block.trim_end().to_string()
}

/// Unified resource finder logic.
/// Resolves a resource name or path:
/// 1. If explicit, checks CWD (base_dir), then checks `~/.config/rope/`. Returns error if missing.
/// 2. If implicit (e.g. default configuration or prompts when not specified), checks `~/.config/rope/` only.
pub fn locate_resource(
    name: &str,
    is_explicit: bool,
    base_dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let config_dir = std::path::PathBuf::from(&home).join(".config/rope");

    let candidate = std::path::Path::new(name);
    let expanded = expand_tilde(candidate);

    if is_explicit {
        if expanded.is_absolute() {
            if expanded.exists() {
                return Ok(expanded);
            }
        } else {
            let cwd_joined = base_dir.join(&expanded);
            if cwd_joined.exists() {
                return Ok(cwd_joined);
            }
        }

        if !expanded.is_absolute() {
            let config_joined = config_dir.join(&expanded);
            if config_joined.exists() {
                return Ok(config_joined);
            }
        }

        Err(format!(
            "Resource '{}' not found in CWD ({}) or ~/.config/rope/",
            name,
            base_dir.display()
        ))
    } else {
        if expanded.is_absolute() {
            if expanded.exists() {
                return Ok(expanded);
            }
        } else {
            let config_joined = config_dir.join(&expanded);
            if config_joined.exists() {
                return Ok(config_joined);
            }
        }
        Err(format!(
            "Default resource '{}' not found in ~/.config/rope/",
            name
        ))
    }
}

/// Expands a path starting with tilde `~` to the absolute path using the `HOME` environment variable.
pub fn expand_tilde(path: &std::path::Path) -> std::path::PathBuf {
    let path_str = path.to_string_lossy();
    if path_str == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home);
        }
    } else if path_str.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            let mut new_path = std::path::PathBuf::from(home);
            new_path.push(&path_str[2..]);
            return new_path;
        }
    }
    path.to_path_buf()
}

/// Resolves a path string (which may be relative or contain `~`) to an absolute path.
/// Relative paths are joined against `base_dir`.
pub fn resolve_path(base_dir: &std::path::Path, path_str: &str) -> std::path::PathBuf {
    let expanded = expand_tilde(std::path::Path::new(path_str));
    if expanded.is_absolute() {
        expanded
    } else {
        base_dir.join(expanded)
    }
}

