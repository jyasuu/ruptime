use std::time::{Duration, Instant, SystemTime};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::net::TcpStream;
use tokio::time::timeout;
use reqwest::{Client, StatusCode};
use regex::Regex;
use crate::config::{AppConfig, Check, HttpCheck, HttpProtocol, HttpMethod as ConfigHttpMethod};
use log::{info, warn, error, debug}; // Added log macros
use native_tls::TlsConnector;
use openssl::x509::X509;
// std::io::{Read, Write} was removed as Read and Write are unused.
// std::io::Error and std::io::ErrorKind are used via full path.
use std::net::TcpStream as StdTcpStream; // For native-tls
use chrono::{DateTime, Utc};

// --- Data structures for storing check status ---

#[derive(Debug, Clone, serde::Serialize)]
pub enum CheckResult {
    Tcp(TcpCheckResult),
    Http(HttpCheckResultDetails),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TcpCheckResult {
    // Using Result<(), String> directly for simplicity as per current check_tcp_port
    pub result: Result<(), String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HttpCheckResultDetails {
    // This is the HttpCheckResult from the previous step
    pub status: CheckStatus,
    pub response_time_ms: u128,
    pub cert_days_remaining: Option<i64>,
    pub cert_is_valid: Option<bool>,
}

// This mirrors the HttpCheckResult's status for now
// This mirrors the HttpCheckResult's status for now
#[derive(Debug, Clone, serde::Serialize)] // Added Serialize for potential future JSON output
pub enum CheckStatus {
    Healthy,
    Unhealthy(String),
}


#[derive(Debug, Clone, serde::Serialize)] // Added Serialize
pub struct TargetStatus {
    pub target_alias: String,
    #[serde(serialize_with = "serialize_system_time")]
    pub last_check_time: Option<SystemTime>,
    pub last_result: Option<CheckResult>, // CheckResult needs to be Serialize
    pub consecutive_failures: u32,
    pub is_healthy: bool,
    // Placeholders for now
    pub uptime_percentage_24h: f32,
    pub average_response_time_24h_ms: f32,
    pub monitor_url: String,
    pub monitor_hostname: String,
    pub monitor_port: u16,
    pub cert_days_remaining: Option<i64>,
    pub cert_is_valid: Option<bool>,
}

// Helper for serializing SystemTime option
fn serialize_system_time<S>(time: &Option<SystemTime>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::Serialize;
    if let Some(t) = time {
        // Format SystemTime as a string. This is a basic example.
        // You might want a more standard format like RFC3339.
        let datetime: DateTime<Utc> = (*t).into();
        datetime.to_rfc3339().serialize(serializer)
    } else {
        serializer.serialize_none()
    }
}


impl TargetStatus {
    pub fn new(alias: String, monitor_url: String, monitor_hostname: String, monitor_port: u16) -> Self { // Ensure this is public if called from main
        TargetStatus {
            target_alias: alias, // Make fields public if accessed directly from api.rs
            last_check_time: None, // Make fields public
            last_result: None, // Make fields public
            consecutive_failures: 0, // Make fields public
            is_healthy: true, // Start with an optimistic state // Make fields public
            uptime_percentage_24h: 100.0, // Placeholder // Make fields public
            average_response_time_24h_ms: 0.0, // Placeholder // Make fields public
            monitor_url,
            monitor_hostname,
            monitor_port,
            cert_days_remaining: None,
            cert_is_valid: None,
        }
    }
}


// --- Check implementations (from previous steps, potentially with slight adjustments) ---
pub async fn check_tcp_port(address: &str, port: u16, request_timeout: Duration) -> Result<(), String> {
    let target = format!("{}:{}", address, port);

    match timeout(request_timeout, TcpStream::connect(&target)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(format!("Connection to {} failed: {}", target, e)),
        Err(_) => Err(format!(
            "Connection to {} timed out after {} seconds",
            target,
            request_timeout.as_secs()
        )),
    }
}

// Renamed from HttpCheckResult to avoid conflict with the enum variant
#[derive(Debug, Clone)] // Added Clone
pub struct HttpTargetCheckResult {
    pub status: CheckStatus, // Uses the new shared CheckStatus
    pub response_time_ms: u128,
    pub cert_days_remaining: Option<i64>,
    pub cert_is_valid: Option<bool>,
}

pub async fn check_http_target(
    address: &str,
    http_check_config: &HttpCheck,
) -> HttpTargetCheckResult { // Changed return type
    let start_time = Instant::now();
    let mut cert_days_remaining: Option<i64> = None;
    let mut cert_is_valid: Option<bool> = None;

    let protocol_str = match http_check_config.protocol {
        HttpProtocol::Http => "http",
        HttpProtocol::Https => "https",
    };
    let url = format!(
        "{}://{}:{}{}",
        protocol_str, address, http_check_config.port, http_check_config.path
    );

    if http_check_config.protocol == HttpProtocol::Https {
        debug!("Attempting to retrieve SSL certificate for {} on port {}", address, http_check_config.port);
        match TlsConnector::new() {
            Ok(connector) => {
                let stream_result = StdTcpStream::connect(format!("{}:{}", address, http_check_config.port))
                    .and_then(|stream| {
                        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                        connector.connect(address, stream)
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("TLS handshake error: {}", e)))
                    });

                match stream_result {
                    Ok(mut tls_stream) => {
                        if let Some(cert) = tls_stream.peer_certificate().ok().flatten() {
                            match X509::from_der(&cert.to_der().unwrap()) {
                                Ok(x509_cert) => {
                                    let not_after = x509_cert.not_after();
                                    let current_time = openssl::asn1::Asn1Time::days_from_now(0).unwrap();
                                    // Calculate days remaining
                                    // This is a bit complex due to Asn1Time not directly exposing easy diffs.
                                    // We'll compare timestamps.
                                    let days_diff = not_after.diff(&current_time);
                                    if let Ok(diff) = days_diff {
                                         cert_days_remaining = Some(diff.days as i64);
                                         cert_is_valid = Some(diff.days > 0);
                                    } else {
                                        warn!("Could not calculate certificate expiry difference for {}: {:?}", address, days_diff.err());
                                        cert_is_valid = Some(false); // Assume invalid if calculation fails
                                    }
                                    debug!("SSL cert for {}: Not After: {}, Days Remaining: {:?}, Valid: {:?}",
                                           address, not_after.to_string(), cert_days_remaining, cert_is_valid);
                                }
                                Err(e) => {
                                    warn!("Failed to parse X509 certificate for {}: {}", address, e);
                                    cert_is_valid = Some(false);
                                }
                            }
                        } else {
                            warn!("Could not get peer certificate for {}", address);
                            cert_is_valid = Some(false);
                        }
                    }
                    Err(e) => {
                        warn!("TLS connection to {}:{} failed for cert check: {}", address, http_check_config.port, e);
                        cert_is_valid = Some(false); // Cannot connect, so cert is not verifiable here
                    }
                }
            }
            Err(e) => {
                error!("Failed to create TLS connector: {}", e);
                // This is a setup error, not specific to the target's cert
            }
        }
    }


    let mut client_builder = reqwest::Client::builder();

    if http_check_config.protocol == HttpProtocol::Https && !http_check_config.check_ssl_certificate {
        warn!(
            "HTTP check for {} (URL: {}) is configured to accept invalid SSL certificates.",
            address, url
        );
        client_builder = client_builder.danger_accept_invalid_certs(true);
    }

    let client = match client_builder.build() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to build HTTP client for URL {}: {}", url, e);
            return HttpTargetCheckResult {
                status: CheckStatus::Unhealthy(format!("Failed to build HTTP client: {}", e)),
                response_time_ms: start_time.elapsed().as_millis(),
                cert_days_remaining,
                cert_is_valid,
            };
        }
    };

    let method = match http_check_config.method {
        ConfigHttpMethod::Get => reqwest::Method::GET,
        ConfigHttpMethod::Post => reqwest::Method::POST,
        ConfigHttpMethod::Head => reqwest::Method::HEAD,
        ConfigHttpMethod::Put => reqwest::Method::PUT,
        ConfigHttpMethod::Delete => reqwest::Method::DELETE,
        ConfigHttpMethod::Options => reqwest::Method::OPTIONS,
    };

    let request_timeout_duration = Duration::from_secs(http_check_config.timeout_seconds);
    let request_builder = client.request(method, &url).timeout(request_timeout_duration);

    match request_builder.send().await {
        Ok(response) => {
            let response_time_ms = start_time.elapsed().as_millis();
            let response_status_code = response.status().as_u16();

            if response_status_code != http_check_config.expected_status_code {
                return HttpTargetCheckResult { // Changed return type
                    status: CheckStatus::Unhealthy(format!(
                        "Unexpected status code: {} (expected {})",
                        response_status_code, http_check_config.expected_status_code
                    )),
                    response_time_ms,
                    cert_days_remaining,
                    cert_is_valid,
                };
            }

            if let Some(regex_str) = &http_check_config.body_regex_check {
                match Regex::new(regex_str) {
                    Ok(regex) => match response.text().await {
                        Ok(body) => {
                            if !regex.is_match(&body) {
                                return HttpTargetCheckResult { // Changed return type
                                    status: CheckStatus::Unhealthy(format!(
                                        "Response body does not match regex: {}",
                                        regex_str
                                    )),
                                    response_time_ms,
                                    cert_days_remaining,
                                    cert_is_valid,
                                };
                            }
                        }
                        Err(e) => {
                            return HttpTargetCheckResult { // Changed return type
                                status: CheckStatus::Unhealthy(format!(
                                    "Failed to read response body: {}",
                                    e
                                )),
                                response_time_ms,
                                cert_days_remaining,
                                cert_is_valid,
                            };
                        }
                    },
                    Err(e) => {
                        return HttpTargetCheckResult { // Changed return type
                            status: CheckStatus::Unhealthy(format!(
                                "Invalid regex pattern '{}': {}",
                                regex_str, e
                            )),
                            response_time_ms,
                            cert_days_remaining,
                            cert_is_valid,
                        };
                    }
                }
            }

            HttpTargetCheckResult { // Changed return type
                status: CheckStatus::Healthy,
                response_time_ms,
                cert_days_remaining,
                cert_is_valid,
            }
        }
        Err(e) => {
            let response_time_ms = start_time.elapsed().as_millis();
            HttpTargetCheckResult { // Changed return type
                status: CheckStatus::Unhealthy(format!("Request to {} failed: {}", url, e)),
                response_time_ms,
                cert_days_remaining,
                cert_is_valid,
            }
        }
    }
}


// --- Main Monitoring Loop ---

pub async fn run_monitoring_loop(
    app_config: Arc<AppConfig>,
    shared_statuses: Arc<Mutex<Vec<TargetStatus>>>,
) {
    info!(
        "Monitoring loop starting. Interval: {}s. {} hosts configured.",
        app_config.monitoring_interval_seconds,
        app_config.hosts.len()
    );

    let mut initial_statuses = Vec::new();
    for (_host_index, host_config) in app_config.hosts.iter().enumerate() { // host_index not used, renamed to _
        for (check_index, check) in host_config.checks.iter().enumerate() {
            let alias = host_config.alias.clone().unwrap_or_else(|| {
                format!(
                    "{}_{}_{}",
                    host_config.address,
                    match check {
                        Check::Tcp(_) => "tcp",
                        Check::Http(_) => "http",
                    },
                    check_index
                )
            });

            let monitor_hostname = host_config.address.clone();
            let (monitor_url, monitor_port) = match check {
                Check::Tcp(tcp_check) => (
                    format!("tcp://{}:{}", host_config.address, tcp_check.port),
                    tcp_check.port,
                ),
                Check::Http(http_check) => (
                    format!(
                        "{}://{}:{}{}",
                        match http_check.protocol {
                            HttpProtocol::Http => "http",
                            HttpProtocol::Https => "https",
                        },
                        host_config.address,
                        http_check.port,
                        http_check.path
                    ),
                    http_check.port,
                ),
            };

            initial_statuses.push(TargetStatus::new(
                alias.clone(),
                monitor_url,
                monitor_hostname,
                monitor_port,
            ));

            let status_index = initial_statuses.len() - 1;
            let app_config_clone = Arc::clone(&app_config);
            let shared_statuses_clone = Arc::clone(&shared_statuses);
            let host_address_clone = host_config.address.clone();
            let check_clone = check.clone(); // Check enum should derive Clone

            tokio::spawn(async move {
                loop {
                    let interval_seconds = app_config_clone.monitoring_interval_seconds;
                    let current_check_result: CheckResult;
                    let mut is_healthy_check = false;
                    let mut response_time_ms: Option<u128> = None;


                    let alias_clone = alias.clone(); // Clone alias for logging within this iteration
                    match &check_clone {
                        Check::Tcp(tcp_check_config) => {
                            let check_type_str = "TCP";
                            info!(
                                "Performing {} check for target: {} ({}:{})",
                                check_type_str, alias_clone, host_address_clone, tcp_check_config.port
                            );
                            let timeout_duration = Duration::from_secs(tcp_check_config.timeout_seconds);
                            let result = check_tcp_port(
                                &host_address_clone,
                                tcp_check_config.port,
                                timeout_duration,
                            )
                            .await;

                            match &result {
                                Ok(_) => {
                                    is_healthy_check = true;
                                    info!(
                                        "Target {} ({}) is healthy (TCP:{})",
                                        alias_clone, host_address_clone, tcp_check_config.port
                                    );
                                }
                                Err(e) => {
                                    is_healthy_check = false;
                                    // This specific error is already part of the 'result' and will be stored.
                                    // The generic unhealthy log will cover this.
                                }
                            }
                            current_check_result = CheckResult::Tcp(TcpCheckResult { result });
                        }
                        Check::Http(http_check_config) => {
                            let check_type_str = "HTTP";
                            let protocol_str = match http_check_config.protocol { HttpProtocol::Http => "http", HttpProtocol::Https => "https" };
                            info!(
                                "Performing {} check for target: {} ({}://{}:{}{})",
                                check_type_str, alias_clone, protocol_str, host_address_clone, http_check_config.port, http_check_config.path
                            );
                            let http_result =
                                check_http_target(&host_address_clone, http_check_config).await;

                            response_time_ms = Some(http_result.response_time_ms);
                            match &http_result.status { // Borrow http_result.status
                                CheckStatus::Healthy => {
                                    is_healthy_check = true;
                                    info!(
                                        "Target {} ({}) is healthy. Response time: {}ms (HTTP)",
                                        alias_clone, host_address_clone, http_result.response_time_ms
                                    );
                                }
                                CheckStatus::Unhealthy(reason) => {
                                    is_healthy_check = false;
                                    // Reason will be logged by the generic unhealthy log.
                                }
                            }
                            current_check_result = CheckResult::Http(HttpCheckResultDetails {
                                status: http_result.status, // http_result.status is cloned here
                                response_time_ms: http_result.response_time_ms,
                                cert_days_remaining: http_result.cert_days_remaining,
                                cert_is_valid: http_result.cert_is_valid,
                            });
                        }
                    }

                    // Update shared state
                    {
                        let mut statuses = shared_statuses_clone.lock().await;
                        if let Some(status_entry) = statuses.get_mut(status_index) {
                            status_entry.last_check_time = Some(SystemTime::now());
                            status_entry.last_result = Some(current_check_result.clone()); // Clone for storage
                            status_entry.is_healthy = is_healthy_check;

                            // Update cert details from the check result
                            match &current_check_result {
                                CheckResult::Http(http_details) => {
                                    status_entry.cert_days_remaining = http_details.cert_days_remaining;
                                    status_entry.cert_is_valid = http_details.cert_is_valid;
                                }
                                CheckResult::Tcp(_) => {
                                    // Ensure cert fields are None for TCP checks
                                    status_entry.cert_days_remaining = None;
                                    status_entry.cert_is_valid = None;
                                }
                            }

                            if is_healthy_check {
                                if status_entry.consecutive_failures > 0 { // Log recovery
                                    info!(
                                        "Target {} has recovered. Was unhealthy for {} checks.",
                                        alias_clone, status_entry.consecutive_failures
                                    );
                                }
                                status_entry.consecutive_failures = 0;
                            } else {
                                status_entry.consecutive_failures += 1;
                                let reason_str = match &current_check_result {
                                    CheckResult::Tcp(tcp_res) => match &tcp_res.result {
                                        Ok(_) => "Unknown TCP error".to_string(), // Should not happen if unhealthy
                                        Err(s) => s.clone(),
                                    },
                                    CheckResult::Http(http_res_details) => match &http_res_details.status {
                                        CheckStatus::Healthy => "Unknown HTTP error".to_string(), // Should not happen
                                        CheckStatus::Unhealthy(s) => s.clone(),
                                    },
                                };
                                warn!(
                                    "Target {} is UNHEALTHY. Reason: {}. Consecutive failures: {}. Check type: {}",
                                    alias_clone,
                                    reason_str,
                                    status_entry.consecutive_failures,
                                    match check_clone { Check::Http(_) => "HTTP", Check::Tcp(_) => "TCP"}
                                );
                            }
                            debug!("[{}] Updated status. Healthy: {}, Consecutive Failures: {}, Cert Valid: {:?}, Cert Days: {:?}",
                                   alias_clone, status_entry.is_healthy, status_entry.consecutive_failures,
                                   status_entry.cert_is_valid, status_entry.cert_days_remaining);
                        }
                    }

                    tokio::time::sleep(Duration::from_secs(interval_seconds)).await;
                }
            });
        }
    }
    // Initialize shared_statuses with the prepared TargetStatus entries
    let mut statuses_guard = shared_statuses.lock().await;
    *statuses_guard = initial_statuses;
    info!("Monitoring setup complete. {} individual checks spawned.", statuses_guard.len());
}


#[cfg(test)]
mod tests {
    use super::*; // Import items from the parent module (monitoring)
    use crate::config::{HttpCheck, HttpProtocol, HttpMethod};
    use reqwest::StatusCode; // We'll need this for constructing mock responses if possible

    // Helper function to create a basic HttpCheck config for tests
    fn basic_http_check_config() -> HttpCheck {
        HttpCheck {
            port: 80,
            path: "/".to_string(),
            protocol: HttpProtocol::Http,
            method: HttpMethod::Get,
            timeout_seconds: 5,
            check_ssl_certificate: true,
            expected_status_code: 200,
            body_regex_check: None,
        }
    }

    // This is a simplified scenario. Directly testing check_http_target is hard
    // due to reqwest::Client and async nature without a full mocking framework.
    // Instead, we can test helper functions or parts of its logic if we refactor.
    // For now, let's imagine a helper function that evaluates the response.

    // Let's define a new function that encapsulates the logic we want to test.
    // This function would normally be part of your src/monitoring.rs, made public or pub(crate).
    // For this exercise, I'll define it here within the test module to show intent.
    // In a real scenario, you would refactor check_http_target.

    fn evaluate_http_response(
        response_status_code: u16,
        response_body: Option<String>, // Option to simulate cases where body reading fails or isn't needed
        check_config: &HttpCheck,
    ) -> CheckStatus {
        if response_status_code != check_config.expected_status_code {
            return CheckStatus::Unhealthy(format!(
                "Unexpected status code: {} (expected {})",
                response_status_code, check_config.expected_status_code
            ));
        }

        if let Some(regex_str) = &check_config.body_regex_check {
            match Regex::new(regex_str) {
                Ok(regex) => {
                    if let Some(body) = response_body {
                        if !regex.is_match(&body) {
                            return CheckStatus::Unhealthy(format!(
                                "Response body does not match regex: {}",
                                regex_str
                            ));
                        }
                    } else {
                        // If regex is expected, but body is None (e.g. failed to read)
                        return CheckStatus::Unhealthy(
                            "Response body not available for regex check".to_string()
                        );
                    }
                }
                Err(e) => {
                    return CheckStatus::Unhealthy(format!(
                        "Invalid regex pattern '{}': {}",
                        regex_str, e
                    ));
                }
            }
        }
        CheckStatus::Healthy
    }

    #[test]
    fn test_evaluate_http_response_healthy_no_regex() {
        let config = basic_http_check_config();
        let status = evaluate_http_response(200, Some("body".to_string()), &config);
        assert!(matches!(status, CheckStatus::Healthy));
    }

    #[test]
    fn test_evaluate_http_response_unhealthy_status_code() {
        let config = basic_http_check_config(); // expects 200
        let status = evaluate_http_response(500, Some("body".to_string()), &config);
        match status {
            CheckStatus::Unhealthy(reason) => {
                assert!(reason.contains("Unexpected status code: 500"));
            }
            _ => panic!("Expected Unhealthy status"),
        }
    }

    #[test]
    fn test_evaluate_http_response_healthy_with_matching_regex() {
        let mut config = basic_http_check_config();
        config.body_regex_check = Some("success".to_string());
        let status = evaluate_http_response(200, Some("Request was a success!".to_string()), &config);
        assert!(matches!(status, CheckStatus::Healthy));
    }

    #[test]
    fn test_evaluate_http_response_unhealthy_non_matching_regex() {
        let mut config = basic_http_check_config();
        config.body_regex_check = Some("success".to_string());
        let status = evaluate_http_response(200, Some("Request failed.".to_string()), &config);
        match status {
            CheckStatus::Unhealthy(reason) => {
                assert!(reason.contains("Response body does not match regex"));
            }
            _ => panic!("Expected Unhealthy status"),
        }
    }

    #[test]
    fn test_evaluate_http_response_unhealthy_regex_compilation_error() {
        let mut config = basic_http_check_config();
        config.body_regex_check = Some("[[invalid_regex".to_string()); // Invalid regex
        let status = evaluate_http_response(200, Some("body".to_string()), &config);
        match status {
            CheckStatus::Unhealthy(reason) => {
                assert!(reason.contains("Invalid regex pattern"));
            }
            _ => panic!("Expected Unhealthy status"),
        }
    }

    #[test]
    fn test_evaluate_http_response_unhealthy_body_not_available_for_regex() {
        let mut config = basic_http_check_config();
        config.body_regex_check = Some("success".to_string());
        let status = evaluate_http_response(200, None, &config); // Body is None
        match status {
            CheckStatus::Unhealthy(reason) => {
                assert!(reason.contains("Response body not available for regex check"));
            }
            _ => panic!("Expected Unhealthy status"),
        }
    }

    // Note: Testing the full check_tcp_port and check_http_target functions
    // as unit tests is complex because they perform real network operations.
    // These would typically be covered by integration tests with a mock server
    // or specific test environment. The tests above focus on the synchronous,
    // logical parts that can be extracted.
}
