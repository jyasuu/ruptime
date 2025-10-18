use uptime_monitor::config::*;
use uptime_monitor::test_utils::*;
use uptime_monitor::monitoring::evaluate_predicate;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Test helper to create HTTPS httpbin check
fn create_httpbin_https_check(path: &str, method: HttpMethod, expected_status: u16) -> HttpCheck {
    HttpCheck {
        port: 443,
        path: path.to_string(),
        protocol: HttpProtocol::Https,
        method,
        timeout_seconds: 10,
        check_ssl_certificate: true,
        expected_status_code: expected_status,
        body_regex_check: None,
        auth: None,
        headers: None,
        assertions: Some(vec![
            HttpAssertion {
                query: AssertionQuery::Status,
                predicate: AssertionPredicate::Equals,
                value: AssertionValue::Integer(expected_status as i64),
            }
        ]),
    }
}

/// Test helper to create HTTP httpbin check  
fn create_httpbin_http_check(path: &str, method: HttpMethod, expected_status: u16) -> HttpCheck {
    HttpCheck {
        port: 80,
        path: path.to_string(),
        protocol: HttpProtocol::Http,
        method,
        timeout_seconds: 10,
        check_ssl_certificate: false,
        expected_status_code: expected_status,
        body_regex_check: None,
        auth: None,
        headers: None,
        assertions: Some(vec![
            HttpAssertion {
                query: AssertionQuery::Status,
                predicate: AssertionPredicate::Equals,
                value: AssertionValue::Integer(expected_status as i64),
            }
        ]),
    }
}

/// Test helper to create JSON path assertion
fn create_json_assertion(json_path: &str, predicate: AssertionPredicate, expected_value: AssertionValue) -> HttpAssertion {
    HttpAssertion {
        query: AssertionQuery::JsonPath { path: json_path.to_string() },
        predicate,
        value: expected_value,
    }
}

/// Test helper to create header assertion
fn create_header_assertion(header_name: &str, predicate: AssertionPredicate, expected_value: AssertionValue) -> HttpAssertion {
    HttpAssertion {
        query: AssertionQuery::Header { name: header_name.to_string() },
        predicate,
        value: expected_value,
    }
}

// =============================================================================
// HTTP Methods Tests (Testing different HTTP verbs)
// =============================================================================

#[test]
fn test_httpbin_get_method_check() {
    let check = create_httpbin_https_check("/get", HttpMethod::Get, 200);
    
    // Test that check is properly configured
    assert_eq!(check.method, HttpMethod::Get);
    assert_eq!(check.path, "/get");
    assert_eq!(check.port, 443);
    assert_eq!(check.protocol, HttpProtocol::Https);
    assert_eq!(check.timeout_seconds, 10);
    
    // Test status assertion
    let status_assertion = &check.assertions.as_ref().unwrap()[0];
    let status_value = Value::Number(serde_json::Number::from(200));
    assert!(evaluate_predicate(&status_assertion.predicate, &status_value, &status_assertion.value));
    
    // Test with wrong status
    let status_value = Value::Number(serde_json::Number::from(404));
    assert!(!evaluate_predicate(&status_assertion.predicate, &status_value, &status_assertion.value));
}

#[test]
fn test_httpbin_post_method_check() {
    let mut check = create_httpbin_https_check("/post", HttpMethod::Post, 200);
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    check.headers = Some(headers);
    
    assert_eq!(check.method, HttpMethod::Post);
    assert_eq!(check.path, "/post");
    assert_eq!(check.port, 443);
    assert!(check.headers.is_some());
}

#[test]
fn test_httpbin_put_method_check() {
    let check = create_httpbin_https_check("/put", HttpMethod::Put, 200);
    assert_eq!(check.method, HttpMethod::Put);
    assert_eq!(check.path, "/put");
}

#[test]
fn test_httpbin_delete_method_check() {
    let check = create_httpbin_https_check("/delete", HttpMethod::Delete, 200);
    assert_eq!(check.method, HttpMethod::Delete);
    assert_eq!(check.path, "/delete");
}

#[test]
fn test_httpbin_head_method_check() {
    let check = create_httpbin_https_check("/", HttpMethod::Head, 200);
    assert_eq!(check.method, HttpMethod::Head);
    assert_eq!(check.path, "/");
}

#[test]
fn test_httpbin_options_method_check() {
    let check = create_httpbin_https_check("/", HttpMethod::Options, 200);
    assert_eq!(check.method, HttpMethod::Options);
    assert_eq!(check.path, "/");
}

// =============================================================================
// Status Code Tests
// =============================================================================

#[test]
fn test_httpbin_status_codes() {
    let test_cases = vec![
        (200, "OK"),
        (201, "Created"),
        (400, "Bad Request"),
        (401, "Unauthorized"),
        (403, "Forbidden"),
        (404, "Not Found"),
        (500, "Internal Server Error"),
        (502, "Bad Gateway"),
        (503, "Service Unavailable"),
    ];
    
    for (status_code, description) in test_cases {
        let check = create_httpbin_https_check(&format!("/status/{}", status_code), HttpMethod::Get, status_code);
        
        // Test assertion evaluation
        let assertion = &check.assertions.as_ref().unwrap()[0];
        let status_value = Value::Number(serde_json::Number::from(status_code));
        assert!(
            evaluate_predicate(&assertion.predicate, &status_value, &assertion.value),
            "Failed for status {} ({})", status_code, description
        );
    }
}

#[test]
fn test_httpbin_status_assertion_logic() {
    // Test status assertion with various predicates
    let status_assertion_eq = create_test_assertion(
        AssertionQuery::Status,
        AssertionPredicate::Equals,
        AssertionValue::Integer(200),
    );
    
    let status_assertion_gte = create_test_assertion(
        AssertionQuery::Status,
        AssertionPredicate::GreaterThanOrEqual,
        AssertionValue::Integer(200),
    );
    
    let status_assertion_lt = create_test_assertion(
        AssertionQuery::Status,
        AssertionPredicate::LessThan,
        AssertionValue::Integer(400),
    );
    
    // Test with status 200
    let status_200 = Value::Number(serde_json::Number::from(200));
    assert!(evaluate_predicate(&status_assertion_eq.predicate, &status_200, &status_assertion_eq.value));
    assert!(evaluate_predicate(&status_assertion_gte.predicate, &status_200, &status_assertion_gte.value));
    assert!(evaluate_predicate(&status_assertion_lt.predicate, &status_200, &status_assertion_lt.value));
    
    // Test with status 404
    let status_404 = Value::Number(serde_json::Number::from(404));
    assert!(!evaluate_predicate(&status_assertion_eq.predicate, &status_404, &status_assertion_eq.value));
    assert!(evaluate_predicate(&status_assertion_gte.predicate, &status_404, &status_assertion_gte.value));
    assert!(!evaluate_predicate(&status_assertion_lt.predicate, &status_404, &status_assertion_lt.value));
}

// =============================================================================
// Authentication Tests
// =============================================================================

#[test]
fn test_httpbin_basic_auth_check() {
    let mut check = create_httpbin_https_check("/basic-auth/testuser/testpass", HttpMethod::Get, 200);
    check.auth = Some(AuthConfig::Basic {
        username: "testuser".to_string(),
        password: "testpass".to_string(),
    });
    
    assert_eq!(check.path, "/basic-auth/testuser/testpass");
    assert!(check.auth.is_some());
    
    if let Some(AuthConfig::Basic { username, password }) = &check.auth {
        assert_eq!(username, "testuser");
        assert_eq!(password, "testpass");
    } else {
        panic!("Expected Basic auth configuration");
    }
}

#[test]
fn test_httpbin_bearer_auth_check() {
    let mut check = create_httpbin_https_check("/bearer", HttpMethod::Get, 200);
    check.auth = Some(AuthConfig::Bearer {
        token: "test-bearer-token-123".to_string(),
    });
    
    assert_eq!(check.path, "/bearer");
    assert!(check.auth.is_some());
    
    if let Some(AuthConfig::Bearer { token }) = &check.auth {
        assert_eq!(token, "test-bearer-token-123");
    } else {
        panic!("Expected Bearer auth configuration");
    }
}

#[test]
fn test_httpbin_oauth2_auth_check() {
    let mut check = create_httpbin_https_check("/oauth", HttpMethod::Get, 200);
    check.auth = Some(AuthConfig::OAuth2 {
        client_id: "test-client-id".to_string(),
        client_secret: "test-client-secret".to_string(),
        token_url: "https://httpbin.org/oauth/token".to_string(),
    });
    
    assert_eq!(check.path, "/oauth");
    assert!(check.auth.is_some());
    
    if let Some(AuthConfig::OAuth2 { client_id, client_secret, token_url }) = &check.auth {
        assert_eq!(client_id, "test-client-id");
        assert_eq!(client_secret, "test-client-secret");
        assert_eq!(token_url, "https://httpbin.org/oauth/token");
    } else {
        panic!("Expected OAuth2 auth configuration");
    }
}

// =============================================================================
// Headers and Request Inspection Tests
// =============================================================================

#[test]
fn test_httpbin_headers_configuration() {
    let mut check = create_httpbin_https_check("/headers", HttpMethod::Get, 200);
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), "UptimeMonitor/1.0".to_string());
    headers.insert("Accept".to_string(), "application/json".to_string());
    headers.insert("Authorization".to_string(), "Bearer test-token".to_string());
    check.headers = Some(headers);
    
    assert_eq!(check.path, "/headers");
    assert!(check.headers.is_some());
    assert_eq!(check.headers.as_ref().unwrap().len(), 3);
    
    let headers_ref = check.headers.as_ref().unwrap();
    assert_eq!(headers_ref.get("User-Agent"), Some(&"UptimeMonitor/1.0".to_string()));
    assert_eq!(headers_ref.get("Accept"), Some(&"application/json".to_string()));
    assert_eq!(headers_ref.get("Authorization"), Some(&"Bearer test-token".to_string()));
}

#[test]
fn test_httpbin_user_agent_check() {
    let mut check = create_httpbin_https_check("/user-agent", HttpMethod::Get, 200);
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), "UptimeMonitor/1.0".to_string());
    check.headers = Some(headers);
    
    assert_eq!(check.path, "/user-agent");
    assert!(check.headers.is_some());
    assert_eq!(check.headers.as_ref().unwrap().get("User-Agent"), Some(&"UptimeMonitor/1.0".to_string()));
}

// =============================================================================
// Response Format and Content Tests
// =============================================================================

#[test]
fn test_httpbin_json_response_check() {
    let check = create_httpbin_https_check("/json", HttpMethod::Get, 200);
    assert_eq!(check.path, "/json");
    assert_eq!(check.expected_status_code, 200);
}

#[test]
fn test_httpbin_html_response_check() {
    let check = create_httpbin_https_check("/html", HttpMethod::Get, 200);
    assert_eq!(check.path, "/html");
    assert_eq!(check.expected_status_code, 200);
}

#[test]
fn test_httpbin_xml_response_check() {
    let check = create_httpbin_https_check("/xml", HttpMethod::Get, 200);
    assert_eq!(check.path, "/xml");
    assert_eq!(check.expected_status_code, 200);
}

// =============================================================================
// Assertion Tests
// =============================================================================

#[test]
fn test_httpbin_json_path_assertions() {
    // Test JSON path assertion creation and evaluation
    let json_assertion = create_json_assertion(
        "$.url",
        AssertionPredicate::Contains,
        AssertionValue::String("httpbin.org".to_string()),
    );
    
    // Mock JSON response from httpbin
    let _mock_json = json!({
        "url": "https://httpbin.org/get?param=value",
        "args": {
            "param": "value"
        },
        "headers": {
            "Host": "httpbin.org"
        }
    });
    
    // Test URL assertion
    let url_value = Value::String("https://httpbin.org/get?param=value".to_string());
    assert!(evaluate_predicate(&json_assertion.predicate, &url_value, &json_assertion.value));
    
    let wrong_url = Value::String("https://example.com/get".to_string());
    assert!(!evaluate_predicate(&json_assertion.predicate, &wrong_url, &json_assertion.value));
}

#[test]
fn test_httpbin_header_assertions() {
    let header_assertion = create_header_assertion(
        "Content-Type",
        AssertionPredicate::Contains,
        AssertionValue::String("application/json".to_string()),
    );
    
    let json_content_type = Value::String("application/json; charset=utf-8".to_string());
    assert!(evaluate_predicate(&header_assertion.predicate, &json_content_type, &header_assertion.value));
    
    let html_content_type = Value::String("text/html".to_string());
    assert!(!evaluate_predicate(&header_assertion.predicate, &html_content_type, &header_assertion.value));
}

#[test]
fn test_httpbin_body_regex_assertions() {
    let mut check = create_httpbin_https_check("/html", HttpMethod::Get, 200);
    check.body_regex_check = Some(r"<title>.*</title>".to_string());
    
    assert_eq!(check.path, "/html");
    assert!(check.body_regex_check.is_some());
    assert_eq!(check.body_regex_check.as_ref().unwrap(), r"<title>.*</title>");
}

// =============================================================================
// UUID and Data Validation Tests
// =============================================================================

#[test]
fn test_httpbin_uuid_validation() {
    let uuid_assertion = HttpAssertion {
        query: AssertionQuery::JsonPath { path: "$.uuid".to_string() },
        predicate: AssertionPredicate::IsUuid,
        value: AssertionValue::String("".to_string()), // Value not used for type checks
    };
    
    // Test valid UUID
    let valid_uuid = Value::String("550e8400-e29b-41d4-a716-446655440000".to_string());
    assert!(evaluate_predicate(&uuid_assertion.predicate, &valid_uuid, &uuid_assertion.value));
    
    // Test invalid UUID
    let invalid_uuid = Value::String("not-a-uuid".to_string());
    assert!(!evaluate_predicate(&uuid_assertion.predicate, &invalid_uuid, &uuid_assertion.value));
    
    // Test another valid UUID format
    let another_valid_uuid = Value::String("123e4567-e89b-12d3-a456-426614174000".to_string());
    assert!(evaluate_predicate(&uuid_assertion.predicate, &another_valid_uuid, &uuid_assertion.value));
}

#[test]
fn test_httpbin_ip_validation() {
    let ip_assertion = HttpAssertion {
        query: AssertionQuery::JsonPath { path: "$.origin".to_string() },
        predicate: AssertionPredicate::IsIpv4,
        value: AssertionValue::String("".to_string()),
    };
    
    // Test valid IPv4
    let valid_ipv4 = Value::String("192.168.1.1".to_string());
    assert!(evaluate_predicate(&ip_assertion.predicate, &valid_ipv4, &ip_assertion.value));
    
    // Test invalid IP
    let invalid_ip = Value::String("not-an-ip".to_string());
    assert!(!evaluate_predicate(&ip_assertion.predicate, &invalid_ip, &ip_assertion.value));
    
    // Test IPv6 with IPv4 predicate (should fail)
    let ipv6_addr = Value::String("2001:0db8:85a3:0000:0000:8a2e:0370:7334".to_string());
    assert!(!evaluate_predicate(&ip_assertion.predicate, &ipv6_addr, &ip_assertion.value));
}

#[test]
fn test_httpbin_ipv6_validation() {
    let ipv6_assertion = HttpAssertion {
        query: AssertionQuery::JsonPath { path: "$.origin".to_string() },
        predicate: AssertionPredicate::IsIpv6,
        value: AssertionValue::String("".to_string()),
    };
    
    // Test valid IPv6
    let valid_ipv6 = Value::String("2001:0db8:85a3:0000:0000:8a2e:0370:7334".to_string());
    assert!(evaluate_predicate(&ipv6_assertion.predicate, &valid_ipv6, &ipv6_assertion.value));
    
    // Test compressed IPv6
    let compressed_ipv6 = Value::String("2001:db8::8a2e:370:7334".to_string());
    assert!(evaluate_predicate(&ipv6_assertion.predicate, &compressed_ipv6, &ipv6_assertion.value));
    
    // Test IPv4 with IPv6 predicate (should fail)
    let ipv4_addr = Value::String("192.168.1.1".to_string());
    assert!(!evaluate_predicate(&ipv6_assertion.predicate, &ipv4_addr, &ipv6_assertion.value));
}

// =============================================================================
// Date and Time Validation Tests
// =============================================================================

#[test]
fn test_httpbin_iso_date_validation() {
    let date_assertion = HttpAssertion {
        query: AssertionQuery::JsonPath { path: "$.timestamp".to_string() },
        predicate: AssertionPredicate::IsIsoDate,
        value: AssertionValue::String("".to_string()),
    };
    
    // Test valid ISO date
    let valid_iso_date = Value::String("2024-01-15T10:30:00Z".to_string());
    assert!(evaluate_predicate(&date_assertion.predicate, &valid_iso_date, &date_assertion.value));
    
    // Test ISO date with timezone
    let iso_date_with_tz = Value::String("2024-01-15T10:30:00+00:00".to_string());
    assert!(evaluate_predicate(&date_assertion.predicate, &iso_date_with_tz, &date_assertion.value));
    
    // Test invalid date format
    let invalid_date = Value::String("not-a-date".to_string());
    assert!(!evaluate_predicate(&date_assertion.predicate, &invalid_date, &date_assertion.value));
}

// =============================================================================
// Certificate Field Tests
// =============================================================================

#[test]
fn test_httpbin_certificate_assertions() {
    // Test certificate field assertions (for HTTPS endpoints)
    let cert_subject_assertion = HttpAssertion {
        query: AssertionQuery::Certificate { field: CertificateField::Subject },
        predicate: AssertionPredicate::Contains,
        value: AssertionValue::String("httpbin.org".to_string()),
    };
    
    let cert_issuer_assertion = HttpAssertion {
        query: AssertionQuery::Certificate { field: CertificateField::Issuer },
        predicate: AssertionPredicate::Exists,
        value: AssertionValue::String("".to_string()),
    };
    
    let cert_algorithm_assertion = HttpAssertion {
        query: AssertionQuery::Certificate { field: CertificateField::Algorithm },
        predicate: AssertionPredicate::Exists,
        value: AssertionValue::String("".to_string()),
    };
    
    // These assertions would be evaluated against actual certificate data
    // Note: AssertionQuery and AssertionPredicate don't implement PartialEq, so we test the structure using pattern matching
    match (&cert_subject_assertion.query, &cert_subject_assertion.predicate) {
        (AssertionQuery::Certificate { field: CertificateField::Subject }, AssertionPredicate::Contains) => {
            // Test passed - correct query and predicate combination
        },
        _ => panic!("Expected Certificate Subject query with Contains predicate"),
    }
    
    match (&cert_issuer_assertion.query, &cert_issuer_assertion.predicate) {
        (AssertionQuery::Certificate { field: CertificateField::Issuer }, AssertionPredicate::Exists) => {
            // Test passed - correct query and predicate combination
        },
        _ => panic!("Expected Certificate Issuer query with Exists predicate"),
    }
    
    match (&cert_algorithm_assertion.query, &cert_algorithm_assertion.predicate) {
        (AssertionQuery::Certificate { field: CertificateField::Algorithm }, AssertionPredicate::Exists) => {
            // Test passed - correct query and predicate combination
        },
        _ => panic!("Expected Certificate Algorithm query with Exists predicate"),
    }
}

// =============================================================================
// Complex Multi-Assertion Tests
// =============================================================================

#[test]
fn test_httpbin_complex_assertions_combination() {
    let mut check = create_httpbin_https_check("/get?param1=value1&param2=value2", HttpMethod::Get, 200);
    
    // Add multiple assertions for comprehensive validation
    let assertions = vec![
        // Status check
        HttpAssertion {
            query: AssertionQuery::Status,
            predicate: AssertionPredicate::Equals,
            value: AssertionValue::Integer(200),
        },
        // URL validation
        HttpAssertion {
            query: AssertionQuery::JsonPath { path: "$.url".to_string() },
            predicate: AssertionPredicate::Contains,
            value: AssertionValue::String("httpbin.org/get".to_string()),
        },
        // Parameter validation
        HttpAssertion {
            query: AssertionQuery::JsonPath { path: "$.args.param1".to_string() },
            predicate: AssertionPredicate::Equals,
            value: AssertionValue::String("value1".to_string()),
        },
        // Header validation
        HttpAssertion {
            query: AssertionQuery::Header { name: "Content-Type".to_string() },
            predicate: AssertionPredicate::Contains,
            value: AssertionValue::String("application/json".to_string()),
        },
        // Response time validation
        HttpAssertion {
            query: AssertionQuery::Duration,
            predicate: AssertionPredicate::LessThan,
            value: AssertionValue::Integer(5000), // Less than 5 seconds
        },
    ];
    
    check.assertions = Some(assertions);
    
    assert_eq!(check.assertions.as_ref().unwrap().len(), 5);
    assert_eq!(check.path, "/get?param1=value1&param2=value2");
    
    // Test individual assertion evaluation
    let assertions_ref = check.assertions.as_ref().unwrap();
    
    // Test status assertion
    let status_200 = Value::Number(serde_json::Number::from(200));
    assert!(evaluate_predicate(&assertions_ref[0].predicate, &status_200, &assertions_ref[0].value));
    
    // Test URL assertion
    let url_value = Value::String("https://httpbin.org/get?param1=value1".to_string());
    assert!(evaluate_predicate(&assertions_ref[1].predicate, &url_value, &assertions_ref[1].value));
    
    // Test parameter assertion
    let param_value = Value::String("value1".to_string());
    assert!(evaluate_predicate(&assertions_ref[2].predicate, &param_value, &assertions_ref[2].value));
    
    // Test content type assertion
    let content_type = Value::String("application/json; charset=utf-8".to_string());
    assert!(evaluate_predicate(&assertions_ref[3].predicate, &content_type, &assertions_ref[3].value));
    
    // Test duration assertion
    let response_time = Value::Number(serde_json::Number::from(1500)); // 1.5 seconds
    assert!(evaluate_predicate(&assertions_ref[4].predicate, &response_time, &assertions_ref[4].value));
}

// =============================================================================
// Configuration Tests
// =============================================================================

#[test]
fn test_httpbin_monitoring_configuration() {
    // Test creating a complete monitoring configuration for httpbin.org
    let config = AppConfig {
        monitoring_interval_seconds: 30,
        memory_cleanup_interval_minutes: 60,
        keep_history_hours: 24,
        hosts: vec![
            HostConfig {
                address: "httpbin.org".to_string(),
                alias: Some("HttpBin-Status".to_string()),
                checks: vec![
                    Check::Http(create_httpbin_https_check("/status/200", HttpMethod::Get, 200)),
                ],
            },
            HostConfig {
                address: "httpbin.org".to_string(),
                alias: Some("HttpBin-JSON".to_string()),
                checks: vec![
                    Check::Http(create_httpbin_https_check("/json", HttpMethod::Get, 200)),
                ],
            },
            HostConfig {
                address: "httpbin.org".to_string(),
                alias: Some("HttpBin-Auth".to_string()),
                checks: vec![
                    Check::Http({
                        let mut check = create_httpbin_https_check("/basic-auth/user/pass", HttpMethod::Get, 200);
                        check.auth = Some(AuthConfig::Basic {
                            username: "user".to_string(),
                            password: "pass".to_string(),
                        });
                        check
                    }),
                ],
            },
        ],
    };
    
    assert_eq!(config.hosts.len(), 3);
    assert_eq!(config.monitoring_interval_seconds, 30);
    assert_eq!(config.memory_cleanup_interval_minutes, 60);
    assert_eq!(config.keep_history_hours, 24);
    
    // Verify each host configuration
    for host in &config.hosts {
        assert_eq!(host.address, "httpbin.org");
        assert!(host.alias.is_some());
        assert_eq!(host.checks.len(), 1);
        
        if let Check::Http(http_check) = &host.checks[0] {
            assert_eq!(http_check.protocol, HttpProtocol::Https);
            assert_eq!(http_check.port, 443);
            assert!(http_check.path.starts_with("/"));
            assert!(http_check.assertions.is_some());
        }
    }
}

// =============================================================================
// Edge Cases and Error Handling Tests
// =============================================================================

#[test]
fn test_httpbin_timeout_configuration() {
    let mut check = create_httpbin_https_check("/delay/3", HttpMethod::Get, 200);
    check.timeout_seconds = 5; // Set timeout longer than delay
    
    assert_eq!(check.path, "/delay/3");
    assert_eq!(check.timeout_seconds, 5);
}

#[test]
fn test_httpbin_ssl_certificate_configuration() {
    let mut check = create_httpbin_https_check("/", HttpMethod::Get, 200);
    check.check_ssl_certificate = true;
    
    assert_eq!(check.protocol, HttpProtocol::Https);
    assert_eq!(check.check_ssl_certificate, true);
    assert_eq!(check.port, 443);
}

#[test]
fn test_httpbin_various_endpoints() {
    let endpoints = vec![
        "/base64/SFRUUEJJTiBpcyBhd2Vzb21l", // Base64 decode
        "/bytes/1024",                        // Random bytes
        "/cache/60",                          // Cache control
        "/cookies",                           // Cookie handling
        "/deflate",                           // Compression
        "/etag/test-etag",                   // ETag handling
        "/gzip",                             // Gzip compression
        "/image/png",                        // Image response
        "/redirect/3",                       // Redirects
        "/robots.txt",                       // Robots.txt
        "/stream/5",                         // Streaming response
        "/uuid",                             // UUID generation
    ];
    
    for endpoint in endpoints {
        let check = create_httpbin_https_check(endpoint, HttpMethod::Get, 200);
        assert_eq!(check.path, endpoint);
        assert_eq!(check.protocol, HttpProtocol::Https);
        assert_eq!(check.expected_status_code, 200);
    }
}