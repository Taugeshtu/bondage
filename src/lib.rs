use serde::{Serialize, Deserialize};
use futures_util::StreamExt;

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

/// The streaming step function. It calls the model, feeds text tokens to `on_token`,
/// and returns the fully assembled final message rope once the stream finishes.
pub async fn step_stream(
    client: &GenaiClient,
    model: &str,
    messages: &[Message],
    allowed_tools: &[ToolDefinition],
    options: Option<ChatOptions>,
    on_token: &(dyn Fn(String) + Send + Sync),
) -> Result<Vec<Message>, BondageError> {
    // Force capture_content in the stream options so that genai normalizes and 
    // delivers the fully assembled message/tool payload in the End event.
    let mut options = options.unwrap_or_default();
    options.capture_content = Some(true);

    let chat_req = genai::chat::ChatRequest::new(util::to_genai_messages(messages));

    let chat_req = if allowed_tools.is_empty() {
        chat_req
    } else {
        chat_req.with_tools(util::to_genai_tools(allowed_tools))
    };

    let mut response = client
        .exec_chat_stream(model, chat_req, Some(&options))
        .await
        .map_err(|e| BondageError::Engine(e.to_string()))?;

    let mut final_messages = Vec::new();

    while let Some(event_res) = response.stream.next().await {
        let event = event_res.map_err(|e| BondageError::Engine(e.to_string()))?;
        match event {
            genai::chat::ChatStreamEvent::Chunk(chunk) => {
                on_token(chunk.content);
            }
            genai::chat::ChatStreamEvent::ReasoningChunk(chunk) => {
                // Forward thinking/reasoning deltas (like DeepSeek <think> tags) to on_token
                on_token(chunk.content);
            }
            genai::chat::ChatStreamEvent::End(end) => {
                if let Some(content) = end.captured_content {
                    final_messages = util::from_genai_content(content);
                }
            }
            _ => {}
        }
    }

    Ok(final_messages)
}
