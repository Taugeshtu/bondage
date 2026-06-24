mod config;
mod tmux_utils;
mod tmux_orchestration;
mod render;
mod interactive;

use std::io::{self, Write};
use bondage::Message;
use bondage::kit::{step_agent, ToolExecutor};
use config::{Config, load_config, ensure_resources_installed};
use tmux_orchestration::execute_bash_tmux;

struct TmuxExecutor {
    terminal: Option<String>,
}

#[async_trait::async_trait]
impl ToolExecutor for TmuxExecutor {
    async fn execute(
        &self,
        call: &bondage::ToolCall,
        policy: &bondage::policy::Policy,
        current_dir: &std::path::Path,
    ) -> Message {
        let policy_mode = match call.name.as_str() {
            "lookup" => {
                if let Ok(args) = serde_json::from_str::<bondage::tools::tool_lookup::LookupArgs>(&call.arguments) {
                    policy.check_lookup(&args.target, current_dir)
                } else {
                    bondage::policy::PolicyMode::Ask
                }
            }
            "write" => {
                if let Ok(args) = serde_json::from_str::<bondage::tools::tool_write::WriteArgs>(&call.arguments) {
                    policy.check_write(&args.path, current_dir)
                } else {
                    bondage::policy::PolicyMode::Ask
                }
            }
            "bash" => policy.check_bash(),
            _ => bondage::policy::PolicyMode::Ask,
        };

        let approved = match policy_mode {
            bondage::policy::PolicyMode::Yes => Some(true),
            bondage::policy::PolicyMode::No => Some(false),
            bondage::policy::PolicyMode::Ask => {
                if call.name == "bash" {
                    Some(true)
                } else if tmux_orchestration::ask_approval(&call.name, &call.arguments) {
                    Some(true)
                } else {
                    None
                }
            }
        };

        if approved == Some(true) {
            if policy_mode == bondage::policy::PolicyMode::Yes {
                println!("\n⚡ [Auto-approved by Policy] {} ({})", call.name, call.arguments.trim());
            }
            println!("▶️ Executing {}...", call.name);

            let tool_result = if call.name == "bash" {
                execute_bash_tmux(&call.id, &call.arguments, current_dir, policy_mode, self.terminal.clone()).await
            } else {
                bondage::tools::execute_tool(&call.id, &call.name, &call.arguments, current_dir).await
            };

            if let Message::ToolResponse { content, is_error, .. } = &tool_result {
                let status = if *is_error { "ERROR" } else { "SUCCESS" };
                let preview: String = content.lines().take(5).collect::<Vec<_>>().join("\n");
                println!("✅ [{}] Output preview:\n{}\n...", status, preview);
            }

            tool_result
        } else {
            let reason = match policy_mode {
                bondage::policy::PolicyMode::No => {
                    println!("❌ [Blocked by Policy] Rejection sent to agent.");
                    "Permission Denied: execution blocked by safety policy.".to_string()
                }
                _ => {
                    println!("❌ Denied.");
                    "Permission Denied by User".to_string()
                }
            };
            Message::ToolResponse {
                id: call.id.clone(),
                name: call.name.clone(),
                content: reason,
                is_error: true,
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Parse arguments: -c/--config, -s/--system, -h/--help, -l/--log, -i/--interactive, --no-tmux/--notmux, and prompt
    let mut config_paths = Vec::new();
    let mut system_paths = Vec::new();
    let mut help = false;
    let mut enable_logging = false;
    let mut interactive_file = None;
    let mut no_tmux = false;
    let mut positional_args = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                if let Some(path) = args.next() {
                    config_paths.push(path);
                }
            }
            "-s" | "--system" => {
                if let Some(path) = args.next() {
                    system_paths.push(path);
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
            "--no-tmux" | "--notmux" => {
                no_tmux = true;
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

    // Ensure baseline config/prompt resources are exported to ~/.config/rope/
    ensure_resources_installed()?;

    let current_dir = std::env::current_dir()?;
    let mut init_errors = Vec::new();

    // 2. Resolve Config Files and Merge them
    let mut config = Config::default();
    if config_paths.is_empty() {
        // Search config.toml implicitly (only in ~/.config/rope/)
        match bondage::util::locate_resource("config.toml", false, &current_dir) {
            Ok(path) => {
                config = load_config(&path);
            }
            Err(_) => {
                // Config missing by default -> write template and error/abort
                init_errors.push("Config file 'config.toml' was not found. A default configuration template has been created at ~/.config/rope/config.toml. Please configure it and rerun.".to_string());
            }
        }
    } else {
        // Explicit configs (checks CWD first, then ~/.config/rope/)
        for name_str in &config_paths {
            let filename = if name_str.to_lowercase().ends_with(".toml") {
                name_str.to_string()
            } else {
                format!("{}.toml", name_str)
            };
            match bondage::util::locate_resource(&filename, true, &current_dir) {
                Ok(path) => {
                    let loaded = load_config(&path);
                    config.merge(loaded);
                }
                Err(err) => {
                    init_errors.push(err);
                }
            }
        }
    }

    // 3. Resolve System Prompts and Overlay/Concatenate them
    let is_interactive = interactive_file.is_some();
    let mut system_prompt_template = String::new();

    if system_paths.is_empty() {
        // Default system prompt implicitly from ~/.config/rope/ only
        let default_prompt_name = if is_interactive {
            "system-interactive.txt"
        } else {
            "system-regular.txt"
        };
        match bondage::util::locate_resource(default_prompt_name, false, &current_dir) {
            Ok(path) => {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    system_prompt_template = content;
                }
            }
            Err(_) => {
                // Safe compile-time fallback
                system_prompt_template = if is_interactive {
                    include_str!("../../docs/system-interactive.txt").to_string()
                } else {
                    include_str!("../../docs/system-regular.txt").to_string()
                };
            }
        }
    } else {
        // Explicit system prompts (checks CWD first, then ~/.config/rope/)
        for name_str in &system_paths {
            let mut resolved_path = None;
            if let Ok(path) = bondage::util::locate_resource(name_str, true, &current_dir) {
                resolved_path = Some(path);
            } else if !name_str.to_lowercase().ends_with(".txt") {
                let txt_name = format!("{}.txt", name_str);
                if let Ok(path) = bondage::util::locate_resource(&txt_name, true, &current_dir) {
                    resolved_path = Some(path);
                }
            }

            if let Some(path) = resolved_path {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if !system_prompt_template.is_empty() {
                        system_prompt_template.push_str("\n\n");
                    }
                    system_prompt_template.push_str(&content);
                } else {
                    init_errors.push(format!("Failed to read system prompt file at '{}'", path.display()));
                }
            } else {
                init_errors.push(format!("System prompt resource '{}' not found in CWD or ~/.config/rope/", name_str));
            }
        }
    }

    // 4. Resolve Prompt and check for missing @file reference resources
    let mut processed_prompt = String::new();
    if !is_interactive && !user_prompt.trim().is_empty() {
        match bondage::prompt_file_injector::process_prompt_in_dir(&user_prompt, &current_dir) {
            Ok(prompt) => {
                processed_prompt = prompt;
            }
            Err(mut file_errors) => {
                init_errors.append(&mut file_errors);
            }
        }
    }

    // 5. Check validation/init errors and abort if any exist
    if !init_errors.is_empty() {
        for err in init_errors {
            eprintln!("❌ Error: {}", err);
        }
        std::process::exit(1);
    }

    // 6. Tmux bootstrapping if TTY, not in TMUX, and --no-tmux not passed
    let is_gui = std::env::var("WAYLAND_DISPLAY").is_ok() || std::env::var("DISPLAY").is_ok();
    let in_tmux = std::env::var("TMUX").is_ok();
    
    if !no_tmux && !is_gui && !in_tmux {
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

    // 7. Inject API keys dynamically into environment
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

    // 8. Initialize GenAI Client with model mapper and service target resolver
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

    // 9. If interactive mode file is specified, launch it
    if let Some(ref file_path) = interactive_file {
        let path = std::path::PathBuf::from(file_path);
        if let Err(e) = interactive::run_file_sitter(path, config, client, model, system_prompt_template).await {
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

    // 10. Run normal prompt turn
    let policy = bondage::policy::Policy::from_config(&config.policy);
    let tools = bondage::tools::get_standard_tools();
    
    let tools_block = bondage::util::format_tools_block(&tools);
    let final_system_prompt = system_prompt_template.replace("{TOOLS}", &tools_block);

    let mut history = vec![
        Message::System(final_system_prompt),
        Message::User(processed_prompt),
    ];

    println!("🤖 Invoking {}...", model);

    let executor = TmuxExecutor { terminal: config.terminal.clone() };
    step_agent(&client, &model, &mut history, &tools, None, &current_dir, &policy, &executor, &|token| {
        print!("{}", token);
        let _ = io::stdout().flush();
    }).await?;

    let pid = std::process::id();
    let session_name = format!("rope-shell-{}", pid);
    if tmux_utils::has_session(&session_name) {
        let _ = tmux_utils::kill_session(&session_name);
    }

    Ok(())
}
