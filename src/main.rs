use std::path::PathBuf;
use std::io::{self, Write};
use serde::Deserialize;
use bondage::{Message, step_stream};

#[derive(Deserialize, Debug, Default)]
struct Config {
    model: Option<String>,
    api_key: Option<String>,
    endpoint: Option<String>,
    provider: Option<String>,
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Parse arguments: -c/--config, -p/--prompt, and positional fallback
    let mut config_path_str = None;
    let mut prompt = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                config_path_str = args.next();
            }
            "-p" | "--prompt" => {
                prompt = args.next();
            }
            other => {
                if prompt.is_none() {
                    prompt = Some(other.to_string());
                }
            }
        }
    }

    let user_prompt = match prompt {
        Some(p) => p,
        None => {
            eprintln!("Usage: rope [-c <config_path>] [-p <prompt>] or rope <prompt>");
            std::process::exit(1);
        }
    };

    // 2. Resolve Config File
    let config_path = match config_path_str {
        Some(p) => PathBuf::from(p),
        None => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".config/rope/config.toml")
        }
    };

    let config = load_config(&config_path);

    // 3. Inject keys/endpoints dynamically into environment before initializing Client
    if let Some(key) = &config.api_key {
        let env_var = match config.provider.as_deref().unwrap_or("openai") {
            "gemini" => "GEMINI_API_KEY",
            "anthropic" => "ANTHROPIC_API_KEY",
            _ => "OPENAI_API_KEY",
        };
        unsafe {
            std::env::set_var(env_var, key);
        }
    }

    if let Some(endpoint) = &config.endpoint {
        let env_var = match config.provider.as_deref().unwrap_or("openai") {
            _ => "OPENAI_API_BASE",
        };
        unsafe {
            std::env::set_var(env_var, endpoint);
        }
    }

    // 4. Initialize GenAI Client with a custom model mapper to route custom models
    // to the correct provider adapter (overriding the default prefix fallback to Ollama).
    let provider = config.provider.clone().unwrap_or_else(|| "openai".to_string());
    
    let model_mapper = genai::resolver::ModelMapper::from_mapper_fn(move |model_iden: genai::ModelIden| {
        let target_provider = provider.to_lowercase();
        if model_iden.adapter_kind == genai::adapter::AdapterKind::Ollama {
            let mapped_kind = match target_provider.as_str() {
                "openai" => genai::adapter::AdapterKind::OpenAI,
                "gemini" => genai::adapter::AdapterKind::Gemini,
                "anthropic" => genai::adapter::AdapterKind::Anthropic,
                _ => genai::adapter::AdapterKind::Ollama,
            };
            Ok(genai::ModelIden::new(mapped_kind, model_iden.model_name))
        } else {
            Ok(model_iden)
        }
    });

    let client = genai::Client::builder()
        .with_model_mapper(model_mapper)
        .build();

    let model = config
        .model
        .clone()
        .unwrap_or_else(|| std::env::var("BONDAGE_MODEL").unwrap_or_else(|_| "gemini-1.5-flash".to_string()));

    let current_dir = std::env::current_dir()?;

    // 5. Setup tools and history
    let tools = bondage::tools::get_standard_tools();
    let mut history = vec![
        Message::System("You are Bondage, a stateless actor core. You have access to a 'lookup' tool. Always look up files or directories if you need more information to answer the user's request. Keep your answers concise.".to_string()),
        Message::User(user_prompt),
    ];

    println!("🤖 Invoking {} (using config: {})...", model, config_path.display());

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
                
                if ask_approval(&name, &arguments) {
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
