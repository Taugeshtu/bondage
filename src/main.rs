use std::path::PathBuf;
use std::io::{self, Write};
use serde::Deserialize;
use bondage::{Message, step_stream};

#[derive(Deserialize, Debug, Default)]
struct Config {
    model: Option<String>,
    api_key: Option<String>,
    endpoint: Option<String>,
    adapter: Option<String>,
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

    // Default to the first candidate as a direct path if nothing exists
    Ok(PathBuf::from(&candidates[0]))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Parse arguments: -c/--config, -y/--yolo, and collect positional prompt
    let mut config_path_str = None;
    let mut yolo = false;
    let mut positional_args = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                config_path_str = args.next();
            }
            "-y" | "--yolo" => {
                yolo = true;
            }
            other => {
                positional_args.push(other.to_string());
            }
        }
    }

    if positional_args.is_empty() {
        eprintln!("Usage: rope [-c <config_path>] [-y|--yolo] <prompt...>");
        std::process::exit(1);
    }
    let user_prompt = positional_args.join(" ");

    // 2. Resolve Config File
    let config_path = resolve_config_path(config_path_str.as_deref())?;

    let config = load_config(&config_path);

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

    // 5. Setup tools and history
    let tools = bondage::tools::get_standard_tools();
    let mut history = vec![
        Message::System("You are Bondage, a stateless actor core. You have access to a 'lookup' tool. Always look up files or directories if you need more information to answer the user's request. Keep your answers concise.".to_string()),
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
                
                let approved = yolo || ask_approval(&name, &arguments);
                if approved {
                    if yolo {
                        println!("\n⚡ [YOLO Mode] Auto-approving execution of: {} ({})", name, arguments.trim());
                    }
                    println!("▶️ Executing {}...", name);
                    let tool_result = bondage::tools::execute_tool(&id, &name, &arguments, &current_dir).await;
                    
                    if let Message::ToolResponse { content, is_error, .. } = &tool_result {
                        let status = if *is_error { "ERROR" } else { "SUCCESS" };
                        let preview: String = content.lines().take(5).collect::<Vec<_>>().join("\n");
                        println!("✅ [{}] Output preview:\n{}\n...", status, preview);
                    }
                    
                    history.push(tool_result);
                } else {
                    println!("❌ Denied.");
                    history.push(Message::ToolResponse {
                        id,
                        name,
                        content: "Permission Denied by User".to_string(),
                        is_error: true,
                    });
                }
            }
        }

        if !has_tool_calls {
            break;
        }
    }

    Ok(())
}
