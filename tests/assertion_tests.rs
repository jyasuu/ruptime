use uptime_monitor::config::*;
use uptime_monitor::monitoring::*;
use uptime_monitor::test_utils::*;
use reqwest;
use serde_json::json;

// Mock response for testing
fn create_mock_response() -> reqwest::Response {
    // This is a simplified mock - in real tests you might use a mock server
    // For now, we'll test the assertion logic directly
    unimplemented!("Use integration tests with real HTTP responses")
}

#[tokio::test]
async fn test_status_assertion() {
    let assertion = create_test_assertion(
        AssertionQuery::Status,
        AssertionPredicate::Equals,
        AssertionValue::Integer(200),
    );
    
    let response_body = create_mock_response_body();
    
    // Create a mock response with status 200
    // Note: This would need a proper mock in real implementation
    // For now, test the assertion evaluation directly
    
    // Test with status 200
    let status_value = serde_json::Value::Number(serde_json::Number::from(200));
    assert!(evaluate_predicate(&assertion.predicate, &status_value, &assertion.value));
    
    // Test with status 404
    let status_value = serde_json::Value::Number(serde_json::Number::from(404));
    assert!(!evaluate_predicate(&assertion.predicate, &status_value, &assertion.value));
}

#[test]
fn test_jsonpath_assertions() {
    let response_body = create_mock_response_body();
    let json: serde_json::Value = serde_json::from_str(&response_body).unwrap();
    
    // Test extracting test_param
    let results = jsonpath_lib::select(&json, "$.args.test_param").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], serde_json::Value::String("hello".to_string()));
    
    // Test number extraction
    let results = jsonpath_lib::select(&json, "$.args.number").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], serde_json::Value::String("42".to_string()));
    
    // Test URL extraction
    let results = jsonpath_lib::select(&json, "$.url").unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].as_str().unwrap().starts_with("https://httpbin.org"));
}

#[test]
fn test_string_predicates() {
    let test_value = serde_json::Value::String("Hello World".to_string());
    
    // Test StartsWith
    assert!(string_predicate(&test_value, &AssertionValue::String("Hello".to_string()), |a, b| a.starts_with(b)));
    assert!(!string_predicate(&test_value, &AssertionValue::String("World".to_string()), |a, b| a.starts_with(b)));
    
    // Test EndsWith
    assert!(string_predicate(&test_value, &AssertionValue::String("World".to_string()), |a, b| a.ends_with(b)));
    assert!(!string_predicate(&test_value, &AssertionValue::String("Hello".to_string()), |a, b| a.ends_with(b)));
    
    // Test Contains
    assert!(string_predicate(&test_value, &AssertionValue::String("lo Wo".to_string()), |a, b| a.contains(b)));
    assert!(!string_predicate(&test_value, &AssertionValue::String("xyz".to_string()), |a, b| a.contains(b)));
}

#[test]
fn test_number_predicates() {
    let test_value = serde_json::Value::Number(serde_json::Number::from(42));
    
    // Test equals
    assert!(values_equal(&test_value, &AssertionValue::Integer(42)));
    assert!(!values_equal(&test_value, &AssertionValue::Integer(43)));
    
    // Test greater than
    assert!(compare_values(&test_value, &AssertionValue::Integer(41), |a, b| a > b));
    assert!(!compare_values(&test_value, &AssertionValue::Integer(42), |a, b| a > b));
    assert!(!compare_values(&test_value, &AssertionValue::Integer(43), |a, b| a > b));
    
    // Test less than
    assert!(compare_values(&test_value, &AssertionValue::Integer(43), |a, b| a < b));
    assert!(!compare_values(&test_value, &AssertionValue::Integer(42), |a, b| a < b));
    assert!(!compare_values(&test_value, &AssertionValue::Integer(41), |a, b| a < b));
}

#[test]
fn test_regex_predicate() {
    let test_value = serde_json::Value::String("2024-01-15T10:30:00Z".to_string());
    
    // Test ISO date regex
    let iso_pattern = AssertionValue::String(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z".to_string());
    assert!(regex_predicate(&test_value, &iso_pattern));
    
    // Test non-matching regex
    let email_pattern = AssertionValue::String(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}".to_string());
    assert!(!regex_predicate(&test_value, &email_pattern));
}

#[test]
fn test_type_predicates() {
    // Test boolean
    let bool_value = serde_json::Value::Bool(true);
    assert!(bool_value.is_boolean());
    assert!(!bool_value.is_string());
    assert!(!bool_value.is_number());
    
    // Test string
    let string_value = serde_json::Value::String("test".to_string());
    assert!(string_value.is_string());
    assert!(!string_value.is_boolean());
    assert!(!string_value.is_number());
    
    // Test number
    let number_value = serde_json::Value::Number(serde_json::Number::from(42));
    assert!(number_value.is_number());
    assert!(!number_value.is_boolean());
    assert!(!number_value.is_string());
    
    // Test array
    let array_value = serde_json::Value::Array(vec![]);
    assert!(array_value.is_array());
    assert!(!array_value.is_string());
}

#[test]
fn test_validation_predicates() {
    // Test UUID validation
    let valid_uuid = serde_json::Value::String("550e8400-e29b-41d4-a716-446655440000".to_string());
    assert!(is_uuid(&valid_uuid));
    
    let invalid_uuid = serde_json::Value::String("not-a-uuid".to_string());
    assert!(!is_uuid(&invalid_uuid));
    
    // Test IPv4 validation
    let valid_ipv4 = serde_json::Value::String("192.168.1.100".to_string());
    assert!(is_ipv4(&valid_ipv4));
    
    let invalid_ipv4 = serde_json::Value::String("999.999.999.999".to_string());
    assert!(!is_ipv4(&invalid_ipv4));
    
    // Test IPv6 validation
    let valid_ipv6 = serde_json::Value::String("2001:0db8:85a3:0000:0000:8a2e:0370:7334".to_string());
    assert!(is_ipv6(&valid_ipv6));
    
    let invalid_ipv6 = serde_json::Value::String("not-an-ipv6".to_string());
    assert!(!is_ipv6(&invalid_ipv6));
    
    // Test ISO date validation
    let valid_iso_date = serde_json::Value::String("2024-01-15T10:30:00Z".to_string());
    assert!(is_iso_date(&valid_iso_date));
    
    let invalid_iso_date = serde_json::Value::String("not-a-date".to_string());
    assert!(!is_iso_date(&invalid_iso_date));
}

#[test]
fn test_complex_assertion_combinations() {
    let response_body = create_mock_response_body();
    let json: serde_json::Value = serde_json::from_str(&response_body).unwrap();
    
    // Test multiple assertions that should pass
    let assertions = vec![
        create_test_assertion(
            AssertionQuery::JsonPath { path: "$.args.test_param".to_string() },
            AssertionPredicate::Equals,
            AssertionValue::String("hello".to_string()),
        ),
        create_test_assertion(
            AssertionQuery::JsonPath { path: "$.url".to_string() },
            AssertionPredicate::StartsWith,
            AssertionValue::String("https://".to_string()),
        ),
        create_test_assertion(
            AssertionQuery::JsonPath { path: "$.authenticated".to_string() },
            AssertionPredicate::Equals,
            AssertionValue::Boolean(true),
        ),
        create_test_assertion(
            AssertionQuery::JsonPath { path: "$.uuid".to_string() },
            AssertionPredicate::IsUuid,
            AssertionValue::Null,
        ),
    ];
    
    // Manually test each assertion
    for assertion in assertions {
        match &assertion.query {
            AssertionQuery::JsonPath { path } => {
                let results = jsonpath_lib::select(&json, path).unwrap();
                if !results.is_empty() {
                    let actual = &results[0];
                    assert!(evaluate_predicate(&assertion.predicate, actual, &assertion.value));
                }
            },
            _ => {}
        }
    }
}

#[test]
fn test_negative_assertions() {
    let response_body = create_mock_response_body();
    let json: serde_json::Value = serde_json::from_str(&response_body).unwrap();
    
    // Test NotExists predicate
    let results = jsonpath_lib::select(&json, "$.nonexistent_field");
    assert!(results.is_ok());
    assert!(results.unwrap().is_empty());
    
    // Test NotEquals predicate
    let results = jsonpath_lib::select(&json, "$.args.test_param").unwrap();
    assert!(!values_equal(&results[0], &AssertionValue::String("goodbye".to_string())));
    
    // Test NotContains predicate
    let url_results = jsonpath_lib::select(&json, "$.url").unwrap();
    assert!(!string_predicate(&url_results[0], &AssertionValue::String("forbidden".to_string()), |a, b| a.contains(b)));
}

#[test]
fn test_assertion_formatting() {
    // Test query formatting
    assert_eq!(format_query(&AssertionQuery::Status), "status");
    assert_eq!(format_query(&AssertionQuery::Header { name: "Content-Type".to_string() }), "header[Content-Type]");
    assert_eq!(format_query(&AssertionQuery::JsonPath { path: "$.test".to_string() }), "jsonpath[$.test]");
    
    // Test predicate formatting
    assert_eq!(format_predicate(&AssertionPredicate::Equals), "==");
    assert_eq!(format_predicate(&AssertionPredicate::Contains), "contains");
    assert_eq!(format_predicate(&AssertionPredicate::IsUuid), "isUuid");
    
    // Test value formatting
    assert_eq!(format_assertion_value(&AssertionValue::String("test".to_string())), "\"test\"");
    assert_eq!(format_assertion_value(&AssertionValue::Integer(42)), "42");
    assert_eq!(format_assertion_value(&AssertionValue::Boolean(true)), "true");
    assert_eq!(format_assertion_value(&AssertionValue::Null), "null");
}

// Integration test with httpbin.org (requires network)
#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_integration() {
    let client = reqwest::Client::new();
    
    // Test GET endpoint
    let response = client
        .get("https://httpbin.org/get?test=hello")
        .send()
        .await
        .expect("Failed to send request");
    
    assert_eq!(response.status(), 200);
    
    let body = response.text().await.expect("Failed to read response body");
    let json: serde_json::Value = serde_json::from_str(&body).expect("Failed to parse JSON");
    
    // Test assertions against real response
    let test_param = jsonpath_lib::select(&json, "$.args.test").unwrap();
    assert_eq!(test_param[0], serde_json::Value::String("hello".to_string()));
    
    let url = jsonpath_lib::select(&json, "$.url").unwrap();
    assert!(url[0].as_str().unwrap().contains("httpbin.org"));
}

#[tokio::test]
#[ignore] // Use --ignored to run network tests
async fn test_httpbin_auth_integration() {
    let client = reqwest::Client::new();
    
    // Test basic auth endpoint
    let response = client
        .get("https://httpbin.org/basic-auth/testuser/testpass")
        .basic_auth("testuser", Some("testpass"))
        .send()
        .await
        .expect("Failed to send request");
    
    assert_eq!(response.status(), 200);
    
    let body = response.text().await.expect("Failed to read response body");
    let json: serde_json::Value = serde_json::from_str(&body).expect("Failed to parse JSON");
    
    // Test authentication assertions
    let authenticated = jsonpath_lib::select(&json, "$.authenticated").unwrap();
    assert_eq!(authenticated[0], serde_json::Value::Bool(true));
    
    let user = jsonpath_lib::select(&json, "$.user").unwrap();
    assert_eq!(user[0], serde_json::Value::String("testuser".to_string()));
}

// Import the functions we're testing
use uptime_monitor::monitoring::{
    evaluate_predicate, values_equal, compare_values, string_predicate, 
    regex_predicate, is_uuid, is_ipv4, is_ipv6, is_iso_date,
    format_query, format_predicate, format_assertion_value
};