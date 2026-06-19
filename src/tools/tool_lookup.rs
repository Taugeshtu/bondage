use std::path::{Path, PathBuf};
use serde::Deserialize;
use crate::{ToolDefinition, ToolContext, BondageError};

#[derive(Deserialize)]
pub struct LookupArgs {
    pub target: String,
    pub query: Option<String>,
    pub radius: Option<usize>,
}

/// Helper to resolve target path relative to context current_dir
fn resolve_path(current_dir: &Path, path_str: &str) -> PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir.join(path)
    }
}

// =========================================================================
// File Lookup Logic
// =========================================================================

fn lookup_file(path: &Path, query: Option<&str>, radius: usize) -> Result<String, BondageError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| BondageError::Io(format!("Failed to read file: {}", e)))?;

    let lines: Vec<&str> = content.lines().collect();

    if let Some(q) = query {
        let q_lower = q.to_lowercase();
        let mut matches = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            if line.to_lowercase().contains(&q_lower) {
                matches.push(idx);
            }
        }

        if matches.is_empty() {
            return Ok(format!("Query '{}' not found in file.", q));
        }

        let mut output = String::new();
        let mut last_end = 0;

        for match_idx in matches {
            let start = match_idx.saturating_sub(radius);
            let end = (match_idx + radius + 1).min(lines.len());

            if start > last_end && last_end > 0 {
                output.push_str("\n... [\n");
            }

            let print_start = start.max(last_end);
            for i in print_start..end {
                output.push_str(&format!("{}: {}\n", i + 1, lines[i]));
            }
            last_end = end;
        }

        Ok(output)
    } else {
        // No query: Return the head of the file for now (first 100 lines)
        let end = 100.min(lines.len());
        let head = lines[0..end].join("\n");
        if lines.len() > 100 {
            Ok(format!("{}\n\n... truncated ({} lines remaining) ...", head, lines.len() - 100))
        } else {
            Ok(head)
        }
    }
}

// =========================================================================
// Directory Lookup Logic
// =========================================================================

fn lookup_dir(path: &Path, query: Option<&str>, radius: usize) -> Result<String, BondageError> {
    if let Some(q) = query {
        // Recursive text search (grep)
        let mut results = String::new();
        let q_lower = q.to_lowercase();

        fn walk(dir: &Path, q_lower: &str, results: &mut String) -> std::io::Result<()> {
            if dir.is_dir() {
                for entry in std::fs::read_dir(dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    
                    if name.starts_with('.') || name == "node_modules" || name == "target" {
                        continue;
                    }
                    walk(&path, q_lower, results)?;
                }
            } else if dir.is_file() {
                if let Ok(content) = std::fs::read_to_string(dir) {
                    for (idx, line) in content.lines().enumerate() {
                        if line.to_lowercase().contains(q_lower) {
                            results.push_str(&format!("{}:{}: {}\n", dir.display(), idx + 1, line.trim()));
                            if results.len() > 10000 {
                                results.push_str("... truncated (too many results) ...\n");
                                return Ok(());
                            }
                        }
                    }
                }
            }
            Ok(())
        }

        walk(path, &q_lower, &mut results)
            .map_err(|e| BondageError::Io(format!("Search failed: {}", e)))?;

        if results.is_empty() {
            Ok("No matches found.".to_string())
        } else {
            Ok(results)
        }
    } else {
        // Directory listing
        let mut output = String::new();
        let entries = std::fs::read_dir(path)
            .map_err(|e| BondageError::Io(format!("Failed to read directory: {}", e)))?;
        
        let mut count = 0;
        for entry in entries {
            if let Ok(entry) = entry {
                let name = entry.file_name().to_string_lossy().into_owned();
                let file_type = entry.file_type()
                    .map(|t| if t.is_dir() { "dir" } else { "file" })
                    .unwrap_or("unknown");
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                output.push_str(&format!("{}: {} ({} bytes)\n", file_type, name, size));
                
                count += 1;
                if count >= radius {
                    output.push_str("\n... truncated list ...");
                    break;
                }
            }
        }
        Ok(output)
    }
}

// =========================================================================
// Web URL Lookup Logic
// =========================================================================

async fn lookup_url(url: &str, query: Option<&str>) -> Result<String, BondageError> {
    let client = reqwest::Client::builder()
        .user_agent("Bondage/0.1.0")
        .build()
        .map_err(|e| BondageError::Tool(e.to_string()))?;
    
    let text = client.get(url)
        .send()
        .await
        .map_err(|e| BondageError::Tool(e.to_string()))?
        .text()
        .await
        .map_err(|e| BondageError::Tool(e.to_string()))?;

    if let Some(q) = query {
        let q_lower = q.to_lowercase();
        let mut matches = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&q_lower) {
                matches.push(format!("{}: {}", idx + 1, line.trim()));
            }
        }
        if matches.is_empty() {
            Ok(format!("Query '{}' not found on page.", q))
        } else {
            Ok(matches.join("\n"))
        }
    } else {
        // Return top of page text
        let head: Vec<&str> = text.lines().take(100).collect();
        Ok(head.join("\n"))
    }
}

// =========================================================================
// Main Execution Entrypoint
// =========================================================================

pub async fn execute(args: LookupArgs, context: &ToolContext) -> Result<String, BondageError> {
    let radius = args.radius.unwrap_or(20);

    // 1. Web Search query
    if args.target == "web" {
        if let Some(q) = args.query {
            return Ok(format!("Web search for '{}' is not implemented yet.", q));
        } else {
            return Err(BondageError::Tool("Web search target requires a query.".to_string()));
        }
    }

    // 2. HTTP Web URL lookup
    if args.target.starts_with("http://") || args.target.starts_with("https://") {
        return lookup_url(&args.target, args.query.as_deref()).await;
    }

    // 3. Local path lookup
    let resolved = resolve_path(&context.current_dir, &args.target);
    if resolved.is_dir() {
        lookup_dir(&resolved, args.query.as_deref(), radius)
    } else if resolved.is_file() {
        lookup_file(&resolved, args.query.as_deref(), radius)
    } else {
        Err(BondageError::Io(format!("Target path '{}' does not exist.", args.target)))
    }
}

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "lookup".to_string(),
        description: "Examine a file, search inside a file, list directories, search directories, read webpages, or run a web search.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "target": { "type": "string", "description": "The destination path, URL, or 'web'." },
                "query": { "type": "string", "description": "Optional search term, symbol anchor, or web query." },
                "radius": { "type": "integer", "description": "Optional context radius (lines for files, depth for directory listing, results count for web search)." }
            },
            "required": ["target"]
        }),
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_workspace() -> (PathBuf, ToolContext) {
        let test_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("bondage_test_{}", test_id));
        fs::create_dir_all(&temp_dir).unwrap();

        // 1. Create a small file
        let small_path = temp_dir.join("small.txt");
        fs::write(&small_path, "apple\nbanana\ncherry").unwrap();

        // 2. Create a larger file (120 lines)
        let large_path = temp_dir.join("large.txt");
        let mut large_content = String::new();
        for i in 1..=120 {
            large_content.push_str(&format!("Line {}\n", i));
        }
        fs::write(&large_path, large_content).unwrap();

        // 3. Create a subdirectory and a nested file
        let subdir = temp_dir.join("subdir");
        fs::create_dir_all(&subdir).unwrap();
        let nested_path = subdir.join("nested.txt");
        fs::write(&nested_path, "nested apple").unwrap();

        let context = ToolContext {
            workspace_root: temp_dir.clone(),
            current_dir: temp_dir.clone(),
        };

        (temp_dir, context)
    }

    fn teardown_workspace(dir: PathBuf) {
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_file_read_entire() {
        let (dir, context) = setup_workspace();
        let args = LookupArgs {
            target: "small.txt".to_string(),
            query: None,
            radius: None,
        };
        let res = execute(args, &context).await.unwrap();
        assert_eq!(res, "apple\nbanana\ncherry");
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_file_read_truncated() {
        let (dir, context) = setup_workspace();
        let args = LookupArgs {
            target: "large.txt".to_string(),
            query: None,
            radius: None,
        };
        let res = execute(args, &context).await.unwrap();
        assert!(res.contains("... truncated"));
        assert!(res.contains("20 lines remaining"));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_file_lookup_query() {
        let (dir, context) = setup_workspace();
        let args = LookupArgs {
            target: "small.txt".to_string(),
            query: Some("banana".to_string()),
            radius: Some(1),
        };
        let res = execute(args, &context).await.unwrap();
        // banana is line 2. With radius 1, it should output lines 1, 2, and 3
        assert!(res.contains("1: apple"));
        assert!(res.contains("2: banana"));
        assert!(res.contains("3: cherry"));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_dir_list() {
        let (dir, context) = setup_workspace();
        let args = LookupArgs {
            target: "subdir".to_string(),
            query: None,
            radius: Some(5),
        };
        let res = execute(args, &context).await.unwrap();
        assert!(res.contains("file: nested.txt"));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_dir_grep() {
        let (dir, context) = setup_workspace();
        let args = LookupArgs {
            target: ".".to_string(),
            query: Some("apple".to_string()),
            radius: None,
        };
        let res = execute(args, &context).await.unwrap();
        // Should find matches in small.txt and nested.txt
        assert!(res.contains("small.txt:1: apple"));
        assert!(res.contains("nested.txt:1: nested apple"));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_web_url_lookup() {
        let (_, context) = setup_workspace();
        let args = LookupArgs {
            target: "https://example.com".to_string(),
            query: None,
            radius: None,
        };
        // This will perform a real fetch. Let it fail if network isn't accessible.
        let res = execute(args, &context).await.unwrap();
        assert!(res.contains("Example Domain"));
    }

    #[tokio::test]
    async fn test_web_search_not_implemented() {
        let (_, context) = setup_workspace();
        let args = LookupArgs {
            target: "web".to_string(),
            query: Some("rust lang".to_string()),
            radius: None,
        };
        let res = execute(args, &context).await.unwrap();
        assert!(res.contains("not implemented yet"));
    }
}
