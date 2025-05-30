use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::NamedTempFile;
use std::io::Write;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Assuming your main application logic can be triggered by a function.
// If main.rs's main is not easily callable, you might need to refactor main.rs
// or expose a lib function. For now, let's assume we can call a setup function
// or that `uptime_monitor::main()` can be used if it's part of a library.
// We will need to ensure `main.rs` functions are accessible or refactor.
// For this example, let's assume `uptime_monitor::main()` is available.
// If not, we'll need to adjust how the app is started.

// No need for get_target_bin if we call run_app directly.

// This makes items from the crate's main.rs (if it were a library) or lib.rs accessible.
// For a binary crate, we rely on `uptime_monitor::run_app` being available.
// If `run_app` is in `src/main.rs`, it needs to be `pub` and the crate needs to be
// referenced correctly. Integration tests treat the crate like an external one.
// So, `uptime_monitor::run_app` should work if `src/main.rs` is the main entry point.
// If there was a `src/lib.rs`, `run_app` would need to be exposed there.

#[tokio::test]
async fn test_metrics_endpoint_prometheus_format_simple() {
    // Initialize env_logger for test output if desired
    // let _ = env_logger::builder().is_test(true).try_init(); // Uncomment to see logs from the app

    // 1. Setup Mock HTTP Servers
    let mock_server_healthy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Healthy mock server"))
        .mount(&mock_server_healthy)
        .await;

    let mock_server_unhealthy = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/unhealthy"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Unhealthy mock server")) // Will be reported as unhealthy by app
        .mount(&mock_server_unhealthy)
        .await;
    
    let mock_server_regex_pass = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/regex_pass"))
        .respond_with(ResponseTemplate::new(200).set_body_string("This body contains the word SUCCESS"))
        .mount(&mock_server_regex_pass)
        .await;

    let mock_server_regex_fail = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/regex_fail"))
        .respond_with(ResponseTemplate::new(200).set_body_string("This body does not contain the magic word"))
        .mount(&mock_server_regex_fail)
        .await;
    
    // Using one of the mock server ports for a TCP check that should be open
    let open_tcp_port = mock_server_healthy.address().port();
    let closed_tcp_port = 34567; // A port that is likely closed

    // 2. Create a Test Configuration
    let test_config_content = format!(
        r#"
monitoring_interval_seconds = 3 # Short interval for testing

[[hosts]]
address = "{}"
alias = "MockHealthyHttp"
  [[hosts.checks]]
  type = "Http"
  port = {}
  path = "/"
  protocol = "Http"
  method = "Get"
  expected_status_code = 200

[[hosts]]
address = "{}"
alias = "MockUnhealthyHttp"
  [[hosts.checks]]
  type = "Http"
  port = {}
  path = "/unhealthy"
  protocol = "Http"
  method = "Get"
  expected_status_code = 200 # App expects 200, server gives 500

[[hosts]]
address = "{}"
alias = "MockRegexPassHttp"
  [[hosts.checks]]
  type = "Http"
  port = {}
  path = "/regex_pass"
  protocol = "Http"
  method = "Get"
  expected_status_code = 200
  body_regex_check = "SUCCESS"

[[hosts]]
address = "{}"
alias = "MockRegexFailHttp"
  [[hosts.checks]]
  type = "Http"
  port = {}
  path = "/regex_fail"
  protocol = "Http"
  method = "Get"
  expected_status_code = 200
  body_regex_check = "SUCCESS" # This will fail

[[hosts]]
address = "127.0.0.1"
alias = "LocalTcpOpen"
  [[hosts.checks]]
  type = "Tcp"
  port = {} # Use an open port from one of the mock servers
  timeout_seconds = 1

[[hosts]]
address = "127.0.0.1"
alias = "LocalTcpClosed"
  [[hosts.checks]]
  type = "Tcp"
  port = {} # Use a likely closed port
  timeout_seconds = 1
"#,
        mock_server_healthy.address().ip(), mock_server_healthy.address().port(),
        mock_server_unhealthy.address().ip(), mock_server_unhealthy.address().port(),
        mock_server_regex_pass.address().ip(), mock_server_regex_pass.address().port(),
        mock_server_regex_fail.address().ip(), mock_server_regex_fail.address().port(),
        open_tcp_port,
        closed_tcp_port
    );

    let mut tmp_config_file = NamedTempFile::new().unwrap();
    writeln!(tmp_config_file, "{}", test_config_content).unwrap();
    let config_path = tmp_config_file.path().to_str().unwrap().to_string();

    // 3. Start the Uptime Monitor Application
    std::env::set_var("TEST_CONFIG_PATH", &config_path);
    let test_app_server_address = "127.0.0.1:8088"; // Fixed port for test app's web server
    std::env::set_var("TEST_SERVER_ADDRESS", test_app_server_address);
                                                                
    let app_thread = tokio::spawn(async move {
        if let Err(e) = uptime_monitor::run_app().await { // Call the public run_app
            eprintln!("[Test App Thread] Uptime monitor run_app function failed: {}", e);
        }
    });

    // Allow time for the app to start and perform some checks.
    // Interval is 3s. Allow for 2-3 cycles + startup time.
    tokio::time::sleep(Duration::from_secs(10)).await; 

    // 4. Test Logic: Make HTTP GET request to the /metrics endpoint
    let client = reqwest::Client::new();
    let metrics_url = format!("http://{}/metrics", test_app_server_address);
    
    let response = match client.get(&metrics_url).send().await {
        Ok(resp) => resp,
        Err(e) => {
            app_thread.abort(); // Stop the app if we can't even connect
            panic!("Failed to connect to metrics endpoint {}: {}", metrics_url, e);
        }
    };

    assert_eq!(response.status(), reqwest::StatusCode::OK, "Metrics endpoint should return 200 OK");
    assert_eq!(response.headers().get("content-type").unwrap(), "text/plain; version=0.0.4; charset=utf-8", "Incorrect content type");
    
    let metrics_body = response.text().await.unwrap();
    println!("--- Metrics Body ---\n{}\n--- End Metrics Body ---", metrics_body);

    // Assertions
    // Healthy HTTP target
    assert!(metrics_body.contains("uptime_status_health{target_alias=\"MockHealthyHttp\", check_type=\"http\"} 1"), "MockHealthyHttp health status");
    assert!(metrics_body.contains("uptime_response_time_seconds{target_alias=\"MockHealthyHttp\", check_type=\"http\"}"), "MockHealthyHttp response time");
    assert!(metrics_body.contains("uptime_consecutive_failures_total{target_alias=\"MockHealthyHttp\", check_type=\"http\"} 0"), "MockHealthyHttp failures");

    // Unhealthy HTTP target (wrong status code)
    assert!(metrics_body.contains("uptime_status_health{target_alias=\"MockUnhealthyHttp\", check_type=\"http\"} 0"), "MockUnhealthyHttp health status");
    // No response time metric for failed checks if failure is before sending/getting response, but wiremock gives a response, so it should be there.
    assert!(metrics_body.contains("uptime_response_time_seconds{target_alias=\"MockUnhealthyHttp\", check_type=\"http\"}"), "MockUnhealthyHttp response time for 500");
    assert!(metrics_body.contains("uptime_consecutive_failures_total{target_alias=\"MockUnhealthyHttp\", check_type=\"http\"}")); // Value could be >0

    // Regex pass HTTP target
    assert!(metrics_body.contains("uptime_status_health{target_alias=\"MockRegexPassHttp\", check_type=\"http\"} 1"), "MockRegexPassHttp health status");
    assert!(metrics_body.contains("uptime_consecutive_failures_total{target_alias=\"MockRegexPassHttp\", check_type=\"http\"} 0"), "MockRegexPassHttp failures");

    // Regex fail HTTP target
    assert!(metrics_body.contains("uptime_status_health{target_alias=\"MockRegexFailHttp\", check_type=\"http\"} 0"), "MockRegexFailHttp health status");
    assert!(metrics_body.contains("uptime_consecutive_failures_total{target_alias=\"MockRegexFailHttp\", check_type=\"http\"}")); // Value could be >0
    
    // TCP Checks
    // The health of LocalTcpOpen depends on whether the port used by mock_server_healthy is connectable via raw TCP.
    // Wiremock opens the port, so it should be connectable.
    assert!(metrics_body.contains("uptime_status_health{target_alias=\"LocalTcpOpen\", check_type=\"tcp\"} 1"), "LocalTcpOpen health status");
    assert!(metrics_body.contains("uptime_consecutive_failures_total{target_alias=\"LocalTcpOpen\", check_type=\"tcp\"} 0"), "LocalTcpOpen failures");

    assert!(metrics_body.contains("uptime_status_health{target_alias=\"LocalTcpClosed\", check_type=\"tcp\"} 0"), "LocalTcpClosed health status");
    assert!(metrics_body.contains("uptime_consecutive_failures_total{target_alias=\"LocalTcpClosed\", check_type=\"tcp\"}")); // Value could be >0
    
    // Check that HELP and TYPE lines are present (once per metric name)
    assert_eq!(metrics_body.matches("# HELP uptime_status_health").count(), 1, "HELP for status_health");
    assert_eq!(metrics_body.matches("# TYPE uptime_status_health gauge").count(), 1, "TYPE for status_health");
    assert_eq!(metrics_body.matches("# HELP uptime_response_time_seconds").count(), 1, "HELP for response_time");
    assert_eq!(metrics_body.matches("# TYPE uptime_response_time_seconds gauge").count(), 1, "TYPE for response_time");
    assert_eq!(metrics_body.matches("# HELP uptime_consecutive_failures_total").count(), 1, "HELP for failures_total");
    assert_eq!(metrics_body.matches("# TYPE uptime_consecutive_failures_total counter").count(), 1, "TYPE for failures_total");

    // 5. Teardown
    app_thread.abort(); // Abort the app task after the test
}
