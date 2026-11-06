use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;

#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub hosts: Vec<HostConfig>,
    #[serde(default = "default_monitoring_interval")]
    pub monitoring_interval_seconds: u64,
    #[serde(default = "default_memory_cleanup_interval")]
    pub memory_cleanup_interval_minutes: u64,
    #[serde(default = "default_keep_history_hours")]
    pub keep_history_hours: u64,
    /// Default UI language for badges / API text (e.g. "en", "zh-TW", "ja").
    #[serde(default)]
    pub default_language: Option<String>,
    /// How often registered-domain expiry (RDAP) is refreshed.
    #[serde(default = "default_domain_expiry_interval_hours")]
    pub domain_expiry_check_interval_hours: u64,
}

fn default_domain_expiry_interval_hours() -> u64 {
    12
}

use log::error; // Added log macro

// Function to load configuration from a TOML file
pub fn load_config(file_path: &str) -> Result<AppConfig, Box<dyn Error>> {
    let contents = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to read configuration file '{}': {}", file_path, e);
            return Err(Box::new(e));
        }
    };
    match toml::from_str(&contents) {
        Ok(config) => Ok(config),
        Err(e) => {
            error!(
                "Failed to parse TOML from configuration file '{}': {}",
                file_path, e
            );
            Err(Box::new(e))
        }
    }
}

fn default_monitoring_interval() -> u64 {
    60
}

fn default_memory_cleanup_interval() -> u64 {
    60 // Clean up every hour
}

fn default_keep_history_hours() -> u64 {
    24 // Keep 24 hours of history
}

#[cfg(test)]
mod tests {
    use crate::config::*;
    use std::io::Write;
    use tempfile::NamedTempFile; // Using tempfile for easier management

    #[test]
    fn test_load_valid_config() {
        let valid_toml_content = r#"
monitoring_interval_seconds = 30

[[hosts]]
address = "example.com"
alias = "Example HTTP"
  [[hosts.checks]]
  type = "Http"
  port = 80
  path = "/"
  protocol = "Http"
  method = "Get"
  timeout_seconds = 5
  expected_status_code = 200

[[hosts]]
address = "1.1.1.1"
alias = "Cloudflare DNS"
  [[hosts.checks]]
  type = "Tcp"
  port = 53
  timeout_seconds = 2
"#;
        let mut tmp_file = NamedTempFile::new().unwrap();
        writeln!(tmp_file, "{}", valid_toml_content).unwrap();

        let loaded_config = load_config(tmp_file.path().to_str().unwrap());
        assert!(loaded_config.is_ok());
        let config = loaded_config.unwrap();

        assert_eq!(config.monitoring_interval_seconds, 30);
        assert_eq!(config.hosts.len(), 2);

        // Check first host (example.com)
        let host1 = &config.hosts[0];
        assert_eq!(host1.address, "example.com");
        assert_eq!(host1.alias, Some("Example HTTP".to_string()));
        assert_eq!(host1.checks.len(), 1);
        if let Check::Http(http_check) = &host1.checks[0] {
            assert_eq!(http_check.port, 80);
            assert_eq!(http_check.path, "/");
            assert_eq!(http_check.protocol, HttpProtocol::Http);
            assert_eq!(http_check.method, HttpMethod::Get);
            assert_eq!(http_check.timeout_seconds, 5);
            assert_eq!(http_check.expected_status_code, 200);
        } else {
            panic!("Expected Http check for first host");
        }

        // Check second host (1.1.1.1)
        let host2 = &config.hosts[1];
        assert_eq!(host2.address, "1.1.1.1");
        assert_eq!(host2.alias, Some("Cloudflare DNS".to_string()));
        assert_eq!(host2.checks.len(), 1);
        if let Check::Tcp(tcp_check) = &host2.checks[0] {
            assert_eq!(tcp_check.port, 53);
            assert_eq!(tcp_check.timeout_seconds, 2);
        } else {
            panic!("Expected Tcp check for second host");
        }
    }

    #[test]
    fn test_load_invalid_toml_format() {
        let invalid_toml_content = r#"
monitoring_interval_seconds = 30
[[hosts]]
address = "example.com"
alias = "Example HTTP"
  [[hosts.checks]] # Missing closing bracket for table array
  type = "Http"
  port = 80
"#;
        let mut tmp_file = NamedTempFile::new().unwrap();
        writeln!(tmp_file, "{}", invalid_toml_content).unwrap();

        let loaded_config = load_config(tmp_file.path().to_str().unwrap());
        assert!(loaded_config.is_err());
        let err_msg = loaded_config.err().unwrap().to_string();
        // Check if the error message indicates a TOML parsing issue
        // This can be specific, like "TOML parse error", "expected table key", etc.
        // For now, a general check for "parse" or "TOML"
        assert!(
            err_msg.to_lowercase().contains("toml") || err_msg.to_lowercase().contains("parse")
        );
    }

    #[test]
    fn test_load_config_missing_required_field() {
        // Missing 'address' for a host
        let toml_missing_address = r#"
[[hosts]]
alias = "Missing Address Host"
  [[hosts.checks]]
  type = "Tcp"
  port = 123
"#;
        let mut tmp_file_addr = NamedTempFile::new().unwrap();
        writeln!(tmp_file_addr, "{}", toml_missing_address).unwrap();
        let loaded_config_addr = load_config(tmp_file_addr.path().to_str().unwrap());
        assert!(loaded_config_addr.is_err());
        let err_msg_addr = loaded_config_addr.err().unwrap().to_string();
        assert!(err_msg_addr
            .to_lowercase()
            .contains("missing field `address`"));

        // Missing 'port' for a TCP check
        let toml_missing_port_tcp = r#"
[[hosts]]
address = "example.com"
  [[hosts.checks]]
  type = "Tcp"
  # port = 123 # Missing port
"#;
        let mut tmp_file_port_tcp = NamedTempFile::new().unwrap();
        writeln!(tmp_file_port_tcp, "{}", toml_missing_port_tcp).unwrap();
        let loaded_config_port_tcp = load_config(tmp_file_port_tcp.path().to_str().unwrap());
        assert!(loaded_config_port_tcp.is_err());
        let err_msg_port_tcp = loaded_config_port_tcp.err().unwrap().to_string();
        assert!(err_msg_port_tcp
            .to_lowercase()
            .contains("missing field `port`"));

        // Missing 'type' for a check
        let toml_missing_type = r#"
[[hosts]]
address = "example.com"
  [[hosts.checks]]
  # type = "Tcp" # Missing type
  port = 123
"#;
        let mut tmp_file_type = NamedTempFile::new().unwrap();
        writeln!(tmp_file_type, "{}", toml_missing_type).unwrap();
        let loaded_config_type = load_config(tmp_file_type.path().to_str().unwrap());
        assert!(loaded_config_type.is_err());
        // Serde's TOML error for missing tag is often "invalid type: map, expected a string"
        // or "missing field `type`" depending on how it's structured.
        // Let's check for "missing field `type`" or "invalid type"
        let err_msg_type_lower = loaded_config_type.err().unwrap().to_string().to_lowercase();
        assert!(
            err_msg_type_lower.contains("missing field `type`")
                || err_msg_type_lower.contains("invalid type")
        );
    }

    #[test]
    fn test_config_defaults() {
        let minimal_toml_content = r#"
# monitoring_interval_seconds is omitted, should use default_monitoring_interval (60)

[[hosts]]
address = "default.example.com"
# alias is omitted, should be None
  [[hosts.checks]]
  type = "Http"
  port = 80
  path = "/default"
  protocol = "Http"
  method = "Get"
  # timeout_seconds is omitted, should use default_http_timeout (10)
  # check_ssl_certificate is omitted, should use default_check_ssl_certificate (true)
  # expected_status_code is omitted, should use default_expected_status_code (200)
  # body_regex_check is omitted, should be None

[[hosts]]
address = "default.tcp.example.com"
  [[hosts.checks]]
  type = "Tcp"
  port = 22
  # timeout_seconds is omitted, should use default_tcp_timeout (5)
"#;
        let mut tmp_file = NamedTempFile::new().unwrap();
        writeln!(tmp_file, "{}", minimal_toml_content).unwrap();

        let loaded_config = load_config(tmp_file.path().to_str().unwrap());
        assert!(
            loaded_config.is_ok(),
            "Failed to load minimal config: {:?}",
            loaded_config.err()
        );
        let config = loaded_config.unwrap();

        assert_eq!(
            config.monitoring_interval_seconds, 60,
            "Default monitoring interval"
        ); // As per default_monitoring_interval()

        let http_host = &config.hosts[0];
        assert_eq!(http_host.address, "default.example.com");
        assert_eq!(http_host.alias, None, "Default alias");
        if let Check::Http(http_check) = &http_host.checks[0] {
            assert_eq!(http_check.timeout_seconds, 10, "Default HTTP timeout"); // As per default_http_timeout()
            assert!(http_check.check_ssl_certificate, "Default SSL check"); // As per default_check_ssl_certificate()
            assert_eq!(
                http_check.expected_status_code, 200,
                "Default expected status code"
            ); // As per default_expected_status_code()
            assert_eq!(
                http_check.body_regex_check, None,
                "Default body regex check"
            );
        } else {
            panic!("Expected Http check for default.example.com");
        }

        let tcp_host = &config.hosts[1];
        assert_eq!(tcp_host.address, "default.tcp.example.com");
        if let Check::Tcp(tcp_check) = &tcp_host.checks[0] {
            assert_eq!(tcp_check.timeout_seconds, 5, "Default TCP timeout"); // As per default_tcp_timeout()
        } else {
            panic!("Expected Tcp check for default.tcp.example.com");
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HostConfig {
    pub address: String,
    pub alias: Option<String>,
    pub checks: Vec<Check>,
    /// Logical group (Uptime Kuma "group" monitor). Group status is the
    /// aggregate of all checks that share this name.
    #[serde(default)]
    pub group: Option<String>,
    /// Free-form tags, exposed in `/api/monitors` and `monitor_info` metrics.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Look up registered-domain expiry via RDAP for this host.
    #[serde(default)]
    pub check_domain_expiry: bool,
    /// Override the registrable domain used for the RDAP lookup
    /// (default: derived from `address` using the public-suffix list).
    #[serde(default)]
    pub domain_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")] // Allows using a 'type' field in TOML to distinguish enum variants
pub enum Check {
    Tcp(TcpCheck),
    Http(HttpCheck),
    Postgres(PostgresCheck),
    Redis(RedisCheck),
    RabbitMQ(RabbitMQCheck),
    Kafka(KafkaCheck),
    MySQL(MySQLCheck),
    MongoDB(MongoDBCheck),
    Elasticsearch(ElasticsearchCheck),
    Ping(PingCheck),
    Dns(DnsCheck),
    Push(PushCheck),
    WebSocket(WebSocketCheck),
    Smtp(SmtpCheck),
    Ntp(NtpCheck),
    Mqtt(MqttCheck),
    Grpc(GrpcCheck),
    Snmp(SnmpCheck),
    Docker(DockerCheck),
}

// ─── Additional protocol checks ───────────────────────────────────────────────

/// ICMP ping (uses the system `ping` binary, like Uptime Kuma).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PingCheck {
    pub name: Option<String>,
    #[serde(default = "default_ping_count")]
    pub count: u32,
    #[serde(default = "default_ping_timeout")]
    pub timeout_seconds: u64,
}
fn default_ping_count() -> u32 {
    1
}
fn default_ping_timeout() -> u64 {
    5
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum DnsRecordType {
    A,
    AAAA,
    CNAME,
    MX,
    NS,
    TXT,
    SRV,
    CAA,
    PTR,
}

/// DNS record resolution, optionally against a specific resolver.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DnsCheck {
    pub name: Option<String>,
    /// Name to resolve (for PTR: the IP address).
    pub query: String,
    pub record_type: DnsRecordType,
    /// Resolver IP; when absent the system resolver is used.
    #[serde(default)]
    pub resolver: Option<String>,
    #[serde(default = "default_dns_port")]
    pub port: u16,
    /// If set, at least one returned record must contain this text.
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default = "default_ping_timeout")]
    pub timeout_seconds: u64,
}
fn default_dns_port() -> u16 {
    53
}

/// Passive monitor: the monitored job calls `GET|POST /api/push/{token}`.
/// The target is DOWN when no push arrived within `max_silence_seconds`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PushCheck {
    pub name: Option<String>,
    pub token: String,
    /// Defaults to 2x the global monitoring interval.
    #[serde(default)]
    pub max_silence_seconds: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WebSocketCheck {
    pub name: Option<String>,
    pub port: u16,
    #[serde(default = "default_ws_path")]
    pub path: String,
    #[serde(default)]
    pub use_tls: bool,
    #[serde(default)]
    pub ignore_tls_errors: bool,
    #[serde(default)]
    pub subprotocol: Option<String>,
    /// Optional text frame to send after the handshake.
    #[serde(default)]
    pub send_message: Option<String>,
    /// If set, a text frame containing this value must be received.
    #[serde(default)]
    pub expect_message: Option<String>,
    #[serde(default = "default_ping_timeout")]
    pub timeout_seconds: u64,
}
fn default_ws_path() -> String {
    "/".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SmtpCheck {
    pub name: Option<String>,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    /// If set, the 220 banner must contain this text.
    #[serde(default)]
    pub expect_banner: Option<String>,
    #[serde(default = "default_ping_timeout")]
    pub timeout_seconds: u64,
}
fn default_smtp_port() -> u16 {
    25
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NtpCheck {
    pub name: Option<String>,
    #[serde(default = "default_ntp_port")]
    pub port: u16,
    /// Fail when |local clock - server clock| exceeds this many milliseconds.
    #[serde(default)]
    pub max_offset_ms: Option<u64>,
    #[serde(default = "default_ping_timeout")]
    pub timeout_seconds: u64,
}
fn default_ntp_port() -> u16 {
    123
}

/// SNMP v2c GET of a single OID.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SnmpCheck {
    pub name: Option<String>,
    #[serde(default = "default_snmp_port")]
    pub port: u16,
    #[serde(default = "default_snmp_community")]
    pub community: String,
    /// Dotted OID; default is sysUpTime.0.
    #[serde(default = "default_snmp_oid")]
    pub oid: String,
    /// If set, the returned value (rendered as text) must contain this.
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default = "default_ping_timeout")]
    pub timeout_seconds: u64,
}
fn default_snmp_port() -> u16 {
    161
}
fn default_snmp_community() -> String {
    "public".to_string()
}
fn default_snmp_oid() -> String {
    "1.3.6.1.2.1.1.3.0".to_string()
}

/// Docker container state via the Engine API (the host `address` is not used).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DockerCheck {
    pub name: Option<String>,
    /// Container name or id.
    pub container: String,
    /// Unix socket path or `tcp://host:port`.
    #[serde(default = "default_docker_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_ping_timeout")]
    pub timeout_seconds: u64,
}
fn default_docker_endpoint() -> String {
    "/var/run/docker.sock".to_string()
}

/// gRPC health check (`grpc.health.v1.Health/Check`).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GrpcCheck {
    pub name: Option<String>,
    pub port: u16,
    /// Use TLS (h2 via ALPN). Without it, HTTP/2 cleartext (h2c) is used.
    #[serde(default)]
    pub use_tls: bool,
    #[serde(default)]
    pub ignore_tls_errors: bool,
    /// Service name passed to the health RPC; empty = overall server health.
    #[serde(default)]
    pub service: String,
    #[serde(default = "default_ping_timeout")]
    pub timeout_seconds: u64,
}

/// MQTT 3.1.1 broker check (CONNECT / CONNACK).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MqttCheck {
    pub name: Option<String>,
    #[serde(default = "default_mqtt_port")]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default = "default_ping_timeout")]
    pub timeout_seconds: u64,
}
fn default_mqtt_port() -> u16 {
    1883
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TcpCheck {
    pub name: Option<String>,
    pub port: u16,
    #[serde(default = "default_tcp_timeout")]
    pub timeout_seconds: u64,
}

fn default_tcp_timeout() -> u64 {
    5
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PostgresCheck {
    pub name: Option<String>,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    #[serde(default = "default_postgres_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_postgres_ssl")]
    pub ssl_mode: PostgresSslMode,
}

fn default_postgres_timeout() -> u64 {
    10
}

fn default_postgres_ssl() -> PostgresSslMode {
    PostgresSslMode::Prefer
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum PostgresSslMode {
    Disable,
    Prefer,
    Require,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RedisCheck {
    pub name: Option<String>,
    pub port: u16,
    #[serde(default = "default_redis_timeout")]
    pub timeout_seconds: u64,
    pub password: Option<String>,
    #[serde(default = "default_redis_database")]
    pub database: u32,
}

fn default_redis_timeout() -> u64 {
    5
}

fn default_redis_database() -> u32 {
    0
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RabbitMQCheck {
    pub name: Option<String>,
    pub port: u16,
    pub username: String,
    pub password: String,
    #[serde(default = "default_rabbitmq_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_rabbitmq_vhost")]
    pub vhost: String,
    #[serde(default = "default_rabbitmq_ssl")]
    pub use_ssl: bool,
}

fn default_rabbitmq_timeout() -> u64 {
    10
}

fn default_rabbitmq_vhost() -> String {
    "/".to_string()
}

fn default_rabbitmq_ssl() -> bool {
    false
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KafkaCheck {
    pub name: Option<String>,
    pub port: u16,
    #[serde(default = "default_kafka_timeout")]
    pub timeout_seconds: u64,
    pub topic: Option<String>, // Optional topic to check
    #[serde(default = "default_kafka_ssl")]
    pub use_ssl: bool,
}

fn default_kafka_timeout() -> u64 {
    10
}

fn default_kafka_ssl() -> bool {
    false
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MySQLCheck {
    pub name: Option<String>,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    #[serde(default = "default_mysql_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_mysql_ssl")]
    pub use_ssl: bool,
}

fn default_mysql_timeout() -> u64 {
    10
}

fn default_mysql_ssl() -> bool {
    false
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MongoDBCheck {
    pub name: Option<String>,
    pub port: u16,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default = "default_mongodb_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_mongodb_ssl")]
    pub use_ssl: bool,
}

fn default_mongodb_timeout() -> u64 {
    10
}

fn default_mongodb_ssl() -> bool {
    false
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ElasticsearchCheck {
    pub name: Option<String>,
    pub port: u16,
    #[serde(default = "default_elasticsearch_timeout")]
    pub timeout_seconds: u64,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default = "default_elasticsearch_ssl")]
    pub use_ssl: bool,
    #[serde(default = "default_elasticsearch_index")]
    pub index: Option<String>, // Optional index to check
}

fn default_elasticsearch_timeout() -> u64 {
    10
}

fn default_elasticsearch_ssl() -> bool {
    false
}

fn default_elasticsearch_index() -> Option<String> {
    None
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HttpCheck {
    pub name: Option<String>,
    pub port: u16,
    pub path: String,
    pub protocol: HttpProtocol,
    pub method: HttpMethod,
    #[serde(default = "default_http_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_check_ssl_certificate")]
    pub check_ssl_certificate: bool,
    #[serde(default = "default_expected_status_code")]
    pub expected_status_code: u16,
    pub body_regex_check: Option<String>,
    pub auth: Option<AuthConfig>, // Authentication configuration
    pub headers: Option<std::collections::HashMap<String, String>>, // Custom headers
    pub assertions: Option<Vec<HttpAssertion>>, // HTTP response assertions

    // ── Extended HTTP options ────────────────────────────────────────────────
    /// Request body for POST/PUT/etc.
    #[serde(default)]
    pub body: Option<String>,
    /// Response body must contain this keyword (Uptime Kuma "HTTP(s) - Keyword").
    #[serde(default)]
    pub keyword: Option<String>,
    /// Invert the keyword check: body must NOT contain the keyword.
    #[serde(default)]
    pub invert_keyword: bool,
    #[serde(default = "default_true")]
    pub keyword_case_sensitive: bool,
    /// Accepted status codes / ranges, e.g. ["200-299", "301"].
    /// When non-empty this replaces `expected_status_code`.
    #[serde(default)]
    pub accepted_status_codes: Vec<String>,
    #[serde(default = "default_true")]
    pub follow_redirects: bool,
    #[serde(default = "default_max_redirects")]
    pub max_redirects: usize,
    /// Skip TLS certificate verification (self-signed certs).
    #[serde(default)]
    pub ignore_tls_errors: bool,
    /// HTTP/HTTPS/SOCKS5 proxy.
    #[serde(default)]
    pub proxy: Option<ProxyConfig>,
    /// Extra CA bundle (PEM) to trust.
    #[serde(default)]
    pub ca_cert_path: Option<String>,
    /// Client certificate + private key in one PEM file (mutual TLS).
    #[serde(default)]
    pub client_identity_path: Option<String>,
    /// Flip the result: DOWN when the check passes, UP when it fails.
    #[serde(default)]
    pub upside_down: bool,
    #[serde(default)]
    pub user_agent: Option<String>,
}

impl Default for HttpCheck {
    /// Mirrors the serde defaults, so `..Default::default()` behaves exactly
    /// like omitting the fields in the TOML file.
    fn default() -> Self {
        HttpCheck {
            name: None,
            port: 80,
            path: "/".to_string(),
            protocol: HttpProtocol::default(),
            method: HttpMethod::default(),
            timeout_seconds: default_http_timeout(),
            check_ssl_certificate: default_check_ssl_certificate(),
            expected_status_code: default_expected_status_code(),
            body_regex_check: None,
            auth: None,
            headers: None,
            assertions: None,
            body: None,
            keyword: None,
            invert_keyword: false,
            keyword_case_sensitive: true,
            accepted_status_codes: Vec::new(),
            follow_redirects: true,
            max_redirects: default_max_redirects(),
            ignore_tls_errors: false,
            proxy: None,
            ca_cert_path: None,
            client_identity_path: None,
            upside_down: false,
            user_agent: None,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_max_redirects() -> usize {
    10
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProxyConfig {
    /// e.g. `http://proxy:3128` or `socks5://127.0.0.1:1080`
    pub url: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

fn default_http_timeout() -> u64 {
    10
}

fn default_check_ssl_certificate() -> bool {
    true
}

fn default_expected_status_code() -> u16 {
    200
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub enum HttpProtocol {
    #[default]
    Http,
    Https,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Head,
    Put,
    Delete,
    Options,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AuthConfig {
    Basic {
        username: String,
        password: String,
    },
    OAuth2 {
        client_id: String,
        client_secret: String,
        token_url: String,
    },
    Bearer {
        token: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HttpAssertion {
    pub query: AssertionQuery,
    pub predicate: AssertionPredicate,
    pub value: AssertionValue,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AssertionQuery {
    Status,
    Header { name: String },
    Body,
    JsonPath { path: String },
    XPath { path: String },
    Regex { pattern: String },
    Cookie { name: String },
    Duration,
    Certificate { field: CertificateField },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum CertificateField {
    Subject,
    Issuer,
    Serial,
    NotBefore,
    NotAfter,
    Algorithm,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AssertionPredicate {
    Equals,
    NotEquals,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    StartsWith,
    EndsWith,
    Contains,
    NotContains,
    Matches,
    NotMatches,
    Exists,
    NotExists,
    IsBoolean,
    IsNumber,
    IsInteger,
    IsFloat,
    IsString,
    IsCollection,
    IsEmpty,
    IsIsoDate,
    IsIpv4,
    IsIpv6,
    IsUuid,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AssertionValue {
    String(String),
    Number(f64),
    Integer(i64),
    Boolean(bool),
    Null,
}
