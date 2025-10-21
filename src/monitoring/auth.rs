use std::time::{Duration, SystemTime};
use std::collections::HashMap;
use std::sync::Mutex;

// Simple in-memory cache for OAuth2 tokens
static OAUTH2_TOKEN_CACHE: std::sync::LazyLock<Mutex<HashMap<String, (String, SystemTime)>>> = 
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub async fn get_oauth2_token(client_id: &str, client_secret: &str, token_url: &str) -> Result<String, String> {
    let cache_key = format!("{}:{}", client_id, token_url);
    
    // Check cache first
    {
        let cache = OAUTH2_TOKEN_CACHE.lock().unwrap();
        if let Some((token, timestamp)) = cache.get(&cache_key) {
            // Check if token is still valid (assuming 3600 seconds expiry with 300 second buffer)
            if timestamp.elapsed().unwrap_or(Duration::from_secs(3900)) < Duration::from_secs(3300) {
                return Ok(token.clone());
            }
        }
    }
    
    // Get new token
    let client = reqwest::Client::new();
    let params = [
        ("grant_type", "client_credentials"),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];
    
    match client.post(token_url)
        .form(&params)
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(json) => {
                        if let Some(access_token) = json.get("access_token").and_then(|v| v.as_str()) {
                            // Cache the token
                            let mut cache = OAUTH2_TOKEN_CACHE.lock().unwrap();
                            cache.insert(cache_key, (access_token.to_string(), SystemTime::now()));
                            Ok(access_token.to_string())
                        } else {
                            Err("No access_token in OAuth2 response".to_string())
                        }
                    }
                    Err(e) => Err(format!("Failed to parse OAuth2 response: {}", e))
                }
            } else {
                Err(format!("OAuth2 token request failed with status: {}", response.status()))
            }
        }
        Err(e) => Err(format!("OAuth2 token request failed: {}", e))
    }
}