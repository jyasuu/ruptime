use std::sync::Arc;
use tokio::sync::Mutex;

mod config;
mod monitoring;
mod api;

use crate::monitoring::TargetStatus;
use std::process;
use log::{info, error}; // Added log macros

const DEFAULT_CONFIG_FILE_PATH: &str = "config.toml";
const DEFAULT_SERVER_ADDRESS: &str = "0.0.0.0:8080";

// This 'main' function is what `cargo run` executes.
// For integration tests, we might call `run_app` directly if we make it public,
// or the test will spawn this `main` as a process (more complex) or this `main`
// needs to be structured to be callable for tests (e.g. if it were in lib.rs).
// Given it's in main.rs, the env var approach is simplest for now without major refactor.
#[tokio::main]
async fn main() -> std::io::Result<()> {
    run_app().await
}

// Extracted core logic to be potentially callable or to keep main clean.
pub async fn run_app() -> std::io::Result<()> {
    env_logger::init();
    info!("Starting uptime_monitor application");

    let config_path = std::env::var("TEST_CONFIG_PATH")
        .unwrap_or_else(|_| DEFAULT_CONFIG_FILE_PATH.to_string());
    
    let server_address = std::env::var("TEST_SERVER_ADDRESS")
        .unwrap_or_else(|_| DEFAULT_SERVER_ADDRESS.to_string());

    info!("Using configuration file: {}", config_path);
    info!("Attempting to bind server to: {}", server_address);

    let loaded_config = match config::load_config(&config_path) {
        Ok(cfg) => {
            info!("Configuration loaded successfully from {}", config_path);
            cfg
        }
        Err(_e) => { // Error is already logged by load_config
            error!("Exiting due to configuration load failure from {}.", config_path);
            process::exit(1);
        }
    };

    let app_config = Arc::new(loaded_config);
    let shared_target_statuses: Arc<tokio::sync::Mutex<Vec<TargetStatus>>> = Arc::new(Mutex::new(Vec::new()));

    let app_config_clone = Arc::clone(&app_config);
    let statuses_clone_monitor = Arc::clone(&shared_target_statuses);

    info!("Spawning monitoring loop task");
    tokio::spawn(async move {
        monitoring::run_monitoring_loop(app_config_clone, statuses_clone_monitor).await;
    });

    info!("Attempting to start HTTP server on {}", server_address);
    if let Err(e) = api::start_web_server(server_address.clone(), shared_target_statuses.clone()).await {
        error!("Failed to start HTTP server on {}: {}", server_address, e);
        process::exit(1); // Exit if server fails to start
    }
    
    Ok(())
}
