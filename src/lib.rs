use std::path::PathBuf;
use serde::{Serialize, Deserialize};

pub mod tools;

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
    Error(String), // Model-level generation failures (e.g. context limit, content block)
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

#[async_trait::async_trait]
pub trait LlmEngine {
    async fn generate(
        &self,
        messages: &[Message],
        allowed_tools: &[ToolDefinition],
    ) -> Result<Vec<Message>, BondageError>;
}

/// Boundary and state configuration for tool execution
pub struct ToolContext {
    pub workspace_root: PathBuf,
    pub current_dir: PathBuf,
}

/// The core step function. It is a stateless wrapper that delegates 
/// model invocation to the supplied engine.
pub async fn step(
    engine: &dyn LlmEngine,
    messages: &[Message],
    allowed_tools: &[ToolDefinition],
) -> Result<Vec<Message>, BondageError> {
    engine.generate(messages, allowed_tools).await
}
