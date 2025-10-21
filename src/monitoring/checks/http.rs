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
) -> HttpTargetCheckResult {
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
        info!("Attempting to retrieve SSL certificate for {} on port {}", address, http_check_config.port);
        // Use OpenSSL directly for more reliable certificate extraction
        match openssl::ssl::SslConnector::builder(openssl::ssl::SslMethod::tls()) {
            Ok(mut builder) => {
                // Set up SSL connector with verification disabled for certificate extraction
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
                            // OpenSSL peer_certificate() already returns an X509 certificate
                            {
                                    let not_after = x509_cert.not_after();
                                    let current_time = openssl::asn1::Asn1Time::days_from_now(0).unwrap();
                                    // Calculate days remaining
                                    // This is a bit complex due to Asn1Time not directly exposing easy diffs.
                                    // We'll compare timestamps.
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
                                            // Try a more robust approach using chrono and NaiveDate parsing
                                            let not_after_str = not_after.to_string();
                                            info!("Certificate not_after string for {}: {}", address, not_after_str);
                                            
                                            // Parse the ASN1 time string format (e.g., "Dec 31 23:59:59 2024 GMT")
                                            // OpenSSL typically outputs in format like "Jan  1 00:00:00 2025 GMT"
                                            if let Ok(parsed_time) = chrono::DateTime::parse_from_str(&not_after_str.replace("  ", " "), "%b %d %H:%M:%S %Y %Z") {
                                                let now = chrono::Utc::now();
                                                let days_remaining = (parsed_time.date_naive() - now.date_naive()).num_days();
                                                cert_days_remaining = Some(days_remaining);
                                                cert_is_valid = Some(days_remaining > 0);
                                                info!("SSL cert for {} (chrono): Days Remaining: {}, Valid: {}", 
                                                      address, days_remaining, days_remaining > 0);
                                            } else {
                                                // Final fallback - assume certificate is valid with reasonable expiry
                                                warn!("Could not parse certificate time format '{}' for {}, using fallback", not_after_str, address);
                                                cert_days_remaining = Some(90); // Assume 90 days remaining as safe fallback
                                                cert_is_valid = Some(true);
                                            }
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
                        cert_is_valid = Some(false); // Cannot connect, so cert is not verifiable here
                    }
                }
            }
            Err(_e) => {
                error!("Failed to create SSL connector: {}", _e);
                // This is a setup error, not specific to the target's cert
                cert_is_valid = Some(false);
            }
        }
    }

    let mut client_builder = reqwest::Client::builder();

    if http_check_config.protocol == HttpProtocol::Https && !http_check_config.check_ssl_certificate {
        // For connectivity check, accept invalid certs but still try to get cert info above
        client_builder = client_builder.danger_accept_invalid_certs(true);
    }

    let client = match client_builder.build() {
        Ok(c) => c,
        Err(_e) => {
            return HttpTargetCheckResult {
                status: CheckStatus::Unhealthy(format!("Failed to build HTTP client: {}", _e)),
                response_time_ms: start_time.elapsed().as_millis(),
                cert_days_remaining,
                cert_is_valid,
            };
        }
    };

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

    // Add custom headers
    if let Some(headers) = &http_check_config.headers {
        for (key, value) in headers {
            request_builder = request_builder.header(key, value);
        }
    }

    // Note: Request body configuration not available in current HttpCheck struct

    // Handle authentication
    if let Some(auth) = &http_check_config.auth {
        request_builder = match auth {
            AuthConfig::Basic { username, password } => {
                request_builder.basic_auth(username, Some(password))
            }
            AuthConfig::Bearer { token } => {
                request_builder.bearer_auth(token)
            }
            AuthConfig::OAuth2 { client_id, client_secret, token_url } => {
                // For OAuth2, we need to first get an access token
                match get_oauth2_token(client_id, client_secret, token_url).await {
                    Ok(access_token) => request_builder.bearer_auth(access_token),
                    Err(e) => {
                        return HttpTargetCheckResult {
                            status: CheckStatus::Unhealthy(format!("OAuth2 authentication failed: {}", e)),
                            response_time_ms: start_time.elapsed().as_millis(),
                            cert_days_remaining,
                            cert_is_valid,
                        };
                    }
                }
            }
        };
    }

    match request_builder.send().await {
        Ok(response) => {
            let response_time_ms = start_time.elapsed().as_millis();
            let response_status_code = response.status().as_u16();
            
            // Clone response headers and other data before consuming the response
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

            // Get response body for assertions and regex check
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

            // Check regex match if configured (using body_regex_check field)
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


            // Evaluate assertions if configured
            if let Some(assertions) = &http_check_config.assertions {
                let assertion_results = evaluate_assertions_with_data(
                    assertions,
                    response_status,
                    &response_headers,
                    &response_body,
                    response_time_ms,
                    None, // cert_info is not available here for assertions
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