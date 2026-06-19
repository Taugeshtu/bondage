use std::path::PathBuf;
use serde::{Serialize, Deserialize};

pub mod tools;
pub mod util;

// Re-export GenAI's client types directly so library consumers 
// can pass them to step without wrappers.
pub use genai::{Client as GenaiClient, chat::ChatOptions};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Message {
    System(String),
    User(String),
    ModelText(String),
    ModelToolRequest {
        id: String,
        name: String,
        arguments: String, // JSON payload string
    },
    ToolResponse {
        id: String,
        name: String,
        content: String,
        is_error: bool,
    },
    Error(String), // Model-level generation failures (e.g. content block)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema representation of arguments
}

#[derive(Debug)]
pub enum BondageError {
    Engine(String),
    Tool(String),
    Io(String),
    Serialization(String),
}

impl std::fmt::Display for BondageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BondageError::Engine(err) => write!(f, "LLM Engine Error: {}", err),
            BondageError::Tool(err) => write!(f, "Tool Execution Error: {}", err),
            BondageError::Io(err) => write!(f, "I/O Error: {}", err),
            BondageError::Serialization(err) => write!(f, "Serialization Error: {}", err),
        }
    }
}

impl std::error::Error for BondageError {}

/// Boundary and state configuration for tool execution
pub struct ToolContext {
    pub workspace_root: PathBuf,
    pub current_dir: PathBuf,
}

/// The core step function. It converts our stateless message rope 
/// to GenAI types, executes the chat, and parses the response back.
pub async fn step(
    client: &GenaiClient,
    model: &str,
    messages: &[Message],
    allowed_tools: &[ToolDefinition],
    options: Option<ChatOptions>,
) -> Result<Vec<Message>, BondageError> {
    let chat_req = genai::chat::ChatRequest::new(util::to_genai_messages(messages));

    let chat_req = if allowed_tools.is_empty() {
        chat_req
    } else {
        chat_req.with_tools(util::to_genai_tools(allowed_tools))
    };

    let response = client
        .exec_chat(model, chat_req, options.as_ref())
        .await
        .map_err(|e| BondageError::Engine(e.to_string()))?;

    Ok(util::from_genai_response(response))
}
