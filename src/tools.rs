use crate::{Message, ToolDefinition, ToolContext, BondageError};

pub mod tool_lookup;

/// Unified router to execute a tool by name with raw JSON arguments
pub async fn execute_tool(
    id: &str,
    name: &str,
    arguments: &str,
    context: &ToolContext,
) -> Message {
    let result = match name {
        "lookup" => match serde_json::from_str::<tool_lookup::LookupArgs>(arguments) {
            Ok(args) => tool_lookup::execute(args, context).await,
            Err(e) => Err(BondageError::Serialization(e.to_string())),
        },
        other => Err(BondageError::Tool(format!("Unknown tool: {}", other))),
    };

    match result {
        Ok(content) => Message::ToolResponse {
            id: id.to_string(),
            name: name.to_string(),
            content,
            is_error: false,
        },
        Err(err) => Message::ToolResponse {
            id: id.to_string(),
            name: name.to_string(),
            content: err.to_string(),
            is_error: true,
        },
    }
}

/// Returns the schemas of all registered tools in the index
pub fn get_standard_tools() -> Vec<ToolDefinition> {
    vec![
        tool_lookup::definition(),
    ]
}
