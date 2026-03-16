use crate::config::{
    AssertionPredicate, AssertionQuery, AssertionValue, CertificateField, HttpAssertion,
};
use crate::monitoring::types::AssertionResult;
use http::{HeaderMap, StatusCode};
use jsonpath_lib as jsonpath;
use regex::Regex;

// Function to evaluate HTTP response assertions with separate data
pub fn evaluate_assertions_with_data(
    assertions: &[HttpAssertion],
    response_status: StatusCode,
    response_headers: &HeaderMap,
    response_body: &str,
    response_time_ms: u128,
    _cert_info: Option<()>, // placeholder — cert assertions return None
) -> Vec<AssertionResult> {
    assertions
        .iter()
        .map(|assertion| {
            evaluate_single_assertion_with_data(
                assertion,
                response_status,
                response_headers,
                response_body,
                response_time_ms,
            )
        })
        .collect()
}

fn evaluate_single_assertion_with_data(
    assertion: &HttpAssertion,
    response_status: StatusCode,
    response_headers: &HeaderMap,
    response_body: &str,
    response_time_ms: u128,
) -> AssertionResult {
    let query_result = match &assertion.query {
        AssertionQuery::Status => Some(serde_json::Value::Number(serde_json::Number::from(
            response_status.as_u16(),
        ))),
        AssertionQuery::Header { name } => response_headers
            .get(name.as_str())
            .and_then(|v: &http::HeaderValue| v.to_str().ok())
            .map(|s: &str| serde_json::Value::String(s.to_string())),
        AssertionQuery::Body => Some(serde_json::Value::String(response_body.to_string())),
        AssertionQuery::JsonPath { path } => {
            match serde_json::from_str::<serde_json::Value>(response_body) {
                Ok(json) => match jsonpath::select(&json, path) {
                    Ok(results) => {
                        if results.is_empty() {
                            None
                        } else if results.len() == 1 {
                            Some(results[0].clone())
                        } else {
                            Some(serde_json::Value::Array(
                                results.into_iter().cloned().collect(),
                            ))
                        }
                    }
                    Err(_) => None,
                },
                Err(_) => None,
            }
        }
        AssertionQuery::Regex { pattern } => match Regex::new(pattern) {
            Ok(re) => re
                .captures(response_body)
                .and_then(|c| c.get(0))
                .map(|m| serde_json::Value::String(m.as_str().to_string())),
            Err(_) => None,
        },
        AssertionQuery::Cookie { name } => response_headers
            .get("set-cookie")
            .and_then(|v: &http::HeaderValue| v.to_str().ok())
            .and_then(|cookies: &str| {
                cookies
                    .split(';')
                    .find(|cookie: &&str| cookie.trim().starts_with(&format!("{}=", name)))
                    .and_then(|cookie: &str| cookie.split('=').nth(1))
                    .map(|value: &str| serde_json::Value::String(value.trim().to_string()))
            }),
        AssertionQuery::Duration => Some(serde_json::Value::Number(serde_json::Number::from(
            response_time_ms as u64,
        ))),
        // Certificate assertions require openssl X509 data; not available in the raw-TCP path.
        AssertionQuery::Certificate { field: _ } => None,
        AssertionQuery::XPath { path: _ } => None,
    };

    let query_str = format_query(&assertion.query);
    let predicate_str = format_predicate(&assertion.predicate);

    match query_result {
        Some(actual_value) => {
            let passed = evaluate_predicate(&assertion.predicate, &actual_value, &assertion.value);
            let expected_str = format_assertion_value(&assertion.value);
            let actual_str = format_json_value(&actual_value);

            AssertionResult {
                query: query_str.clone(),
                predicate: predicate_str.clone(),
                passed,
                message: if passed {
                    format!("{} {} {} ✓", query_str, predicate_str, expected_str)
                } else {
                    format!(
                        "{} {} {} (got: {})",
                        query_str, predicate_str, expected_str, actual_str
                    )
                },
                expected: expected_str,
                actual: Some(actual_str),
            }
        }
        None => {
            let passed = matches!(assertion.predicate, AssertionPredicate::NotExists);
            let expected_str = format_assertion_value(&assertion.value);

            AssertionResult {
                query: query_str.clone(),
                predicate: predicate_str.clone(),
                passed,
                message: if passed {
                    format!("{} {} ✓", query_str, predicate_str)
                } else {
                    format!("{} returned no value", query_str)
                },
                expected: expected_str,
                actual: None,
            }
        }
    }
}

fn evaluate_predicate(
    predicate: &AssertionPredicate,
    actual: &serde_json::Value,
    expected: &AssertionValue,
) -> bool {
    match predicate {
        AssertionPredicate::Equals => values_equal(actual, expected),
        AssertionPredicate::NotEquals => !values_equal(actual, expected),
        AssertionPredicate::GreaterThan => compare_values(actual, expected, |a, b| a > b),
        AssertionPredicate::GreaterThanOrEqual => compare_values(actual, expected, |a, b| a >= b),
        AssertionPredicate::LessThan => compare_values(actual, expected, |a, b| a < b),
        AssertionPredicate::LessThanOrEqual => compare_values(actual, expected, |a, b| a <= b),
        AssertionPredicate::StartsWith => {
            string_predicate(actual, expected, |a, b| a.starts_with(b))
        }
        AssertionPredicate::EndsWith => string_predicate(actual, expected, |a, b| a.ends_with(b)),
        AssertionPredicate::Contains => string_predicate(actual, expected, |a, b| a.contains(b)),
        AssertionPredicate::NotContains => {
            !string_predicate(actual, expected, |a, b| a.contains(b))
        }
        AssertionPredicate::Matches => regex_predicate(actual, expected),
        AssertionPredicate::NotMatches => !regex_predicate(actual, expected),
        AssertionPredicate::Exists => true,
        AssertionPredicate::NotExists => false,
        AssertionPredicate::IsBoolean => actual.is_boolean(),
        AssertionPredicate::IsNumber => actual.is_number(),
        AssertionPredicate::IsInteger => actual.is_i64() || actual.is_u64(),
        AssertionPredicate::IsFloat => actual.is_f64(),
        AssertionPredicate::IsString => actual.is_string(),
        AssertionPredicate::IsCollection => actual.is_array(),
        AssertionPredicate::IsEmpty => match actual {
            serde_json::Value::Array(arr) => arr.is_empty(),
            serde_json::Value::Object(obj) => obj.is_empty(),
            serde_json::Value::String(s) => s.is_empty(),
            _ => false,
        },
        AssertionPredicate::IsIsoDate => is_iso_date(actual),
        AssertionPredicate::IsIpv4 => is_ipv4(actual),
        AssertionPredicate::IsIpv6 => is_ipv6(actual),
        AssertionPredicate::IsUuid => is_uuid(actual),
    }
}

pub fn values_equal(actual: &serde_json::Value, expected: &AssertionValue) -> bool {
    match (actual, expected) {
        (serde_json::Value::String(a), AssertionValue::String(e)) => a == e,
        (serde_json::Value::Number(a), AssertionValue::Number(e)) => a.as_f64() == Some(*e),
        (serde_json::Value::Number(a), AssertionValue::Integer(e)) => a.as_i64() == Some(*e),
        (serde_json::Value::Bool(a), AssertionValue::Boolean(e)) => a == e,
        (serde_json::Value::Null, AssertionValue::Null) => true,
        _ => false,
    }
}

pub fn compare_values<F>(actual: &serde_json::Value, expected: &AssertionValue, op: F) -> bool
where
    F: Fn(f64, f64) -> bool,
{
    match (actual, expected) {
        (serde_json::Value::Number(a), AssertionValue::Number(e)) => {
            a.as_f64().map(|v| op(v, *e)).unwrap_or(false)
        }
        (serde_json::Value::Number(a), AssertionValue::Integer(e)) => {
            a.as_f64().map(|v| op(v, *e as f64)).unwrap_or(false)
        }
        _ => false,
    }
}

pub fn string_predicate<F>(actual: &serde_json::Value, expected: &AssertionValue, op: F) -> bool
where
    F: Fn(&str, &str) -> bool,
{
    match (actual, expected) {
        (serde_json::Value::String(a), AssertionValue::String(e)) => op(a, e),
        _ => false,
    }
}

pub fn regex_predicate(actual: &serde_json::Value, expected: &AssertionValue) -> bool {
    if let (serde_json::Value::String(actual_str), AssertionValue::String(pattern)) =
        (actual, expected)
    {
        Regex::new(pattern)
            .map(|re| re.is_match(actual_str))
            .unwrap_or(false)
    } else {
        false
    }
}

pub fn is_iso_date(value: &serde_json::Value) -> bool {
    if let serde_json::Value::String(s) = value {
        chrono::DateTime::parse_from_rfc3339(s).is_ok()
    } else {
        false
    }
}

pub fn is_ipv4(value: &serde_json::Value) -> bool {
    if let serde_json::Value::String(s) = value {
        s.parse::<std::net::Ipv4Addr>().is_ok()
    } else {
        false
    }
}

pub fn is_ipv6(value: &serde_json::Value) -> bool {
    if let serde_json::Value::String(s) = value {
        s.parse::<std::net::Ipv6Addr>().is_ok()
    } else {
        false
    }
}

pub fn is_uuid(value: &serde_json::Value) -> bool {
    if let serde_json::Value::String(s) = value {
        uuid::Uuid::parse_str(s).is_ok()
    } else {
        false
    }
}

pub fn format_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        _ => value.to_string(),
    }
}

pub fn format_query(query: &AssertionQuery) -> String {
    match query {
        AssertionQuery::Status => "status".to_string(),
        AssertionQuery::Header { name } => format!("header[{}]", name),
        AssertionQuery::Body => "body".to_string(),
        AssertionQuery::JsonPath { path } => format!("jsonpath[{}]", path),
        AssertionQuery::XPath { path } => format!("xpath[{}]", path),
        AssertionQuery::Regex { pattern } => format!("regex[{}]", pattern),
        AssertionQuery::Cookie { name } => format!("cookie[{}]", name),
        AssertionQuery::Duration => "duration".to_string(),
        AssertionQuery::Certificate { field } => format!("certificate[{:?}]", field),
    }
}

pub fn format_predicate(predicate: &AssertionPredicate) -> String {
    match predicate {
        AssertionPredicate::Equals => "==".to_string(),
        AssertionPredicate::NotEquals => "!=".to_string(),
        AssertionPredicate::GreaterThan => ">".to_string(),
        AssertionPredicate::GreaterThanOrEqual => ">=".to_string(),
        AssertionPredicate::LessThan => "<".to_string(),
        AssertionPredicate::LessThanOrEqual => "<=".to_string(),
        AssertionPredicate::StartsWith => "startsWith".to_string(),
        AssertionPredicate::EndsWith => "endsWith".to_string(),
        AssertionPredicate::Contains => "contains".to_string(),
        AssertionPredicate::NotContains => "notContains".to_string(),
        AssertionPredicate::Matches => "matches".to_string(),
        AssertionPredicate::NotMatches => "notMatches".to_string(),
        AssertionPredicate::Exists => "exists".to_string(),
        AssertionPredicate::NotExists => "notExists".to_string(),
        AssertionPredicate::IsBoolean => "isBoolean".to_string(),
        AssertionPredicate::IsNumber => "isNumber".to_string(),
        AssertionPredicate::IsInteger => "isInteger".to_string(),
        AssertionPredicate::IsFloat => "isFloat".to_string(),
        AssertionPredicate::IsString => "isString".to_string(),
        AssertionPredicate::IsCollection => "isCollection".to_string(),
        AssertionPredicate::IsEmpty => "isEmpty".to_string(),
        AssertionPredicate::IsIsoDate => "isIsoDate".to_string(),
        AssertionPredicate::IsIpv4 => "isIpv4".to_string(),
        AssertionPredicate::IsIpv6 => "isIpv6".to_string(),
        AssertionPredicate::IsUuid => "isUuid".to_string(),
    }
}

pub fn format_assertion_value(value: &AssertionValue) -> String {
    match value {
        AssertionValue::String(s) => format!("\"{}\"", s),
        AssertionValue::Number(n) => n.to_string(),
        AssertionValue::Integer(i) => i.to_string(),
        AssertionValue::Boolean(b) => b.to_string(),
        AssertionValue::Null => "null".to_string(),
    }
}
