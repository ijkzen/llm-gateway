use llm_gateway::config::Config;

#[tokio::main]
async fn main() {
    let config = Config::from_env().expect("Failed to load config");
    if let Err(e) = llm_gateway::run(config).await {
        eprintln!("Application failed: {}", e);
        std::process::exit(1);
    }
}
