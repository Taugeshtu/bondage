use std::path::{Path, PathBuf};
use serde::Deserialize;
use crate::{ToolDefinition, BondageError};

#[derive(Deserialize)]
pub struct WriteArgs {
    pub path: String,
    pub new_content: String,
    pub old_content: Option<String>,
}

fn resolve_path(base_dir: &Path, path_str: &str) -> PathBuf {
    let expanded = crate::util::expand_tilde(Path::new(path_str));
    if expanded.is_absolute() {
        expanded
    } else {
        base_dir.join(expanded)
    }
}

pub async fn execute(args: WriteArgs, base_dir: &Path) -> Result<String, BondageError> {
    let resolved = resolve_path(base_dir, &args.path);
    
    if let Some(old) = args.old_content {
        // Mode: Patch / Substring replace
        if !resolved.exists() {
            return Ok(format!(
                "<write path=\"{}\" status=\"error\" reason=\"File does not exist for patching\" />",
                args.path
            ));
        }

        let content = std::fs::read_to_string(&resolved)
            .map_err(|e| BondageError::Io(format!("Failed to read file: {}", e)))?;

        let occurrences: Vec<_> = content.match_indices(&old).collect();
        if occurrences.is_empty() {
            return Ok(format!(
                "<write path=\"{}\" status=\"error\" reason=\"old_content not found in the file\" />",
                args.path
            ));
        }

        if occurrences.len() > 1 {
            return Ok(format!(
                "<write path=\"{}\" status=\"error\" reason=\"old_content is not unique in the file (found {} occurrences)\" />",
                args.path,
                occurrences.len()
            ));
        }

        // Replace the single occurrence
        let updated = content.replacen(&old, &args.new_content, 1);
        std::fs::write(&resolved, updated)
            .map_err(|e| BondageError::Io(format!("Failed to write file: {}", e)))?;

        Ok(format!(
            "<write path=\"{}\" status=\"success\" mode=\"patch\" />",
            args.path
        ))
    } else {
        // Mode: Full replace / overwrite
        // Create parent directories if missing
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| BondageError::Io(format!("Failed to create parent directories: {}", e)))?;
        }

        std::fs::write(&resolved, &args.new_content)
            .map_err(|e| BondageError::Io(format!("Failed to write file: {}", e)))?;

        Ok(format!(
            "<write path=\"{}\" status=\"success\" mode=\"overwrite\" />",
            args.path
        ))
    }
}

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "write".to_string(),
        description: "Write content to a file. Overwrite the entire file, or patch a specific unique section.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "The destination path of the file." },
                "new_content": { "type": "string", "description": "The text content to write to the file (or the replacement text block)." },
                "old_content": { "type": "string", "description": "Optional. The exact, unique substring block of text in the file to be replaced. If omitted, the tool performs a full file overwrite." }
            },
            "required": ["path", "new_content"]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn setup_workspace() -> PathBuf {
        let counter = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp_dir = std::env::temp_dir().join(format!("bondage_test_write_{}", counter));
        fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    fn teardown_workspace(dir: PathBuf) {
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_write_overwrite() {
        let dir = setup_workspace();
        let args = WriteArgs {
            path: "test.txt".to_string(),
            new_content: "hello world".to_string(),
            old_content: None,
        };

        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("status=\"success\""));
        assert!(res.contains("mode=\"overwrite\""));

        let file_content = fs::read_to_string(dir.join("test.txt")).unwrap();
        assert_eq!(file_content, "hello world");

        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_write_overwrite_creates_dirs() {
        let dir = setup_workspace();
        let args = WriteArgs {
            path: "subdir/nested/test.txt".to_string(),
            new_content: "hello nested world".to_string(),
            old_content: None,
        };

        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("status=\"success\""));

        let file_content = fs::read_to_string(dir.join("subdir/nested/test.txt")).unwrap();
        assert_eq!(file_content, "hello nested world");

        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_write_patch_success() {
        let dir = setup_workspace();
        let file_path = dir.join("patch.txt");
        fs::write(&file_path, "hello apple world").unwrap();

        let args = WriteArgs {
            path: "patch.txt".to_string(),
            new_content: "banana".to_string(),
            old_content: Some("apple".to_string()),
        };

        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("status=\"success\""));
        assert!(res.contains("mode=\"patch\""));

        let file_content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(file_content, "hello banana world");

        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_write_patch_missing_error() {
        let dir = setup_workspace();
        let file_path = dir.join("patch.txt");
        fs::write(&file_path, "hello apple world").unwrap();

        let args = WriteArgs {
            path: "patch.txt".to_string(),
            new_content: "banana".to_string(),
            old_content: Some("orange".to_string()),
        };

        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("status=\"error\""));
        assert!(res.contains("reason=\"old_content not found"));

        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_write_patch_duplicate_error() {
        let dir = setup_workspace();
        let file_path = dir.join("patch.txt");
        fs::write(&file_path, "apple apple").unwrap();

        let args = WriteArgs {
            path: "patch.txt".to_string(),
            new_content: "banana".to_string(),
            old_content: Some("apple".to_string()),
        };

        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("status=\"error\""));
        assert!(res.contains("reason=\"old_content is not unique"));

        teardown_workspace(dir);
    }
}
