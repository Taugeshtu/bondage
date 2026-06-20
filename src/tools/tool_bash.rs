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

    let stdout_raw = String::from_utf8_lossy(&output.stdout);
    let stderr_raw = String::from_utf8_lossy(&output.stderr);
    let status = output.status.code().unwrap_or(-1);

    let stdout = crate::util::truncate_text(&stdout_raw, 10, 100, 1200, 12000);
    let stderr = crate::util::truncate_text(&stderr_raw, 10, 100, 1200, 12000);

    let result = format!(
        "<bash_result status_code=\"{}\">\n<stdout>\n{}\n</stdout>\n<stderr>\n{}\n</stderr>\n</bash_result>",
        status, stdout, stderr
    );

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bash_execute_truncation() {
        let temp_dir = std::env::temp_dir();
        // Generate 200 lines of output
        let cmd = "for i in {1..200}; do echo \"line $i\"; done".to_string();
        let args = BashArgs { command: cmd };
        let res = execute(args, &temp_dir).await.unwrap();

        assert!(res.contains("<stdout>"));
        assert!(res.contains("line 1\n"));
        assert!(res.contains("line 200"));
        assert!(res.contains("lines and"));
        assert!(res.contains("bytes omitted"));
    }
}
