//! Registered-domain expiry via RDAP (https://rdap.org bootstraps to the right registry).
use crate::monitoring::types::TargetStatus;
use chrono::{DateTime, Utc};
use log::{info, warn};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Registrable domain (eTLD+1) of a host name; `None` for IP addresses / unknown suffixes.
pub fn registrable_domain(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_lowercase();
    if host.parse::<IpAddr>().is_ok() {
        return None;
    }
    psl::domain_str(&host).map(|d| d.to_string())
}

/// Extracts the "expiration" event date from an RDAP domain response.
pub fn parse_rdap_expiry(json: &serde_json::Value) -> Option<DateTime<Utc>> {
    json.get("events")?
        .as_array()?
        .iter()
        .find(|e| e.get("eventAction").and_then(|a| a.as_str()) == Some("expiration"))
        .and_then(|e| e.get("eventDate")?.as_str())
        .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
        .map(|d| d.with_timezone(&Utc))
}

pub async fn lookup_expiry_days(domain: &str) -> Result<i64, String> {
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(20))
        .user_agent("ruptime/1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("https://rdap.org/domain/{}", domain);
    let resp = client
        .get(&url)
        .header("Accept", "application/rdap+json, application/json")
        .send()
        .await
        .map_err(|e| format!("RDAP request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!(
            "RDAP lookup for {} returned HTTP {}",
            domain,
            resp.status()
        ));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Bad RDAP JSON: {}", e))?;
    let exp = parse_rdap_expiry(&json)
        .ok_or_else(|| format!("RDAP response for {} has no expiration event", domain))?;
    Ok((exp - Utc::now()).num_days())
}

/// Periodically refreshes `domain_expiry_days` for every status belonging to `host_addr`.
pub async fn run_domain_expiry_task(
    host_addr: String,
    domain_override: Option<String>,
    interval_hours: u64,
    statuses: Arc<Mutex<Vec<TargetStatus>>>,
) {
    let domain = match domain_override.or_else(|| registrable_domain(&host_addr)) {
        Some(d) => d,
        None => {
            warn!("Domain expiry: '{}' has no registrable domain (IP or unknown suffix); set `domain_name`", host_addr);
            return;
        }
    };
    // Let the monitoring loop publish its initial status vector first.
    tokio::time::sleep(Duration::from_secs(5)).await;
    let interval = Duration::from_secs(interval_hours.max(1) * 3600);
    loop {
        let wait = match lookup_expiry_days(&domain).await {
            Ok(days) => {
                info!("Domain {} expires in {} days", domain, days);
                let mut all = statuses.lock().await;
                for s in all.iter_mut().filter(|s| s.monitor_hostname == host_addr) {
                    s.domain_expiry_days = Some(days);
                }
                interval
            }
            Err(e) => {
                warn!("Domain expiry lookup for {} failed: {}", domain, e);
                interval.min(Duration::from_secs(3600))
            }
        };
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registrable_domains() {
        assert_eq!(
            registrable_domain("www.example.com").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            registrable_domain("api.shop.example.co.uk").as_deref(),
            Some("example.co.uk")
        );
        assert_eq!(
            registrable_domain("EXAMPLE.org.").as_deref(),
            Some("example.org")
        );
        assert_eq!(registrable_domain("192.168.1.10"), None);
        assert_eq!(registrable_domain("::1"), None);
    }

    #[test]
    fn rdap_expiry_parse() {
        let j = serde_json::json!({"events":[
            {"eventAction":"registration","eventDate":"2000-01-01T00:00:00Z"},
            {"eventAction":"expiration","eventDate":"2030-05-06T07:08:09Z"}]});
        let d = parse_rdap_expiry(&j).unwrap();
        assert_eq!(d.to_rfc3339(), "2030-05-06T07:08:09+00:00");
        assert!(parse_rdap_expiry(&serde_json::json!({"events":[]})).is_none());
    }
}
