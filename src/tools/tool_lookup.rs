use std::path::{Path, PathBuf};
use serde::Deserialize;
use crate::{ToolDefinition, BondageError};

#[derive(Deserialize)]
pub struct LookupArgs {
    pub target: String,
    pub query: Option<String>,
    pub radius: Option<usize>,
}

/// Helper to resolve target path relative to the base directory
fn resolve_path(base_dir: &Path, path_str: &str) -> PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn html_escape(input: &str) -> String {
    let mut escaped = String::new();
    for c in input.chars() {
        match c {
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
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
            return Ok(format!("<file path=\"{}\" query=\"{}\">\n  <!-- Query not found in file -->\n</file>", path.to_string_lossy(), html_escape(q)));
        }

        let mut output = format!("<file path=\"{}\" query=\"{}\" total_lines=\"{}\">\n", path.to_string_lossy(), html_escape(q), lines.len());
        
        let mut current_fragment_start = None;
        let mut current_fragment_end = None;

        for match_idx in matches {
            let start = match_idx.saturating_sub(radius);
            let end = (match_idx + radius + 1).min(lines.len());

            if let (Some(cur_start), Some(cur_end)) = (current_fragment_start, current_fragment_end) {
                if start <= cur_end {
                    current_fragment_end = Some(end);
                } else {
                    output.push_str(&format!("  <fragment start_line=\"{}\" end_line=\"{}\">\n", cur_start + 1, cur_end));
                    for i in cur_start..cur_end {
                        output.push_str(&format!("{}: {}\n", i + 1, lines[i]));
                    }
                    output.push_str("  </fragment>\n");
                    current_fragment_start = Some(start);
                    current_fragment_end = Some(end);
                }
            } else {
                current_fragment_start = Some(start);
                current_fragment_end = Some(end);
            }
        }

        if let (Some(cur_start), Some(cur_end)) = (current_fragment_start, current_fragment_end) {
            output.push_str(&format!("  <fragment start_line=\"{}\" end_line=\"{}\">\n", cur_start + 1, cur_end));
            for i in cur_start..cur_end {
                output.push_str(&format!("{}: {}\n", i + 1, lines[i]));
            }
            output.push_str("  </fragment>\n");
        }

        output.push_str("</file>");
        Ok(output)
    } else {
        let end = 100.min(lines.len());
        let head = lines[0..end].join("\n");
        let path_str = path.to_string_lossy();
        if lines.len() > 100 {
            let remaining = lines.len() - 100;
            Ok(format!("<file path=\"{}\" total_lines=\"{}\" truncated=\"true\">\n  <content>{}</content>\n  <truncated lines_remaining=\"{}\" />\n</file>", path_str, lines.len(), head, remaining))
        } else {
            Ok(format!("<file path=\"{}\" total_lines=\"{}\">\n  <content>{}</content>\n</file>", path_str, lines.len(), head))
        }
    }
}

// =========================================================================
// Directory Lookup Logic
// =========================================================================

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn lookup_dir(path: &Path, query: Option<&str>, radius: usize) -> Result<String, BondageError> {
    if let Some(q) = query {
        let mut results = Vec::new();
        let q_lower = q.to_lowercase();
        let mut truncated = false;

        fn walk(dir: &Path, q_lower: &str, results: &mut Vec<(String, usize, String)>, truncated: &mut bool) -> std::io::Result<()> {
            if *truncated {
                return Ok(());
            }
            if dir.is_dir() {
                for entry in std::fs::read_dir(dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    
                    if name.starts_with('.') || name == "node_modules" || name == "target" {
                        continue;
                    }
                    if let Ok(file_type) = entry.file_type() {
                        if file_type.is_symlink() {
                            continue;
                        }
                    }
                    walk(&path, q_lower, results, truncated)?;
                }
            } else if dir.is_file() {
                if let Ok(content) = std::fs::read_to_string(dir) {
                    for (idx, line) in content.lines().enumerate() {
                        if line.to_lowercase().contains(q_lower) {
                            results.push((dir.to_string_lossy().into_owned(), idx + 1, line.trim().to_string()));
                            if results.len() > 200 {
                                *truncated = true;
                                return Ok(());
                            }
                        }
                    }
                }
            }
            Ok(())
        }

        walk(path, &q_lower, &mut results, &mut truncated)
            .map_err(|e| BondageError::Io(format!("Search failed: {}", e)))?;

        let mut output = format!("<dir_search path=\"{}\" query=\"{}\">\n", path.to_string_lossy(), html_escape(q));
        for (f_path, line_num, line_content) in results {
            output.push_str(&format!("  <match file=\"{}\" line=\"{}\">{}</match>\n", f_path, line_num, html_escape(&line_content)));
        }
        if truncated {
            output.push_str("  <truncated />\n");
        }
        output.push_str("</dir_search>");
        Ok(output)
    } else {
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        
        let entries = std::fs::read_dir(path)
            .map_err(|e| BondageError::Io(format!("Failed to read directory: {}", e)))?;
        
        for entry in entries {
            if let Ok(entry) = entry {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                
                if is_dir {
                    dirs.push(name);
                } else {
                    files.push((name, size));
                }
            }
        }
        
        dirs.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        
        let mut output = format!("<dir path=\"{}\">\n", path.to_string_lossy());
        let mut count = 0;
        let mut truncated = false;
        
        for d in dirs {
            if count >= radius {
                truncated = true;
                break;
            }
            output.push_str(&format!("  <dir name=\"{}\" />\n", d));
            count += 1;
        }
        
        for (f, size) in files {
            if count >= radius {
                truncated = true;
                break;
            }
            output.push_str(&format!("  <file name=\"{}\" size=\"{}\" />\n", f, format_size(size)));
            count += 1;
        }
        
        if truncated {
            output.push_str("  <truncated />\n");
        }
        
        output.push_str("</dir>");
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
                matches.push((idx + 1, line.trim().to_string()));
            }
        }
        
        let mut output = format!("<webpage url=\"{}\" query=\"{}\">\n", url, html_escape(q));
        if matches.is_empty() {
            output.push_str("  <!-- Query not found on page -->\n");
        } else {
            for (line_num, line_content) in matches {
                output.push_str(&format!("  <match line=\"{}\">{}</match>\n", line_num, html_escape(&line_content)));
            }
        }
        output.push_str("</webpage>");
        Ok(output)
    } else {
        let total_lines = text.lines().count();
        let head: Vec<&str> = text.lines().take(100).collect();
        let head_text = head.join("\n");
        
        let mut output = format!("<webpage url=\"{}\">\n  <content>{}</content>\n", url, html_escape(&head_text));
        if total_lines > 100 {
            output.push_str(&format!("  <truncated lines_remaining=\"{}\" />\n", total_lines - 100));
        }
        output.push_str("</webpage>");
        Ok(output)
    }
}

// =========================================================================
// Main Execution Entrypoint
// =========================================================================

pub async fn execute(args: LookupArgs, base_dir: &Path) -> Result<String, BondageError> {
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
    let resolved = resolve_path(base_dir, &args.target);
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

    static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn setup_workspace() -> PathBuf {
        let counter = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp_dir = std::env::temp_dir().join(format!("bondage_test_lookup_{}", counter));
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

        temp_dir
    }

    fn teardown_workspace(dir: PathBuf) {
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_file_read_entire() {
        let dir = setup_workspace();
        let args = LookupArgs {
            target: "small.txt".to_string(),
            query: None,
            radius: None,
        };
        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("<file path="));
        assert!(res.contains("apple\nbanana\ncherry"));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_file_read_truncated() {
        let dir = setup_workspace();
        let args = LookupArgs {
            target: "large.txt".to_string(),
            query: None,
            radius: None,
        };
        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("truncated=\"true\""));
        assert!(res.contains("lines_remaining=\"20\""));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_file_lookup_query() {
        let dir = setup_workspace();
        let args = LookupArgs {
            target: "small.txt".to_string(),
            query: Some("banana".to_string()),
            radius: Some(1),
        };
        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("<fragment start_line=\"1\" end_line=\"3\">"));
        assert!(res.contains("2: banana"));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_dir_list() {
        let dir = setup_workspace();
        let args = LookupArgs {
            target: "subdir".to_string(),
            query: None,
            radius: Some(5),
        };
        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("<file name=\"nested.txt\""));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_dir_grep() {
        let dir = setup_workspace();
        let args = LookupArgs {
            target: ".".to_string(),
            query: Some("apple".to_string()),
            radius: None,
        };
        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("<match file="));
        assert!(res.contains("small.txt\" line=\"1\">apple</match>"));
        assert!(res.contains("nested.txt\" line=\"1\">nested apple</match>"));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_web_url_lookup() {
        let dir = setup_workspace();
        let args = LookupArgs {
            target: "https://example.com".to_string(),
            query: None,
            radius: None,
        };
        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("<webpage url=\"https://example.com\">"));
        assert!(res.contains("Example Domain"));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_web_search_not_implemented() {
        let dir = setup_workspace();
        let args = LookupArgs {
            target: "web".to_string(),
            query: Some("rust lang".to_string()),
            radius: None,
        };
        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("not implemented yet"));
        teardown_workspace(dir);
    }

    #[tokio::test]
    async fn test_dir_grep_circular_symlink() {
        let dir = setup_workspace();
        
        #[cfg(unix)]
        {
            let link_path = dir.join("loop");
            let _ = std::os::unix::fs::symlink(&dir, link_path);
        }

        let args = LookupArgs {
            target: ".".to_string(),
            query: Some("apple".to_string()),
            radius: None,
        };

        let res = execute(args, &dir).await.unwrap();
        assert!(res.contains("small.txt"));
        
        teardown_workspace(dir);
    }
}
