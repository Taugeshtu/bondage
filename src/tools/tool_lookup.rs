use std::path::{Path, PathBuf};
use serde::Deserialize;
use crate::{Message, ToolDefinition, ToolContext, BondageError};

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
