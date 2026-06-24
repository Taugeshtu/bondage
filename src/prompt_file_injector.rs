use std::path::{Path, PathBuf};
use std::collections::HashSet;

pub fn process_prompt(prompt: &str) -> Result<String, Vec<String>> {
    let current_dir = std::env::current_dir().map_err(|e| vec![e.to_string()])?;
    process_prompt_in_dir(prompt, &current_dir)
}

pub fn process_prompt_in_dir(prompt: &str, base_dir: &Path) -> Result<String, Vec<String>> {
    let mut expanded_prompt = prompt.to_string();
    let mut processed_files = HashSet::new();
    let mut level_0_files = Vec::new();
    let mut level_1_files = Vec::new();
    let mut all_errors = Vec::new();

    // 1. Extract Level 0 files directly from the prompt
    let (level_0_refs, mut level_0_errors) = find_file_references(prompt, base_dir);
    all_errors.append(&mut level_0_errors);

    for path in level_0_refs {
        let path_str = path.to_string_lossy().to_string();
        if processed_files.insert(path_str.clone()) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                // Get the path display relative to the base directory if possible
                let display_name = if let Ok(suffix) = path.strip_prefix(base_dir) {
                    suffix.to_string_lossy().to_string()
                } else {
                    path_str.clone()
                };
                level_0_files.push((display_name, content, path.clone()));
            }
        }
    }

    // 2. Extract Level 1 files from Level 0 contents (recursive depth = 1)
    for (name, content, parent_path) in &level_0_files {
        let parent_dir = parent_path.parent().unwrap_or(base_dir);
        let (level_1_refs, mut level_1_errors) = find_file_references(content, parent_dir);
        all_errors.append(&mut level_1_errors);
        
        for path in level_1_refs {
            let path_str = path.to_string_lossy().to_string();
            if processed_files.insert(path_str.clone()) {
                if let Ok(child_content) = std::fs::read_to_string(&path) {
                    let display_name = if let Ok(suffix) = path.strip_prefix(base_dir) {
                        suffix.to_string_lossy().to_string()
                    } else {
                        path_str.clone()
                    };
                    level_1_files.push((display_name, child_content, name.clone()));
                }
            }
        }
    }

    if !all_errors.is_empty() {
        return Err(all_errors);
    }

    // 3. Assemble the final prompt
    if !level_0_files.is_empty() || !level_1_files.is_empty() {
        expanded_prompt.push_str("\n\n---");
        
        for (name, content, _) in level_0_files {
            expanded_prompt.push_str(&format!("\n\n[File: {}]\n{}", name, content));
        }
        for (name, content, parent_name) in level_1_files {
            expanded_prompt.push_str(&format!("\n\n[File: {} (referenced by {})]\n{}", name, parent_name, content));
        }
    }

    Ok(expanded_prompt)
}

fn find_file_references(text: &str, base_dir: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut refs = Vec::new();
    let mut errors = Vec::new();
    let mut search_str = text;

    while let Some(at_idx) = search_str.find('@') {
        let post_at = &search_str[at_idx + 1..];
        
        // Skip control words like @rope or @rope-done
        if post_at.starts_with("rope") {
            search_str = &search_str[at_idx + 1..];
            continue;
        }

        // Find next '@', newline, or end of string to restrict the search segment
        let limit_idx = post_at.find(|c| c == '@' || c == '\n' || c == '\r').unwrap_or(post_at.len());
        let segment = &post_at[..limit_idx];
        
        let words: Vec<&str> = segment.split_whitespace().collect();
        if words.is_empty() {
            search_str = &search_str[at_idx + 1..];
            continue;
        }

        let mut best_match = None;
        let mut best_word_count = 0;

        // Try greedy prefixes of words
        for k in 1..=words.len().min(10) {
            let candidate_words = &words[..k];
            let mut candidate = candidate_words.join(" ");

            // Clean trailing punctuation
            while candidate.ends_with('.') 
                || candidate.ends_with(',') 
                || candidate.ends_with('?') 
                || candidate.ends_with(';') 
                || candidate.ends_with(':') 
                || candidate.ends_with('"') 
                || candidate.ends_with('\'')
                || candidate.ends_with(')')
                || candidate.ends_with(']')
                || candidate.ends_with('}')
            {
                candidate.pop();
            }

            // Basic sanity check to avoid matching large text sections
            if candidate.is_empty() || candidate.contains('<') || candidate.contains('>') || candidate.contains('|') {
                break;
            }

            // Use the unified locate_resource logic
            if let Ok(resolved) = crate::util::locate_resource(&candidate, true, base_dir) {
                best_match = Some(resolved);
                best_word_count = k;
            }
        }

        if let Some(matched_path) = best_match {
            refs.push(matched_path);
            // Skip the consumed segment to avoid double matching
            let start_ptr = segment.as_ptr() as usize;
            let last_word = words[best_word_count - 1];
            let word_ptr = last_word.as_ptr() as usize;
            let offset_in_segment = word_ptr - start_ptr;
            let end_offset_in_segment = offset_in_segment + last_word.len();
            let consumed_len = at_idx + 1 + end_offset_in_segment;
            search_str = &search_str[consumed_len..];
        } else {
            // Emitted as error since resource is specified but missing
            let first_word = words[0];
            errors.push(format!(
                "Resource '@{}' not found in CWD ({}) or ~/.config/rope/",
                first_word,
                base_dir.display()
            ));
            // No match, advance past the '@' character
            search_str = &search_str[at_idx + 1..];
        }
    }

    (refs, errors)
}

