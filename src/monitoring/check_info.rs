//! Descriptive helpers on [`Check`]: display name, type label and monitor URL.
//!
//! These used to be three separate 19-arm `match` blocks inside the monitoring loop.

use crate::config::{Check, HttpProtocol};

impl Check {
    /// The user-configured name, if any.
    pub fn name(&self) -> Option<&str> {
        match self {
            Check::Tcp(c) => c.name.as_deref(),
            Check::Http(c) => c.name.as_deref(),
            Check::Postgres(c) => c.name.as_deref(),
            Check::Redis(c) => c.name.as_deref(),
            Check::RabbitMQ(c) => c.name.as_deref(),
            Check::Kafka(c) => c.name.as_deref(),
            Check::MySQL(c) => c.name.as_deref(),
            Check::MongoDB(c) => c.name.as_deref(),
            Check::Elasticsearch(c) => c.name.as_deref(),
            Check::Ping(c) => c.name.as_deref(),
            Check::Dns(c) => c.name.as_deref(),
            Check::Push(c) => c.name.as_deref(),
            Check::WebSocket(c) => c.name.as_deref(),
            Check::Smtp(c) => c.name.as_deref(),
            Check::Ntp(c) => c.name.as_deref(),
            Check::Mqtt(c) => c.name.as_deref(),
            Check::Grpc(c) => c.name.as_deref(),
            Check::Snmp(c) => c.name.as_deref(),
            Check::Docker(c) => c.name.as_deref(),
        }
    }

    /// Human-readable check type, used in logs.
    pub fn type_name(&self) -> &'static str {
        match self {
            Check::Tcp(_) => "TCP",
            Check::Http(_) => "HTTP",
            Check::Postgres(_) => "PostgreSQL",
            Check::Redis(_) => "Redis",
            Check::RabbitMQ(_) => "RabbitMQ",
            Check::Kafka(_) => "Kafka",
            Check::MySQL(_) => "MySQL",
            Check::MongoDB(_) => "MongoDB",
            Check::Elasticsearch(_) => "Elasticsearch",
            Check::Ping(_) => "Ping",
            Check::Dns(_) => "DNS",
            Check::Push(_) => "Push",
            Check::WebSocket(_) => "WebSocket",
            Check::Smtp(_) => "SMTP",
            Check::Ntp(_) => "NTP",
            Check::Mqtt(_) => "MQTT",
            Check::Grpc(_) => "gRPC",
            Check::Snmp(_) => "SNMP",
            Check::Docker(_) => "Docker",
        }
    }

    /// Short `Type:detail` label used to build a default alias for unnamed checks.
    pub fn type_label(&self) -> String {
        match self {
            Check::Tcp(c) => format!("TCP:{}", c.port),
            Check::Http(c) => format!("HTTP:{}", c.port),
            Check::Postgres(c) => format!("Postgres:{}", c.port),
            Check::Redis(c) => format!("Redis:{}", c.port),
            Check::RabbitMQ(c) => format!("RabbitMQ:{}", c.port),
            Check::Kafka(c) => format!("Kafka:{}", c.port),
            Check::MySQL(c) => format!("MySQL:{}", c.port),
            Check::MongoDB(c) => format!("MongoDB:{}", c.port),
            Check::Elasticsearch(c) => format!("Elasticsearch:{}", c.port),
            Check::Ping(_) => "Ping".to_string(),
            Check::Dns(c) => format!("DNS:{}", c.query),
            Check::Push(_) => "Push".to_string(), // never expose the token
            Check::WebSocket(c) => format!("WebSocket:{}", c.port),
            Check::Smtp(c) => format!("SMTP:{}", c.port),
            Check::Ntp(c) => format!("NTP:{}", c.port),
            Check::Mqtt(c) => format!("MQTT:{}", c.port),
            Check::Grpc(c) => format!("gRPC:{}", c.port),
            Check::Snmp(c) => format!("SNMP:{}", c.port),
            Check::Docker(c) => format!("Docker:{}", c.container),
        }
    }

    /// The monitor URL (shown in the API/metrics) and port (0 when not applicable)
    /// for this check against `address`.
    pub fn monitor_target(&self, address: &str) -> (String, u16) {
        match self {
            Check::Tcp(c) => (format!("tcp://{}:{}", address, c.port), c.port),
            Check::Http(c) => {
                let proto = match c.protocol {
                    HttpProtocol::Http => "http",
                    HttpProtocol::Https => "https",
                };
                (
                    format!("{}://{}:{}{}", proto, address, c.port, c.path),
                    c.port,
                )
            }
            Check::Postgres(c) => (
                format!("postgres://{}:{}/{}", address, c.port, c.database),
                c.port,
            ),
            Check::Redis(c) => (
                format!("redis://{}:{}/{}", address, c.port, c.database),
                c.port,
            ),
            Check::RabbitMQ(c) => (
                format!(
                    "{}://{}:{}/{}",
                    if c.use_ssl { "amqps" } else { "amqp" },
                    address,
                    c.port,
                    c.vhost
                ),
                c.port,
            ),
            Check::Kafka(c) => (format!("kafka://{}:{}", address, c.port), c.port),
            Check::MySQL(c) => (
                format!("mysql://{}:{}/{}", address, c.port, c.database),
                c.port,
            ),
            Check::MongoDB(c) => (
                format!("mongodb://{}:{}/{}", address, c.port, c.database),
                c.port,
            ),
            Check::Elasticsearch(c) => (
                format!(
                    "{}://{}:{}",
                    if c.use_ssl { "https" } else { "http" },
                    address,
                    c.port
                ),
                c.port,
            ),
            Check::Ping(_) => (format!("ping://{}", address), 0),
            Check::Dns(c) => (
                format!(
                    "dns://{}/{}?type={:?}",
                    c.resolver.as_deref().unwrap_or("system"),
                    c.query,
                    c.record_type
                ),
                c.port,
            ),
            Check::Push(_) => (format!("push://{}", address), 0),
            Check::WebSocket(c) => (
                format!(
                    "{}://{}:{}{}",
                    if c.use_tls { "wss" } else { "ws" },
                    address,
                    c.port,
                    c.path
                ),
                c.port,
            ),
            Check::Smtp(c) => (format!("smtp://{}:{}", address, c.port), c.port),
            Check::Ntp(c) => (format!("ntp://{}:{}", address, c.port), c.port),
            Check::Mqtt(c) => (format!("mqtt://{}:{}", address, c.port), c.port),
            Check::Snmp(c) => (format!("snmp://{}:{}/{}", address, c.port, c.oid), c.port),
            Check::Docker(c) => (format!("docker://{}/{}", c.endpoint, c.container), 0),
            Check::Grpc(c) => (
                format!(
                    "{}://{}:{}",
                    if c.use_tls { "grpcs" } else { "grpc" },
                    address,
                    c.port
                ),
                c.port,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_src: &str) -> Check {
        toml::from_str(toml_src).expect("valid check TOML")
    }

    #[test]
    fn tcp_target_label_and_name() {
        let c = parse("type = \"Tcp\"\nport = 22\n");
        assert_eq!(c.name(), None);
        assert_eq!(c.type_name(), "TCP");
        assert_eq!(c.type_label(), "TCP:22");
        assert_eq!(
            c.monitor_target("example.org"),
            ("tcp://example.org:22".into(), 22)
        );

        let named = parse("type = \"Tcp\"\nname = \"ssh\"\nport = 22\n");
        assert_eq!(named.name(), Some("ssh"));
    }

    #[test]
    fn http_target_includes_scheme_port_and_path() {
        let c = parse(
            "type = \"Http\"\nport = 8443\nprotocol = \"Https\"\npath = \"/health\"\nmethod = \"Get\"\n",
        );
        assert_eq!(c.type_label(), "HTTP:8443");
        assert_eq!(
            c.monitor_target("example.org"),
            ("https://example.org:8443/health".into(), 8443)
        );
    }

    #[test]
    fn push_label_never_contains_the_token() {
        let c = parse("type = \"Push\"\ntoken = \"super-secret-token\"\n");
        assert_eq!(c.type_label(), "Push");
        assert_eq!(
            c.monitor_target("example.org"),
            ("push://example.org".into(), 0)
        );
        assert!(!c.type_label().contains("secret"));
    }

    #[test]
    fn dns_target_uses_system_resolver_by_default() {
        let c = parse("type = \"Dns\"\nquery = \"example.org\"\nrecord_type = \"A\"\n");
        assert_eq!(c.type_label(), "DNS:example.org");
        let (url, _port) = c.monitor_target("ignored");
        assert!(url.starts_with("dns://system/example.org?type="), "{url}");
    }
}
