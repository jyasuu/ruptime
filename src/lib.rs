// Re-export modules for testing
pub mod config;
pub mod monitoring;
pub mod api;

// Test utilities
#[cfg(test)]
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