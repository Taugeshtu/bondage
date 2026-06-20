mod render;
mod tmux;

use std::path::PathBuf;
use std::io::{self, Write};
use serde::Deserialize;
use bondage::{Message, step_stream};

#[derive(Deserialize, Debug, Default, Clone)]
struct Config {
    model: Option<String>,
    api_key: Option<String>,
    endpoint: Option<String>,
    adapter: Option<String>,
    terminal: Option<String>,
    #[serde(default)]
    policy: bondage::policy::PolicyConfig,
}

impl Config {
    fn merge(&mut self, other: Config) {
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

fn ask_approval(tool_name: &str, args: &str) -> bool {
    print!("\n⚠️  [Agent wants to execute: {} ({})] Approve? (y/N): ", tool_name, args.trim());
    let _ = io::stdout().flush();
    
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let trimmed = input.trim().to_lowercase();
        trimmed == "y" || trimmed == "yes"
    } else {
        false
    }
}

fn load_config(config_path: &std::path::Path) -> Config {
    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(config_path) {
            if let Ok(config) = toml::from_str::<Config>(&content) {
                return config;
            }
        }
    }
    Config::default()
}

fn resolve_config_path(config_path_str: Option<&str>) -> std::io::Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let config_dir = PathBuf::from(&home).join(".config/rope");

    let Some(raw_path) = config_path_str else {
        // Default config path fallback
        return Ok(config_dir.join("config.toml"));
    };

    // Build filename candidates
    let mut candidates = Vec::new();
    if raw_path.to_lowercase().ends_with(".toml") {
        candidates.push(raw_path.to_string());
    } else {
        candidates.push(format!("{}.toml", raw_path));
        candidates.push(raw_path.to_string());
    }

    let current_dir = std::env::current_dir()?;

    // Search candidates in order
    for candidate in &candidates {
        // 1. Direct path / relative to CWD
        let direct = PathBuf::from(candidate);
        if direct.exists() {
            return Ok(direct);
        }

        // 2. CWD joined
        let cwd_joined = current_dir.join(candidate);
        if cwd_joined.exists() {
            return Ok(cwd_joined);
        }

        // 3. ~/.config/rope/ joined
        let config_joined = config_dir.join(candidate);
        if config_joined.exists() {
            return Ok(config_joined);
        }
    }

    // If raw_path was explicitly requested but not found, return NotFound error
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("Config file '{}' not found in CWD or ~/.config/rope/", raw_path),
    ))
}

fn ensure_config_installed() -> std::io::Result<()> {
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

    Ok(())
}

async fn execute_bash_tmux(
    id: &str,
    arguments: &str,
    current_dir: &std::path::Path,
    policy_mode: bondage::policy::PolicyMode,
    custom_terminal: Option<String>,
) -> Message {
    let args: Result<bondage::tools::tool_bash::BashArgs, _> = serde_json::from_str(arguments);
    let command_to_run = match args {
        Ok(a) => a.command,
        Err(e) => {
            return Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: format!("Failed to parse arguments: {}", e),
                is_error: true,
            };
        }
    };

    // 0. Hardening Fallback: If tmux is not available on the host system, run command directly via tokio one-off process
    if !tmux::is_tmux_available() {
        println!("⚠️  Warning: tmux is not installed. Falling back to raw one-off command execution.");
        
        // If the safety policy is Ask, we must prompt the user inline because we can't use the tmux-attach approval mechanism
        if policy_mode == bondage::policy::PolicyMode::Ask {
            if !ask_approval("bash", arguments) {
                return Message::ToolResponse {
                    id: id.to_string(),
                    name: "bash".to_string(),
                    content: "Permission Denied: command execution cancelled by user.".to_string(),
                    is_error: true,
                };
            }
        }

        let baseline_res = bondage::tools::tool_bash::execute(
            bondage::tools::tool_bash::BashArgs { command: command_to_run },
            current_dir,
        ).await;
        
        return match baseline_res {
            Ok(content) => Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content,
                is_error: false,
            },
            Err(e) => Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: e.to_string(),
                is_error: true,
            },
        };
    }

    let pid = std::process::id();
    let session_name = format!("rope-shell-{}", pid);

    // 1. Ensure tmux session exists
    if !tmux::has_session(&session_name) {
        if let Err(e) = tmux::start_session(&session_name, current_dir) {
            return Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: format!("Failed to start tmux session: {}", e),
                is_error: true,
            };
        }
    }

    // 2. Send the command (without enter)
    if let Err(e) = tmux::send_command_literal(&session_name, &command_to_run) {
        return Message::ToolResponse {
            id: id.to_string(),
            name: "bash".to_string(),
            content: format!("Failed to send command to tmux: {}", e),
            is_error: true,
        };
    }

    // If the policy mode is "Yes" (auto-accept / yolo mode), send "Enter" right after the command
    if policy_mode == bondage::policy::PolicyMode::Yes {
        if let Err(e) = tmux::send_control_key(&session_name, "C-m") {
            return Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: format!("Failed to automatically execute command in tmux: {}", e),
                is_error: true,
            };
        }
    }

    // 3. Pop the terminal (Only if policy_mode != Yes / we are NOT in auto-accept YOLO mode)
    let mut term_handle = None;
    let mut fallback_to_inline_approval = false;
    if policy_mode != bondage::policy::PolicyMode::Yes {
        println!("📺 Popping interactive terminal window (Alacritty / Tmux Split)...");
        term_handle = match tmux::pop_terminal(&session_name, custom_terminal.as_deref()) {
            Ok(handle) => handle,
            Err(e) => {
                return Message::ToolResponse {
                    id: id.to_string(),
                    name: "bash".to_string(),
                    content: format!(
                        "Error: Failed to spawn the configured terminal command: {}\n\
                         Please verify that the terminal emulator is installed and correctly configured in your config.toml.",
                        e
                    ),
                    is_error: true,
                };
            }
        };
        if term_handle.is_none() {
            fallback_to_inline_approval = true;
        }
    }

    // 4. Polling / Approval loop
    if fallback_to_inline_approval {
        if ask_approval("bash", &arguments) {
            // User approved, send Enter key to run the command
            if let Err(e) = tmux::send_control_key(&session_name, "C-m") {
                return Message::ToolResponse {
                    id: id.to_string(),
                    name: "bash".to_string(),
                    content: format!("Failed to execute command in tmux: {}", e),
                    is_error: true,
                };
            }
        } else {
            // User denied, send Ctrl+C to clear the session
            let _ = tmux::send_control_key(&session_name, "C-c");
            return Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: "Permission Denied: command execution cancelled by user.".to_string(),
                is_error: true,
            };
        }
    } else if policy_mode != bondage::policy::PolicyMode::Yes {
        println!("⏳ Waiting for user to press Enter in terminal to run command...");
    }

    let poll_interval = std::time::Duration::from_millis(200);
    let output_content;

    loop {
        tokio::time::sleep(poll_interval).await;

        let should_cancel = match &mut term_handle {
            Some(tmux::TerminalHandle::Gui(child)) => {
                child.try_wait().map(|opt| opt.is_some()).unwrap_or(false)
            }
            Some(tmux::TerminalHandle::Tty(_pane_id)) => {
                !tmux::has_attached_clients(&session_name)
            }
            None => false,
        };

        if should_cancel {
            // Send Ctrl+C to clear the pending command line in the session
            let _ = tmux::send_control_key(&session_name, "C-c");
            
            // Clean up display window/pane if still running
            if let Some(h) = term_handle {
                let _ = tmux::close_terminal(h);
            }
            
            return Message::ToolResponse {
                id: id.to_string(),
                name: "bash".to_string(),
                content: "Permission Denied: terminal window/split closed or cancelled by user.".to_string(),
                is_error: true,
            };
        }

        // Fetch current pane content
        if let Ok(content) = tmux::get_pane_content(&session_name) {
            let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
            if let Some(&last_line) = lines.last() {
                if let Ok(idle) = tmux::is_pane_idle(&session_name) {
                    let trimmed_cmd = command_to_run.trim();
                    let last_cmd_line = trimmed_cmd.lines().last().unwrap_or("");
                    if idle && !last_line.trim().ends_with(last_cmd_line) {
                        output_content = content;
                        break;
                    }
                }
            }
        }
    }

    // 5. Cleanup the display interface, but KEEP the session running
    if let Some(h) = term_handle {
        let _ = tmux::close_terminal(h);
    }

    Message::ToolResponse {
        id: id.to_string(),
        name: "bash".to_string(),
        content: output_content,
        is_error: false,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Check if we are in TTY and NOT in TMUX, and if so, bootstrap ourselves in tmux
    let is_gui = std::env::var("WAYLAND_DISPLAY").is_ok() || std::env::var("DISPLAY").is_ok();
    let in_tmux = std::env::var("TMUX").is_ok();
    
    if !is_gui && !in_tmux {
        println!("📟 Raw TTY console detected. Launching tmux session to enable split-screen pane...");
        let current_exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("rope"));
        let args: Vec<String> = std::env::args().skip(1).collect();
        
        let pid = std::process::id();
        let main_session_name = format!("rope-main-{}", pid);
        
        let mut tmux_args = vec![
            "new-session".to_string(),
            "-s".to_string(),
            main_session_name,
        ];
        tmux_args.push(current_exe.to_string_lossy().to_string());
        for arg in args {
            tmux_args.push(arg);
        }
        
        match std::process::Command::new("tmux").args(&tmux_args).status() {
            Ok(status) => {
                std::process::exit(status.code().unwrap_or(0));
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    println!("⚠️  Warning: tmux binary not found. Running in raw terminal without split-screen support.");
                } else {
                    println!("⚠️  Warning: Failed to launch tmux: {}. Proceeding in raw terminal.", e);
                }
            }
        }
    }

    // Ensure config dir and baseline settings exist
    ensure_config_installed()?;

    // 1. Parse arguments: -c/--config, -h/--help and collect positional prompt
    let mut config_paths = Vec::new();
    let mut help = false;
    let mut positional_args = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                if let Some(path) = args.next() {
                    config_paths.push(path);
                }
            }
            "-h" | "--help" => {
                help = true;
            }
            other => {
                positional_args.push(other.to_string());
            }
        }
    }

    if help {
        render::print_help();
        std::process::exit(0);
    }

    let user_prompt = positional_args.join(" ");

    // Check if we should drop into interactive mode (no prompt provided)
    let is_interactive = user_prompt.trim().is_empty();
    if is_interactive {
        println!("✨ Entering Interactive Mode (Stub)...");
        // COMMENT: Stub for future interactive session mode.
        std::process::exit(0);
    }

    // 2. Resolve Config Files and Merge them
    let mut config = Config::default();
    if config_paths.is_empty() {
        if let Ok(default_path) = resolve_config_path(None) {
            config = load_config(&default_path);
        }
    } else {
        for path_str in &config_paths {
            let path = resolve_config_path(Some(path_str))?;
            let loaded = load_config(&path);
            config.merge(loaded);
        }
    }

    // 3. Inject keys dynamically into environment before initializing Client
    if let Some(key) = &config.api_key {
        let env_var = match config.adapter.as_deref().unwrap_or("openai") {
            "gemini" => "GEMINI_API_KEY",
            "anthropic" => "ANTHROPIC_API_KEY",
            _ => "OPENAI_API_KEY",
        };
        unsafe {
            std::env::set_var(env_var, key);
        }
    }

    // 4. Initialize GenAI Client with a custom model mapper and service target resolver
    let config_adapter = config.adapter.clone();
    
    let model_mapper = genai::resolver::ModelMapper::from_mapper_fn(move |model_iden: genai::ModelIden| {
        if let Some(ref target_adapter) = config_adapter {
            let mapped_kind = match target_adapter.to_lowercase().as_str() {
                "openai" => genai::adapter::AdapterKind::OpenAI,
                "gemini" => genai::adapter::AdapterKind::Gemini,
                "anthropic" => genai::adapter::AdapterKind::Anthropic,
                _ => model_iden.adapter_kind,
            };
            Ok(genai::ModelIden::new(mapped_kind, model_iden.model_name))
        } else {
            Ok(model_iden)
        }
    });

    let custom_endpoint = config.endpoint.clone();
    let target_resolver = genai::resolver::ServiceTargetResolver::from_resolver_fn(move |mut target: genai::ServiceTarget| {
        if let Some(ref ep) = custom_endpoint {
            let mut url = ep.clone();
            if !url.ends_with('/') {
                url.push('/');
            }
            target.endpoint = genai::resolver::Endpoint::from_owned(url);
        }
        Ok(target)
    });

    let client = genai::Client::builder()
        .with_model_mapper(model_mapper)
        .with_service_target_resolver(target_resolver)
        .build();

    let model = config
        .model
        .clone()
        .unwrap_or_else(|| std::env::var("BONDAGE_MODEL").unwrap_or_else(|_| "gemini-1.5-flash".to_string()));

    let current_dir = std::env::current_dir()?;

    let processed_prompt = bondage::prompt_file_injector::process_prompt(&user_prompt)?;

    // 5. Setup policy, tools and history
    let policy = bondage::policy::Policy::from_config(&config.policy);
    let tools = bondage::tools::get_standard_tools();
    let mut history = vec![
        Message::System("You are Bondage, a stateless actor core. You have access to the 'lookup' and 'write' tools. Use 'lookup' to inspect files or directories, and 'write' to create, edit, or patch files. Keep your answers concise.".to_string()),
        Message::User(processed_prompt),
    ];

    println!("🤖 Invoking {}...", model);

    loop {
        let response_msgs = step_stream(&client, &model, &history, &tools, None, &|token| {
            print!("{}", token);
            let _ = io::stdout().flush();
        }).await?;

        println!();

        let mut has_tool_calls = false;
        
        for msg in response_msgs {
            history.push(msg.clone());
            
            if let Message::ModelToolRequest { id, name, arguments } = msg {
                has_tool_calls = true;
                
                let policy_mode = match name.as_str() {
                    "lookup" => {
                        if let Ok(args) = serde_json::from_str::<bondage::tools::tool_lookup::LookupArgs>(&arguments) {
                            policy.check_lookup(&args.target, &current_dir)
                        } else {
                            bondage::policy::PolicyMode::Ask
                        }
                    }
                    "write" => {
                        if let Ok(args) = serde_json::from_str::<bondage::tools::tool_write::WriteArgs>(&arguments) {
                            policy.check_write(&args.path, &current_dir)
                        } else {
                            bondage::policy::PolicyMode::Ask
                        }
                    }
                    "bash" => {
                        policy.check_bash()
                    }
                    _ => bondage::policy::PolicyMode::Ask,
                };

                let approved = match policy_mode {
                    bondage::policy::PolicyMode::Yes => Some(true),
                    bondage::policy::PolicyMode::No => Some(false),
                    bondage::policy::PolicyMode::Ask => {
                        if name == "bash" {
                            Some(true) // Auto-approve the launch for bash; the popped window is the approval mechanism
                        } else if ask_approval(&name, &arguments) {
                            Some(true)
                        } else {
                            None // Denied by user
                        }
                    }
                };

                if approved == Some(true) {
                    if policy_mode == bondage::policy::PolicyMode::Yes {
                        println!("\n⚡ [Auto-approved by Policy] {} ({})", name, arguments.trim());
                    }
                    println!("▶️ Executing {}...", name);
                    
                    let tool_result = if name == "bash" {
                        execute_bash_tmux(&id, &arguments, &current_dir, policy_mode, config.terminal.clone()).await
                    } else {
                        bondage::tools::execute_tool(&id, &name, &arguments, &current_dir).await
                    };
                    
                    if let Message::ToolResponse { content, is_error, .. } = &tool_result {
                        let status = if *is_error { "ERROR" } else { "SUCCESS" };
                        let preview: String = content.lines().take(5).collect::<Vec<_>>().join("\n");
                        println!("✅ [{}] Output preview:\n{}\n...", status, preview);
                    }
                    
                    history.push(tool_result);
                } else {
                    let (reason, _is_policy_block) = match policy_mode {
                        bondage::policy::PolicyMode::No => {
                            println!("❌ [Blocked by Policy] Rejection sent to agent.");
                            // COMMENT: We report a policy block message here. Can customize this report later.
                            ("Permission Denied: execution blocked by safety policy.".to_string(), true)
                        }
                        _ => {
                            println!("❌ Denied.");
                            ("Permission Denied by User".to_string(), false)
                        }
                    };
                    history.push(Message::ToolResponse {
                        id,
                        name,
                        content: reason,
                        is_error: true,
                    });
                }
            }
        }

        if !has_tool_calls {
            break;
        }
    }

    let pid = std::process::id();
    let session_name = format!("rope-shell-{}", pid);
    if tmux::has_session(&session_name) {
        let _ = tmux::kill_session(&session_name);
    }

    Ok(())
}
