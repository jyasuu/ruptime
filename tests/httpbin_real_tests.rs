use uptime_monitor::config::*;
use uptime_monitor::test_utils::*;
use uptime_monitor::monitoring::evaluate_predicate;
use serde_json::{json, Value};
use std::collections::HashMap;
use reqwest;

/// Test helper to create HTTPS httpbin check
fn create_httpbin_https_check(path: &str, method: HttpMethod, expected_status: u16) -> HttpCheck {
    HttpCheck {
        port: 443,
        path: path.to_string(),
        protocol: HttpProtocol::Https,
        method,
        timeout_seconds: 30, // Increased timeout for real requests
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

/// Helper function to make actual HTTP request to httpbin.org
async fn make_httpbin_request(check: &HttpCheck) -> Result<(reqwest::StatusCode, String, reqwest::header::HeaderMap), reqwest::Error> {
    let client = reqwest::Client::new();
    let url = format!("https://httpbin.org{}", check.path);
    
    let mut request_builder = match check.method {
        HttpMethod::Get => client.get(&url),
        HttpMethod::Post => client.post(&url),
        HttpMethod::Put => client.put(&url),
        HttpMethod::Delete => client.delete(&url),
        HttpMethod::Head => client.head(&url),
        HttpMethod::Options => {
            // Options might not be directly supported, use GET as fallback
            client.get(&url)
        },
    };
    
    // Add custom headers if specified
    if let Some(ref headers) = check.headers {
        for (key, value) in headers {
            request_builder = request_builder.header(key, value);
        }
    }
    
    // Add authentication if specified
    if let Some(ref auth) = check.auth {
        match auth {
            AuthConfig::Basic { username, password } => {
                request_builder = request_builder.basic_auth(username, Some(password));
            },
            AuthConfig::Bearer { token } => {
                request_builder = request_builder.bearer_auth(token);
            },
            AuthConfig::OAuth2 { .. } => {
                // OAuth2 is more complex, would need token exchange
                // For testing, we'll skip this
            },
        }
    }
    
    let response = request_builder.send().await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await?;
    
    Ok((status, body, headers))
}

// =============================================================================
// Real HTTP Request Tests to httpbin.org
// =============================================================================

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_get_real_request() {
    let check = create_httpbin_https_check("/get", HttpMethod::Get, 200);
    
    let result = make_httpbin_request(&check).await;
    assert!(result.is_ok(), "Failed to make request: {:?}", result.err());
    
    let (status, body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Parse JSON response
    let json_response: Value = serde_json::from_str(&body).expect("Response should be valid JSON");
    
    // Test that response contains expected fields
    assert!(json_response.get("url").is_some());
    assert!(json_response.get("headers").is_some());
    assert!(json_response.get("origin").is_some());
    
    // Test URL field
    let url = json_response["url"].as_str().unwrap();
    assert!(url.contains("httpbin.org/get"));
    
    println!("✓ GET request successful: {}", url);
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_post_real_request() {
    let mut check = create_httpbin_https_check("/post", HttpMethod::Post, 200);
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    check.headers = Some(headers);
    
    let result = make_httpbin_request(&check).await;
    assert!(result.is_ok(), "Failed to make POST request: {:?}", result.err());
    
    let (status, body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Parse JSON response
    let json_response: Value = serde_json::from_str(&body).expect("Response should be valid JSON");
    
    // Test that response contains expected fields for POST
    assert!(json_response.get("url").is_some());
    assert!(json_response.get("data").is_some());
    assert!(json_response.get("headers").is_some());
    
    // Test Content-Type header was sent
    let headers = &json_response["headers"];
    assert!(headers.get("Content-Type").is_some());
    
    println!("✓ POST request successful");
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_status_codes_real() {
    let test_cases = vec![200, 201, 400, 404, 500];
    
    for status_code in test_cases {
        let check = create_httpbin_https_check(&format!("/status/{}", status_code), HttpMethod::Get, status_code);
        
        let result = make_httpbin_request(&check).await;
        assert!(result.is_ok(), "Failed to make request for status {}: {:?}", status_code, result.err());
        
        let (actual_status, _body, _headers) = result.unwrap();
        assert_eq!(actual_status.as_u16(), status_code, "Status code mismatch for {}", status_code);
        
        println!("✓ Status {} test successful", status_code);
    }
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_basic_auth_real() {
    let mut check = create_httpbin_https_check("/basic-auth/testuser/testpass", HttpMethod::Get, 200);
    check.auth = Some(AuthConfig::Basic {
        username: "testuser".to_string(),
        password: "testpass".to_string(),
    });
    
    let result = make_httpbin_request(&check).await;
    assert!(result.is_ok(), "Failed to make authenticated request: {:?}", result.err());
    
    let (status, body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Parse JSON response
    let json_response: Value = serde_json::from_str(&body).expect("Response should be valid JSON");
    
    // Test authentication success
    assert_eq!(json_response["authenticated"], true);
    assert_eq!(json_response["user"], "testuser");
    
    println!("✓ Basic auth test successful");
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_bearer_auth_real() {
    let mut check = create_httpbin_https_check("/bearer", HttpMethod::Get, 200);
    check.auth = Some(AuthConfig::Bearer {
        token: "test-token-12345".to_string(),
    });
    
    let result = make_httpbin_request(&check).await;
    assert!(result.is_ok(), "Failed to make bearer auth request: {:?}", result.err());
    
    let (status, body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Parse JSON response
    let json_response: Value = serde_json::from_str(&body).expect("Response should be valid JSON");
    
    // Test bearer token was received
    assert_eq!(json_response["authenticated"], true);
    assert_eq!(json_response["token"], "test-token-12345");
    
    println!("✓ Bearer auth test successful");
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_headers_real() {
    let mut check = create_httpbin_https_check("/headers", HttpMethod::Get, 200);
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), "UptimeMonitor/1.0".to_string());
    headers.insert("X-Custom-Header".to_string(), "test-value".to_string());
    check.headers = Some(headers);
    
    let result = make_httpbin_request(&check).await;
    assert!(result.is_ok(), "Failed to make headers request: {:?}", result.err());
    
    let (status, body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Parse JSON response
    let json_response: Value = serde_json::from_str(&body).expect("Response should be valid JSON");
    
    // Test that our custom headers were sent
    let request_headers = &json_response["headers"];
    assert!(request_headers.get("User-Agent").is_some());
    assert!(request_headers.get("X-Custom-Header").is_some());
    
    let user_agent = request_headers["User-Agent"].as_str().unwrap();
    assert!(user_agent.contains("UptimeMonitor/1.0"));
    
    let custom_header = request_headers["X-Custom-Header"].as_str().unwrap();
    assert_eq!(custom_header, "test-value");
    
    println!("✓ Headers test successful");
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_ip_real() {
    let check = create_httpbin_https_check("/ip", HttpMethod::Get, 200);
    
    let result = make_httpbin_request(&check).await;
    assert!(result.is_ok(), "Failed to make IP request: {:?}", result.err());
    
    let (status, body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Parse JSON response
    let json_response: Value = serde_json::from_str(&body).expect("Response should be valid JSON");
    
    // Test that we got an IP address
    assert!(json_response.get("origin").is_some());
    let origin = json_response["origin"].as_str().unwrap();
    
    // Basic IP format validation (should contain dots or colons)
    assert!(origin.contains('.') || origin.contains(':'));
    
    println!("✓ IP request successful, origin: {}", origin);
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_json_real() {
    let check = create_httpbin_https_check("/json", HttpMethod::Get, 200);
    
    let result = make_httpbin_request(&check).await;
    assert!(result.is_ok(), "Failed to make JSON request: {:?}", result.err());
    
    let (status, body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Parse JSON response
    let json_response: Value = serde_json::from_str(&body).expect("Response should be valid JSON");
    
    // Test that response contains the expected JSON structure
    assert!(json_response.get("slideshow").is_some());
    let slideshow = &json_response["slideshow"];
    assert!(slideshow.get("title").is_some());
    assert!(slideshow.get("author").is_some());
    
    println!("✓ JSON request successful");
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_uuid_real() {
    let check = create_httpbin_https_check("/uuid", HttpMethod::Get, 200);
    
    let result = make_httpbin_request(&check).await;
    assert!(result.is_ok(), "Failed to make UUID request: {:?}", result.err());
    
    let (status, body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Parse JSON response
    let json_response: Value = serde_json::from_str(&body).expect("Response should be valid JSON");
    
    // Test that response contains a UUID
    assert!(json_response.get("uuid").is_some());
    let uuid = json_response["uuid"].as_str().unwrap();
    
    // Basic UUID format validation (8-4-4-4-12 hex digits)
    let uuid_parts: Vec<&str> = uuid.split('-').collect();
    assert_eq!(uuid_parts.len(), 5);
    assert_eq!(uuid_parts[0].len(), 8);
    assert_eq!(uuid_parts[1].len(), 4);
    assert_eq!(uuid_parts[2].len(), 4);
    assert_eq!(uuid_parts[3].len(), 4);
    assert_eq!(uuid_parts[4].len(), 12);
    
    // Test UUID validation with assertion logic
    let uuid_value = Value::String(uuid.to_string());
    let uuid_assertion = HttpAssertion {
        query: AssertionQuery::JsonPath { path: "$.uuid".to_string() },
        predicate: AssertionPredicate::IsUuid,
        value: AssertionValue::String("".to_string()),
    };
    assert!(evaluate_predicate(&uuid_assertion.predicate, &uuid_value, &uuid_assertion.value));
    
    println!("✓ UUID request successful: {}", uuid);
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests  
async fn test_httpbin_user_agent_real() {
    let mut check = create_httpbin_https_check("/user-agent", HttpMethod::Get, 200);
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), "TestAgent/2.0".to_string());
    check.headers = Some(headers);
    
    let result = make_httpbin_request(&check).await;
    assert!(result.is_ok(), "Failed to make user-agent request: {:?}", result.err());
    
    let (status, body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Parse JSON response  
    let json_response: Value = serde_json::from_str(&body).expect("Response should be valid JSON");
    
    // Test that our user agent was echoed back
    assert!(json_response.get("user-agent").is_some());
    let user_agent = json_response["user-agent"].as_str().unwrap();
    assert_eq!(user_agent, "TestAgent/2.0");
    
    println!("✓ User-Agent request successful: {}", user_agent);
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_redirect_real() {
    let check = create_httpbin_https_check("/redirect/1", HttpMethod::Get, 302);
    
    // Create client that doesn't follow redirects
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    
    let url = format!("https://httpbin.org{}", check.path);
    let response = client.get(&url).send().await;
    
    assert!(response.is_ok(), "Failed to make redirect request: {:?}", response.err());
    
    let response = response.unwrap();
    let status = response.status();
    
    // Test that we got a redirect status
    assert_eq!(status.as_u16(), 302);
    
    // Test that Location header exists
    assert!(response.headers().get("location").is_some());
    
    println!("✓ Redirect test successful, got 302");
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_delay_real() {
    let check = create_httpbin_https_check("/delay/1", HttpMethod::Get, 200);
    
    let start = std::time::Instant::now();
    let result = make_httpbin_request(&check).await;
    let duration = start.elapsed();
    
    assert!(result.is_ok(), "Failed to make delay request: {:?}", result.err());
    
    let (status, _body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Test that request took at least 1 second (with some tolerance)
    assert!(duration.as_secs() >= 1, "Request should have taken at least 1 second, took: {:?}", duration);
    assert!(duration.as_secs() < 5, "Request took too long: {:?}", duration);
    
    println!("✓ Delay test successful, took: {:?}", duration);
}

// =============================================================================
// Assertion Evaluation Tests with Real Data
// =============================================================================

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_assertions_with_real_data() {
    let mut check = create_httpbin_https_check("/get?test=hello&number=42", HttpMethod::Get, 200);
    
    // Add comprehensive assertions
    let assertions = vec![
        // Status assertion
        HttpAssertion {
            query: AssertionQuery::Status,
            predicate: AssertionPredicate::Equals,
            value: AssertionValue::Integer(200),
        },
        // URL contains httpbin
        HttpAssertion {
            query: AssertionQuery::JsonPath { path: "$.url".to_string() },
            predicate: AssertionPredicate::Contains,
            value: AssertionValue::String("httpbin.org".to_string()),
        },
        // Test parameter
        HttpAssertion {
            query: AssertionQuery::JsonPath { path: "$.args.test".to_string() },
            predicate: AssertionPredicate::Equals,
            value: AssertionValue::String("hello".to_string()),
        },
        // Number parameter
        HttpAssertion {
            query: AssertionQuery::JsonPath { path: "$.args.number".to_string() },
            predicate: AssertionPredicate::Equals,
            value: AssertionValue::String("42".to_string()),
        },
        // Origin should be an IP
        HttpAssertion {
            query: AssertionQuery::JsonPath { path: "$.origin".to_string() },
            predicate: AssertionPredicate::IsIpv4,
            value: AssertionValue::String("".to_string()),
        },
    ];
    
    check.assertions = Some(assertions);
    
    let result = make_httpbin_request(&check).await;
    assert!(result.is_ok(), "Failed to make request: {:?}", result.err());
    
    let (status, body, _headers) = result.unwrap();
    
    // Test status code
    assert_eq!(status.as_u16(), 200);
    
    // Parse JSON response
    let json_response: Value = serde_json::from_str(&body).expect("Response should be valid JSON");
    
    // Test each assertion against real data
    let assertions_ref = check.assertions.as_ref().unwrap();
    
    // Status assertion
    let status_value = Value::Number(serde_json::Number::from(200));
    assert!(evaluate_predicate(&assertions_ref[0].predicate, &status_value, &assertions_ref[0].value));
    
    // URL assertion
    let url_value = Value::String(json_response["url"].as_str().unwrap().to_string());
    assert!(evaluate_predicate(&assertions_ref[1].predicate, &url_value, &assertions_ref[1].value));
    
    // Test parameter assertion
    let test_value = Value::String(json_response["args"]["test"].as_str().unwrap().to_string());
    assert!(evaluate_predicate(&assertions_ref[2].predicate, &test_value, &assertions_ref[2].value));
    
    // Number parameter assertion
    let number_value = Value::String(json_response["args"]["number"].as_str().unwrap().to_string());
    assert!(evaluate_predicate(&assertions_ref[3].predicate, &number_value, &assertions_ref[3].value));
    
    // Origin IP assertion
    let origin_value = Value::String(json_response["origin"].as_str().unwrap().to_string());
    assert!(evaluate_predicate(&assertions_ref[4].predicate, &origin_value, &assertions_ref[4].value));
    
    println!("✓ All assertions passed with real data");
    println!("  URL: {}", json_response["url"]);
    println!("  Test param: {}", json_response["args"]["test"]);
    println!("  Number param: {}", json_response["args"]["number"]);
    println!("  Origin: {}", json_response["origin"]);
}

// =============================================================================
// Monitoring Configuration Test with Real Requests
// =============================================================================

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_monitoring_configuration_real() {
    // Test a complete monitoring configuration
    let config = AppConfig {
        monitoring_interval_seconds: 30,
        memory_cleanup_interval_minutes: 60,
        keep_history_hours: 24,
        hosts: vec![
            HostConfig {
                address: "httpbin.org".to_string(),
                alias: Some("HttpBin-GET".to_string()),
                checks: vec![
                    Check::Http(create_httpbin_https_check("/get", HttpMethod::Get, 200)),
                ],
            },
            HostConfig {
                address: "httpbin.org".to_string(),
                alias: Some("HttpBin-Status".to_string()),
                checks: vec![
                    Check::Http(create_httpbin_https_check("/status/200", HttpMethod::Get, 200)),
                ],
            },
        ],
    };
    
    assert_eq!(config.hosts.len(), 2);
    
    // Test each check by making real requests
    for host in &config.hosts {
        for check in &host.checks {
            if let Check::Http(http_check) = check {
                let result = make_httpbin_request(http_check).await;
                assert!(result.is_ok(), "Failed request for {}: {:?}", host.alias.as_ref().unwrap(), result.err());
                
                let (status, _body, _headers) = result.unwrap();
                assert_eq!(status.as_u16(), http_check.expected_status_code);
                
                println!("✓ {} check successful", host.alias.as_ref().unwrap());
            }
        }
    }
    
    println!("✓ Complete monitoring configuration test successful");
}

// =============================================================================
// Edge Cases and Error Handling with Real Requests  
// =============================================================================

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_error_cases_real() {
    // Test various error responses
    let error_cases = vec![
        ("/status/404", 404),
        ("/status/500", 500),
        ("/status/403", 403),
    ];
    
    for (path, expected_status) in error_cases {
        let check = create_httpbin_https_check(path, HttpMethod::Get, expected_status);
        
        let result = make_httpbin_request(&check).await;
        assert!(result.is_ok(), "Failed to make request for {}: {:?}", path, result.err());
        
        let (status, _body, _headers) = result.unwrap();
        assert_eq!(status.as_u16(), expected_status, "Status mismatch for {}", path);
        
        println!("✓ Error case {} test successful", expected_status);
    }
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_content_types_real() {
    let content_type_cases = vec![
        ("/json", "application/json"),
        ("/html", "text/html"),
        ("/xml", "application/xml"),
    ];
    
    for (path, expected_content_type) in content_type_cases {
        let check = create_httpbin_https_check(path, HttpMethod::Get, 200);
        
        let result = make_httpbin_request(&check).await;
        assert!(result.is_ok(), "Failed to make request for {}: {:?}", path, result.err());
        
        let (status, _body, headers) = result.unwrap();
        assert_eq!(status.as_u16(), 200);
        
        // Check content type
        let content_type = headers.get("content-type");
        assert!(content_type.is_some(), "Content-Type header missing for {}", path);
        
        let content_type_str = content_type.unwrap().to_str().unwrap();
        assert!(content_type_str.contains(expected_content_type), 
                "Content-Type mismatch for {}: expected {}, got {}", 
                path, expected_content_type, content_type_str);
        
        println!("✓ Content-Type test for {} successful: {}", path, content_type_str);
    }
}