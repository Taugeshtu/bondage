use crate::{Message, ToolDefinition};
use genai::chat::{ChatMessage, Tool, ToolCall, ToolResponse, MessageContent, ContentPart};

/// Group consecutive ModelToolRequests into a single assistant message with parallel tool calls,
/// and map all other message variants to their genai equivalents.
pub fn to_genai_messages(messages: &[Message]) -> Vec<ChatMessage> {
    let mut chat_msgs = Vec::new();
    let mut pending_tool_calls = Vec::new();

    for msg in messages {
        // If we have pending tool calls from a previous iteration and the current message 
        // is NOT another tool call, we must commit them as one assistant message first.
        if !matches!(msg, Message::ModelToolRequest { .. }) && !pending_tool_calls.is_empty() {
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
            Message::ModelToolRequest { id, name, arguments } => {
                let args_val = serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null);
                pending_tool_calls.push(ToolCall {
                    call_id: id.clone(),
                    fn_name: name.clone(),
                    fn_arguments: args_val,
                });
            }
            Message::ToolResponse { id, name: _, content, is_error: _ } => {
                let tool_res = ToolResponse::new(id.clone(), content.clone());
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

/// Convert genai's response object back into our stateless Message rope
pub fn from_genai_response(res: genai::chat::ChatResponse) -> Vec<Message> {
    let mut messages = Vec::new();

    if let Some(content) = res.content {
        match content {
            MessageContent::Text(text) => {
                messages.push(Message::ModelText(text));
            }
            MessageContent::Parts(parts) => {
                let mut text_acc = String::new();
                for part in parts {
                    if let ContentPart::Text(text) = part {
                        text_acc.push_str(&text);
                    }
                }
                if !text_acc.is_empty() {
                    messages.push(Message::ModelText(text_acc));
                }
            }
            MessageContent::ToolCalls(tool_calls) => {
                for call in tool_calls {
                    messages.push(Message::ModelToolRequest {
                        id: call.call_id,
                        name: call.fn_name,
                        arguments: call.fn_arguments.to_string(),
                    });
                }
            }
            MessageContent::ToolResponses(_) => {
                // LLM responses never contain raw ToolResponses (only ToolCalls)
            }
        }
    }

    messages
}
