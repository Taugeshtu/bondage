use std::path::Path;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PolicyMode {
    Yes,
    No,
    Ask,
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct PolicyConfig {
    pub access_lookup_directory: Option<PolicyMode>,
    pub access_lookup_fs: Option<PolicyMode>,
    pub access_lookup_web: Option<PolicyMode>,
    pub access_write_directory: Option<PolicyMode>,
    pub access_write_fs: Option<PolicyMode>,
    pub access_bash: Option<PolicyMode>,
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub lookup_directory: PolicyMode,
    pub lookup_fs: PolicyMode,
    pub lookup_web: PolicyMode,
    pub write_directory: PolicyMode,
    pub write_fs: PolicyMode,
    pub bash: PolicyMode,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            lookup_directory: PolicyMode::Yes,
            lookup_fs: PolicyMode::Yes,
            lookup_web: PolicyMode::Yes,
            write_directory: PolicyMode::Ask,
            write_fs: PolicyMode::Ask,
            bash: PolicyMode::Ask,
        }
    }
}

impl Policy {
    pub fn from_config(cfg: &PolicyConfig) -> Self {
        let mut policy = Policy::default();
        if let Some(mode) = cfg.access_lookup_directory {
            policy.lookup_directory = mode;
        }
        if let Some(mode) = cfg.access_lookup_fs {
            policy.lookup_fs = mode;
        }
        if let Some(mode) = cfg.access_lookup_web {
            policy.lookup_web = mode;
        }
        if let Some(mode) = cfg.access_write_directory {
            policy.write_directory = mode;
        }
        if let Some(mode) = cfg.access_write_fs {
            policy.write_fs = mode;
        }
        if let Some(mode) = cfg.access_bash {
            policy.bash = mode;
        }
        policy
    }

    pub fn check_lookup(&self, target: &str, base_dir: &Path) -> PolicyMode {
        if target == "web" || target.starts_with("http://") || target.starts_with("https://") {
            self.lookup_web
        } else {
            let expanded = crate::util::expand_tilde(Path::new(target));
            let resolved = if expanded.is_absolute() {
                expanded
            } else {
                base_dir.join(expanded)
            };
            
            if is_path_inside_base(base_dir, &resolved) {
                self.lookup_directory
            } else {
                self.lookup_fs
            }
        }
    }

    pub fn check_write(&self, target_path: &str, base_dir: &Path) -> PolicyMode {
        let expanded = crate::util::expand_tilde(Path::new(target_path));
        let resolved = if expanded.is_absolute() {
            expanded
        } else {
            base_dir.join(expanded)
        };
        
        if is_path_inside_base(base_dir, &resolved) {
            self.write_directory
        } else {
            self.write_fs
        }
    }

    pub fn check_bash(&self) -> PolicyMode {
        self.bash
    }
}

/// Robustly checks if resolved_path canonicalizes inside base_dir canonicalized path.
/// Handles non-existent subpaths by traversing parents until an existing directory is found.
fn is_path_inside_base(base_dir: &Path, resolved_path: &Path) -> bool {
    let Ok(canonical_base) = base_dir.canonicalize() else {
        return false;
    };
    
    let mut current = resolved_path.to_path_buf();
    loop {
        if current.exists() {
            if let Ok(canonical_current) = current.canonicalize() {
                return canonical_current.starts_with(&canonical_base);
            }
            return false;
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_path_safety_check() {
        static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counter = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base_dir = std::env::temp_dir().join(format!("bondage_policy_test_{}", counter));
        fs::create_dir_all(&base_dir).unwrap();

        let inside_file = base_dir.join("src/lib.rs");
        let outside_file = std::env::temp_dir().join("some_other_file_outside.txt");

        assert!(is_path_inside_base(&base_dir, &inside_file));
        assert!(!is_path_inside_base(&base_dir, &outside_file));

        fs::remove_dir_all(&base_dir).unwrap();
    }

    #[test]
    fn test_policy_tilde_expansion() {
        let config = PolicyConfig {
            access_lookup_directory: Some(PolicyMode::Yes),
            access_lookup_fs: Some(PolicyMode::No),
            access_write_directory: Some(PolicyMode::Yes),
            access_write_fs: Some(PolicyMode::No),
            ..Default::default()
        };
        let policy = Policy::from_config(&config);
        
        let base_dir = std::path::Path::new("/media/veracrypt1/_PROJECTS/_LinuxUX/Bondage");
        
        unsafe {
            std::env::set_var("HOME", "/home/testuser");
        }
        
        let mode = policy.check_lookup("~/K/Catch-all/", base_dir);
        assert_eq!(mode, PolicyMode::No);
        
        let mode_write = policy.check_write("~/K/Catch-all/file.txt", base_dir);
        assert_eq!(mode_write, PolicyMode::No);
    }
}
