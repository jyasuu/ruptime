// Re-export modules for testing
pub mod config;
pub mod monitoring;
pub mod api;

// Application runner function for main and tests
pub async fn run_app() -> std::io::Result<()> {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use crate::monitoring::TargetStatus;
    use std::process;
    use log::{info, error};

    const DEFAULT_CONFIG_FILE_PATH: &str = "config.toml";
    const DEFAULT_SERVER_ADDRESS: &str = "0.0.0.0:8080";

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

// Test utilities
pub mod test_utils {
    use crate::config::*;
    use serde_json::json;
    
    pub fn create_test_assertion(
        query: AssertionQuery,
        predicate: AssertionPredicate,
        value: AssertionValue,
    ) -> HttpAssertion {
        HttpAssertion {
            query,
            predicate,
            value,
        }
    }
    
    pub fn create_mock_response_body() -> String {
        json!({
            "args": {
                "test_param": "hello",
                "number": "42"
            },
            "headers": {
                "Accept": "application/json",
                "User-Agent": "test-agent"
            },
            "origin": "192.168.1.100",
            "url": "https://httpbin.org/get?test_param=hello&number=42",
            "authenticated": true,
            "user": "testuser",
            "uuid": "550e8400-e29b-41d4-a716-446655440000",
            "timestamp": "2024-01-15T10:30:00Z"
        }).to_string()
    }
}