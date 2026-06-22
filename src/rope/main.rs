mod config;
mod tmux_utils;
mod tmux_orchestration;
mod render;
mod interactive;

use std::io::{self, Write};
use bondage::{Message, step_stream};
use config::{Config, load_config, resolve_config_path, ensure_config_installed};
use tmux_orchestration::execute_bash_tmux;

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

    // 1. Parse arguments: -c/--config, -h/--help, -l/--log, -i/--interactive and collect positional prompt
    let mut config_paths = Vec::new();
    let mut help = false;
    let mut enable_logging = false;
    let mut interactive_file = None;
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
            "-l" | "--log" => {
                enable_logging = true;
            }
            "-i" | "--interactive" => {
                if let Some(path) = args.next() {
                    interactive_file = Some(path);
                } else {
                    eprintln!("Error: -i/--interactive requires a file path argument.");
                    std::process::exit(1);
                }
            }
            other => {
                positional_args.push(other.to_string());
            }
        }
    }

    if enable_logging {
        tmux_orchestration::ENABLE_LOGGING.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    if help {
        render::print_help();
        std::process::exit(0);
    }

    let user_prompt = positional_args.join(" ");

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

    // If interactive mode file is specified, launch it
    if let Some(ref file_path) = interactive_file {
        let path = std::path::PathBuf::from(file_path);
        if let Err(e) = interactive::run_file_sitter(path, config, client, model).await {
            eprintln!("Error in interactive file-sitter: {}", e);
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    if user_prompt.trim().is_empty() {
        println!("✨ Entering Interactive Mode (Stub)...");
        println!("Use `rope -i <session_file.md>` to launch the file-sitter interactive mode.");
        std::process::exit(0);
    }

    let current_dir = std::env::current_dir()?;

    let processed_prompt = bondage::prompt_file_injector::process_prompt(&user_prompt)?;

    // 5. Setup policy, tools and history
    let policy = bondage::policy::Policy::from_config(&config.policy);
    let tools = bondage::tools::get_standard_tools();
    let tools_block = bondage::util::format_tools_block(&tools);
    let system_prompt = include_str!("../../docs/system-regular.txt")
        .replace("{TOOLS}", &tools_block);
    let mut history = vec![
        Message::System(system_prompt),
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
                        } else if tmux_orchestration::ask_approval(&name, &arguments) {
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
    if tmux_utils::has_session(&session_name) {
        let _ = tmux_utils::kill_session(&session_name);
    }

    Ok(())
}
