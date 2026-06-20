use serde::Deserialize;
use crate::{ToolDefinition, BondageError};

#[derive(Deserialize)]
pub struct BashArgs {
    pub command: String,
}

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "bash".to_string(),
        description: "Execute a command in a bash shell. Returns stdout, stderr, and exit status code. Runs in a non-interactive one-off context.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to run."
                }
            },
            "required": ["command"]
        }),
    }
}

pub async fn execute(args: BashArgs, base_dir: &std::path::Path) -> Result<String, BondageError> {
    let output = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(&args.command)
        .current_dir(base_dir)
        .output()
        .await
        .map_err(|e| BondageError::Io(format!("Failed to execute command: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let status = output.status.code().unwrap_or(-1);

    let result = format!(
        "<bash_result status_code=\"{}\">\n<stdout>\n{}\n</stdout>\n<stderr>\n{}\n</stderr>\n</bash_result>",
        status, stdout, stderr
    );

    Ok(result)
}
