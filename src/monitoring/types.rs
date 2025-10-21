use std::time::SystemTime;
use chrono::{DateTime, Utc};
use serde::Serialize;

// --- Data structures for storing check status ---

#[derive(Debug, Clone, serde::Serialize)]
pub enum CheckResult {
    Tcp(TcpCheckResult),
    Http(HttpCheckResultDetails),
    Postgres(ServiceCheckResult),
    Redis(ServiceCheckResult),
    RabbitMQ(ServiceCheckResult),
    Kafka(ServiceCheckResult),
    MySQL(ServiceCheckResult),
    MongoDB(ServiceCheckResult),
    Elasticsearch(ServiceCheckResult),
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct ServiceCheckResult {
    pub status: CheckStatus,
    pub response_time_ms: u128,
    pub service_info: Option<String>, // Database version, cluster info, etc.
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum CheckStatus {
    Healthy,
    Unhealthy(String), // Contains error message for debugging
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetStatus {
    pub target_alias: String,
    #[serde(serialize_with = "serialize_system_time")]
    pub last_check_time: Option<SystemTime>,
    pub last_result: Option<CheckResult>,
    pub consecutive_failures: u32,
    pub is_healthy: bool,
    pub uptime_percentage_24h: f64,
    pub average_response_time_24h_ms: f64,
    pub monitor_url: String,
    pub monitor_hostname: String,
    pub monitor_port: u16,
    pub cert_days_remaining: Option<i64>,
    pub cert_is_valid: Option<bool>,
    pub check_history: Vec<HistoricalCheckResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoricalCheckResult {
    #[serde(serialize_with = "serialize_system_time_direct")]
    pub timestamp: SystemTime,
    pub is_healthy: bool,
    pub response_time_ms: Option<u128>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AssertionResult {
    pub query: String,
    pub predicate: String,
    pub passed: bool,
    pub message: String,
    pub expected: String,
    pub actual: Option<String>,
}

// Renamed from HttpCheckResult to avoid conflict with the enum variant
#[derive(Debug, Clone)] // Added Clone
pub struct HttpTargetCheckResult {
    pub status: CheckStatus, // Uses the new shared CheckStatus
    pub response_time_ms: u128,
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

// Helper for serializing SystemTime directly
fn serialize_system_time_direct<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::Serialize;
    let datetime: DateTime<Utc> = (*time).into();
    datetime.to_rfc3339().serialize(serializer)
}

impl TargetStatus {
    pub fn new(alias: String, monitor_url: String, monitor_hostname: String, monitor_port: u16) -> Self { // Ensure this is public if called from main
        TargetStatus {
            target_alias: alias, // Make fields public if accessed directly from api.rs
            last_check_time: None, // Make fields public
            last_result: None, // Make fields public
            consecutive_failures: 0, // Make fields public
            is_healthy: true, // Start with an optimistic state // Make fields public
            uptime_percentage_24h: 100.0, // Will be calculated from history // Make fields public
            average_response_time_24h_ms: 0.0, // Will be calculated from history // Make fields public
            monitor_url,
            monitor_hostname,
            monitor_port,
            cert_days_remaining: None,
            cert_is_valid: None,
            check_history: Vec::new(),
        }
    }

    // Update historical data and calculate metrics
    pub fn add_check_result(&mut self, is_healthy: bool, response_time_ms: Option<u128>, error_message: Option<String>) {
        let now = SystemTime::now();
        
        // Add new result to history
        self.check_history.push(HistoricalCheckResult {
            timestamp: now,
            is_healthy,
            response_time_ms,
            error_message,
        });
        
        // Keep only last 24 hours of data (assuming checks every 30 seconds = 2880 checks per day)
        const MAX_HISTORY: usize = 2880;
        if self.check_history.len() > MAX_HISTORY {
            self.check_history.drain(0..(self.check_history.len() - MAX_HISTORY));
        }
        
        // Calculate 24h metrics
        self.calculate_24h_metrics();
        
        // Update current status
        self.last_check_time = Some(now);
        self.is_healthy = is_healthy;
        
        if is_healthy {
            self.consecutive_failures = 0;
        } else {
            self.consecutive_failures += 1;
        }
    }
    
    fn calculate_24h_metrics(&mut self) {
        if self.check_history.is_empty() {
            return;
        }
        
        let now = SystemTime::now();
        let twenty_four_hours_ago = now - std::time::Duration::from_secs(24 * 60 * 60);
        
        // Filter to last 24 hours
        let recent_checks: Vec<&HistoricalCheckResult> = self.check_history
            .iter()
            .filter(|check| check.timestamp > twenty_four_hours_ago)
            .collect();
        
        if recent_checks.is_empty() {
            return;
        }
        
        // Calculate uptime percentage
        let healthy_count = recent_checks.iter().filter(|check| check.is_healthy).count();
        self.uptime_percentage_24h = (healthy_count as f64 / recent_checks.len() as f64) * 100.0;
        
        // Calculate average response time
        let response_times: Vec<u128> = recent_checks
            .iter()
            .filter_map(|check| check.response_time_ms)
            .collect();
        
        if !response_times.is_empty() {
            let sum: u128 = response_times.iter().sum();
            self.average_response_time_24h_ms = sum as f64 / response_times.len() as f64;
        }
    }
}