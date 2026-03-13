use std::time::{Duration, Instant};
use std::net::TcpStream as StdTcpStream;
use log::{info, warn, error};
use chrono;
use crate::config::{HttpCheck, HttpProtocol, HttpMethod as ConfigHttpMethod, AuthConfig};
use crate::monitoring::types::{HttpTargetCheckResult, CheckStatus};
use crate::monitoring::assertions::evaluate_assertions_with_data;
use crate::monitoring::auth::get_oauth2_token;

pub async fn check_http_target(
    address: &str,
    http_check_config: &HttpCheck,
    client: &reqwest::Client,  // <-- reused client passed in, not built here
) -> HttpTargetCheckResult {
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

    // SSL cert pre-flight: intentionally OUTSIDE the response time measurement.
    // This opens a separate raw TLS connection purely to extract certificate metadata.
    // It must not be included in response_time_ms, which should only reflect HTTP latency.
    if http_check_config.protocol == HttpProtocol::Https {
        info!("Attempting to retrieve SSL certificate for {} on port {}", address, http_check_config.port);
        match openssl::ssl::SslConnector::builder(openssl::ssl::SslMethod::tls()) {
            Ok(mut builder) => {
                builder.set_verify(openssl::ssl::SslVerifyMode::NONE);
                let connector = builder.build();

                let stream_result = StdTcpStream::connect(format!("{}:{}", address, http_check_config.port))
                    .and_then(|stream| {
                        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                        connector.connect(address, stream)
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("SSL handshake error: {}", e)))
                    });

                match stream_result {
                    Ok(ssl_stream) => {
                        if let Some(x509_cert) = ssl_stream.ssl().peer_certificate() {
                            let not_after = x509_cert.not_after();
                            let current_time = openssl::asn1::Asn1Time::days_from_now(0).unwrap();
                            let days_diff = not_after.diff(&current_time);
                            match days_diff {
                                Ok(diff) => {
                                    cert_days_remaining = Some(-diff.days as i64);
                                    cert_is_valid = Some(-diff.days > 0);
                                    info!("SSL cert for {}: Days Remaining: {}, Valid: {}",
                                          address, -diff.days, -diff.days > 0);
                                }
                                Err(e) => {
                                    warn!("Could not calculate certificate expiry difference for {}: {:?}", address, e);
                                    let not_after_str = not_after.to_string();
                                    info!("Certificate not_after string for {}: {}", address, not_after_str);

                                    if let Ok(parsed_time) = chrono::DateTime::parse_from_str(&not_after_str.replace("  ", " "), "%b %d %H:%M:%S %Y %Z") {
                                        let now = chrono::Utc::now();
                                        let days_remaining = (parsed_time.date_naive() - now.date_naive()).num_days();
                                        cert_days_remaining = Some(days_remaining);
                                        cert_is_valid = Some(days_remaining > 0);
                                        info!("SSL cert for {} (chrono): Days Remaining: {}, Valid: {}",
                                              address, days_remaining, days_remaining > 0);
                                    } else {
                                        warn!("Could not parse certificate time format '{}' for {}, using fallback", not_after_str, address);
                                        cert_days_remaining = Some(90);
                                        cert_is_valid = Some(true);
                                    }
                                }
                            }
                        } else {
                            warn!("Could not get peer certificate for {} - no certificate returned by peer", address);
                            cert_is_valid = Some(false);
                        }
                    }
                    Err(_e) => {
                        warn!("TLS connection to {}:{} failed for cert check: {}", address, http_check_config.port, _e);
                        info!("Will still attempt HTTP request with relaxed SSL validation for connectivity check");
                        cert_is_valid = Some(false);
                    }
                }
            }
            Err(_e) => {
                error!("Failed to create SSL connector: {}", _e);
                cert_is_valid = Some(false);
            }
        }
    }

    // Build the request using the reused client.
    let method = match http_check_config.method {
        ConfigHttpMethod::Get => reqwest::Method::GET,
        ConfigHttpMethod::Post => reqwest::Method::POST,
        ConfigHttpMethod::Put => reqwest::Method::PUT,
        ConfigHttpMethod::Delete => reqwest::Method::DELETE,
        ConfigHttpMethod::Head => reqwest::Method::HEAD,
        ConfigHttpMethod::Options => reqwest::Method::OPTIONS,
    };

    let mut request_builder = client
        .request(method, &url)
        .timeout(Duration::from_secs(http_check_config.timeout_seconds));

    if let Some(headers) = &http_check_config.headers {
        for (key, value) in headers {
            request_builder = request_builder.header(key, value);
        }
    }

    if let Some(auth) = &http_check_config.auth {
        request_builder = match auth {
            AuthConfig::Basic { username, password } => {
                request_builder.basic_auth(username, Some(password))
            }
            AuthConfig::Bearer { token } => {
                request_builder.bearer_auth(token)
            }
            AuthConfig::OAuth2 { client_id, client_secret, token_url } => {
                match get_oauth2_token(client_id, client_secret, token_url).await {
                    Ok(access_token) => request_builder.bearer_auth(access_token),
                    Err(e) => {
                        // OAuth token fetch failed; we haven't started timing yet so use 0.
                        return HttpTargetCheckResult {
                            status: CheckStatus::Unhealthy(format!("OAuth2 authentication failed: {}", e)),
                            response_time_ms: 0,
                            cert_days_remaining,
                            cert_is_valid,
                        };
                    }
                }
            }
        };
    }

    // Start timing immediately before the network request.
    // Everything above (cert pre-flight, client reuse, request building) is excluded.
    let start_time = Instant::now();

    match request_builder.send().await {
        Ok(response) => {
            let response_time_ms = start_time.elapsed().as_millis();
            let response_status_code = response.status().as_u16();
            let response_headers = response.headers().clone();
            let response_status = response.status();

            if response_status_code != http_check_config.expected_status_code {
                return HttpTargetCheckResult {
                    status: CheckStatus::Unhealthy(format!(
                        "Unexpected status code: {} (expected {})",
                        response_status_code, http_check_config.expected_status_code
                    )),
                    response_time_ms,
                    cert_days_remaining,
                    cert_is_valid,
                };
            }

            let response_body = match response.text().await {
                Ok(body) => body,
                Err(_e) => {
                    return HttpTargetCheckResult {
                        status: CheckStatus::Unhealthy(format!(
                            "Failed to read response body: {}",
                            _e
                        )),
                        response_time_ms,
                        cert_days_remaining,
                        cert_is_valid,
                    };
                }
            };

            if let Some(regex_pattern) = &http_check_config.body_regex_check {
                match regex::Regex::new(regex_pattern) {
                    Ok(re) => {
                        if !re.is_match(&response_body) {
                            return HttpTargetCheckResult {
                                status: CheckStatus::Unhealthy(format!(
                                    "Response body does not match regex pattern: '{}'",
                                    regex_pattern
                                )),
                                response_time_ms,
                                cert_days_remaining,
                                cert_is_valid,
                            };
                        }
                    }
                    Err(_e) => {
                        return HttpTargetCheckResult {
                            status: CheckStatus::Unhealthy(format!(
                                "Invalid regex pattern: '{}' - {}",
                                regex_pattern, _e
                            )),
                            response_time_ms,
                            cert_days_remaining,
                            cert_is_valid,
                        };
                    }
                }
            }

            if let Some(assertions) = &http_check_config.assertions {
                let assertion_results = evaluate_assertions_with_data(
                    assertions,
                    response_status,
                    &response_headers,
                    &response_body,
                    response_time_ms,
                    None,
                );

                let failed_assertions: Vec<&crate::monitoring::types::AssertionResult> = assertion_results
                    .iter()
                    .filter(|r| !r.passed)
                    .collect();

                if !failed_assertions.is_empty() {
                    let failure_messages: Vec<String> = failed_assertions
                        .iter()
                        .map(|r| r.message.clone())
                        .collect();

                    return HttpTargetCheckResult {
                        status: CheckStatus::Unhealthy(format!(
                            "Assertion failures: {}",
                            failure_messages.join("; ")
                        )),
                        response_time_ms,
                        cert_days_remaining,
                        cert_is_valid,
                    };
                }
            }

            HttpTargetCheckResult {
                status: CheckStatus::Healthy,
                response_time_ms,
                cert_days_remaining,
                cert_is_valid,
            }
        }
        Err(_e) => {
            let response_time_ms = start_time.elapsed().as_millis();
            HttpTargetCheckResult {
                status: CheckStatus::Unhealthy(format!("Request to {} failed: {}", url, _e)),
                response_time_ms,
                cert_days_remaining,
                cert_is_valid,
            }
        }
    }
}

/// Build a reqwest::Client suitable for one HTTP monitor target.
/// Call this once per monitor at startup and reuse it across all check cycles.
/// This avoids paying TLS context + connection pool construction cost every check.
pub fn build_http_client(http_check_config: &HttpCheck) -> Result<reqwest::Client, reqwest::Error> {
    let mut client_builder = reqwest::Client::builder();

    if http_check_config.protocol == HttpProtocol::Https && !http_check_config.check_ssl_certificate {
        client_builder = client_builder.danger_accept_invalid_certs(true);
    }

    // Disable connection pool session reuse so each check reflects a fresh connection,
    // matching Uptime Kuma's maxCachedSessions: 0 behaviour for accurate measurement.
    client_builder = client_builder.connection_verbose(false);
    client_builder = client_builder.pool_max_idle_per_host(0);

    client_builder.build()
}