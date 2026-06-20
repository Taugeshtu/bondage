use std::path::{Path, PathBuf};
use std::collections::HashSet;

pub fn process_prompt(prompt: &str) -> std::io::Result<String> {
    let current_dir = std::env::current_dir()?;
    process_prompt_in_dir(prompt, &current_dir)
}

pub fn process_prompt_in_dir(prompt: &str, base_dir: &Path) -> std::io::Result<String> {
    let mut expanded_prompt = prompt.to_string();
    let mut processed_files = HashSet::new();
    let mut level_0_files = Vec::new();
    let mut level_1_files = Vec::new();

    // 1. Extract Level 0 files directly from the prompt
    let level_0_refs = find_file_references(prompt, base_dir);

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
        let level_1_refs = find_file_references(content, parent_dir);
        
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

fn find_file_references(text: &str, base_dir: &Path) -> Vec<PathBuf> {
    let mut refs = Vec::new();
    let mut search_str = text;

    while let Some(at_idx) = search_str.find('@') {
        let post_at = &search_str[at_idx + 1..];
        
        // Find next '@' or end of string/line to restrict the search segment
        let limit_idx = post_at.find('@').unwrap_or(post_at.len());
        let segment = &post_at[..limit_idx];
        
        let words: Vec<&str> = segment.split_whitespace().collect();
        let mut best_match = None;
        let mut best_word_count = 0;

        // Try greedy prefixes of words
        for k in 1..=words.len() {
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

            let path = if Path::new(&candidate).is_absolute() {
                PathBuf::from(&candidate)
            } else {
                base_dir.join(&candidate)
            };

            if path.exists() && path.is_file() {
                best_match = Some(path);
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
            // No match, advance past the '@' character
            search_str = &search_str[at_idx + 1..];
        }
    }

    refs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    fn setup_temp_dir() -> PathBuf {
        let test_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("bondage_test_{}", test_id));
        std::fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_find_file_references_basic() {
        let temp_dir = setup_temp_dir();
        
        let file1_path = temp_dir.join("foo bar.txt");
        let mut f1 = File::create(&file1_path).unwrap();
        writeln!(f1, "content of foo bar").unwrap();

        let file2_path = temp_dir.join("baz.rs");
        let mut f2 = File::create(&file2_path).unwrap();
        writeln!(f2, "content of baz").unwrap();

        // 1. Basic matches with spaces and punctuation
        let input = "Check out @foo bar.txt, and also @baz.rs. Thanks!";
        let refs = find_file_references(input, &temp_dir);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0], file1_path);
        assert_eq!(refs[1], file2_path);

        // Clean up
        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_find_file_references_greedy() {
        let temp_dir = setup_temp_dir();

        let foo_path = temp_dir.join("foo");
        let mut f1 = File::create(&foo_path).unwrap();
        writeln!(f1, "foo content").unwrap();

        let foo_bar_path = temp_dir.join("foo bar");
        let mut f2 = File::create(&foo_bar_path).unwrap();
        writeln!(f2, "foo bar content").unwrap();

        let input = "Process @foo bar now";
        let refs = find_file_references(input, &temp_dir);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], foo_bar_path);

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_process_prompt_recursion() {
        let temp_dir = setup_temp_dir();

        // parent.txt references child.txt
        let parent_path = temp_dir.join("parent.txt");
        let mut f1 = File::create(&parent_path).unwrap();
        writeln!(f1, "See @child.txt for details").unwrap();

        // child.txt references grandchild.txt
        let child_path = temp_dir.join("child.txt");
        let mut f2 = File::create(&child_path).unwrap();
        writeln!(f2, "Hello from child! And see @grandchild.txt").unwrap();

        // grandchild.txt exists
        let grandchild_path = temp_dir.join("grandchild.txt");
        let mut f3 = File::create(&grandchild_path).unwrap();
        writeln!(f3, "Hello from grandchild!").unwrap();

        let prompt = "Start by reading @parent.txt";
        let result = process_prompt_in_dir(prompt, &temp_dir).unwrap();

        // Level 0 (parent.txt) should be included
        assert!(result.contains("[File: parent.txt]"));
        assert!(result.contains("See @child.txt for details"));

        // Level 1 (child.txt) should be included
        assert!(result.contains("[File: child.txt (referenced by parent.txt)]"));
        assert!(result.contains("Hello from child! And see @grandchild.txt"));

        // Level 2 (grandchild.txt) should NOT be included
        assert!(!result.contains("[File: grandchild.txt"));
        assert!(!result.contains("Hello from grandchild!"));

        std::fs::remove_dir_all(&temp_dir).unwrap();
    }
}
