use std::path::{Path, PathBuf};
use serde::Deserialize;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub endpoint: Option<String>,
    pub adapter: Option<String>,
    pub terminal: Option<String>,
    #[serde(default)]
    pub policy: bondage::policy::PolicyConfig,
}

impl Config {
    pub fn merge(&mut self, other: Config) {
        if other.model.is_some() {
            self.model = other.model;
        }
        if other.api_key.is_some() {
            self.api_key = other.api_key;
        }
        if other.endpoint.is_some() {
            self.endpoint = other.endpoint;
        }
        if other.adapter.is_some() {
            self.adapter = other.adapter;
        }
        if other.terminal.is_some() {
            self.terminal = other.terminal;
        }
        
        // Merge policy fields
        if other.policy.access_lookup_directory.is_some() {
            self.policy.access_lookup_directory = other.policy.access_lookup_directory;
        }
        if other.policy.access_lookup_fs.is_some() {
            self.policy.access_lookup_fs = other.policy.access_lookup_fs;
        }
        if other.policy.access_lookup_web.is_some() {
            self.policy.access_lookup_web = other.policy.access_lookup_web;
        }
        if other.policy.access_write_directory.is_some() {
            self.policy.access_write_directory = other.policy.access_write_directory;
        }
        if other.policy.access_write_fs.is_some() {
            self.policy.access_write_fs = other.policy.access_write_fs;
        }
        if other.policy.access_bash.is_some() {
            self.policy.access_bash = other.policy.access_bash;
        }
    }
}

pub fn load_config(config_path: &Path) -> Config {
    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(config_path) {
            if let Ok(config) = toml::from_str::<Config>(&content) {
                return config;
            }
        }
    }
    Config::default()
}



pub fn ensure_resources_installed() -> std::io::Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let config_dir = PathBuf::from(&home).join(".config/rope");

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)?;
    }

    let yolo_path = config_dir.join("yolo.toml");
    if !yolo_path.exists() {
        let yolo_template = r#"[policy]
access_lookup_directory = "yes"
access_lookup_fs = "yes"
access_lookup_web = "yes"
access_write_directory = "yes"
access_write_fs = "yes"
access_bash = "yes"
"#;
        std::fs::write(&yolo_path, yolo_template)?;
        println!("✨ Created default yolo configuration at {}", yolo_path.display());
    }

    let config_path = config_dir.join("config.toml");
    if !config_path.exists() {
        let config_template = r#"# Default configuration template
# model = "gemini-3.1-flash-lite"
# adapter = "gemini"
# api_key = "YOUR_GEMINI_API_KEY_HERE"
"#;
        std::fs::write(&config_path, config_template)?;
        println!("✨ Created default config template at {}", config_path.display());
    }

    let regular_prompt_path = config_dir.join("system-regular.txt");
    if !regular_prompt_path.exists() {
        let regular_template = include_str!("../../docs/system-regular.txt");
        std::fs::write(&regular_prompt_path, regular_template)?;
        println!("✨ Created default system-regular.txt prompt at {}", regular_prompt_path.display());
    }

    let interactive_prompt_path = config_dir.join("system-interactive.txt");
    if !interactive_prompt_path.exists() {
        let interactive_template = include_str!("../../docs/system-interactive.txt");
        std::fs::write(&interactive_prompt_path, interactive_template)?;
        println!("✨ Created default system-interactive.txt prompt at {}", interactive_prompt_path.display());
    }

    Ok(())
}
