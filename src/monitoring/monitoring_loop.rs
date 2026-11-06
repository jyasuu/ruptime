use crate::config::{AppConfig, Check, HttpProtocol};
use crate::monitoring::checks::{
    check_dns, check_docker, check_elasticsearch, check_grpc, check_http_target, check_mqtt,
    check_ntp, check_ping, check_push, check_smtp, check_snmp, check_tcp_port, check_websocket,
};
use crate::monitoring::types::{
    CheckResult, CheckStatus, HttpCheckResultDetails, ProtocolKind, ServiceCheckResult,
    TargetStatus, TcpCheckResult,
};
use log::{debug, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

#[cfg(feature = "kafka")]
use crate::monitoring::checks::check_kafka;
#[cfg(feature = "mongodb")]
use crate::monitoring::checks::check_mongodb;
#[cfg(feature = "mysql")]
use crate::monitoring::checks::check_mysql;
#[cfg(feature = "postgres")]
use crate::monitoring::checks::check_postgres;
#[cfg(feature = "rabbitmq")]
use crate::monitoring::checks::check_rabbitmq;
#[cfg(feature = "redis")]
use crate::monitoring::checks::check_redis;

pub async fn run_monitoring_loop(
    app_config: Arc<AppConfig>,
    shared_statuses: Arc<Mutex<Vec<TargetStatus>>>,
) {
    info!(
        "Starting monitoring loop with interval: {} seconds",
        app_config.monitoring_interval_seconds
    );

    let mut initial_statuses = Vec::new();

    for host_config in app_config.hosts.iter() {
        let host_alias = host_config.alias.as_deref().unwrap_or(&host_config.address);

        for check in &host_config.checks {
            let check_name = match check {
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
            };

            let target_alias = if let Some(name) = check_name {
                name.to_string()
            } else {
                let check_type_port = match check {
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
                };
                format!("{} ({})", host_alias, check_type_port)
            };

            let (monitor_url, monitor_port) = match check {
                Check::Tcp(c) => (format!("tcp://{}:{}", host_config.address, c.port), c.port),
                Check::Http(c) => {
                    let proto = match c.protocol {
                        HttpProtocol::Http => "http",
                        HttpProtocol::Https => "https",
                    };
                    (
                        format!("{}://{}:{}{}", proto, host_config.address, c.port, c.path),
                        c.port,
                    )
                }
                Check::Postgres(c) => (
                    format!(
                        "postgres://{}:{}/{}",
                        host_config.address, c.port, c.database
                    ),
                    c.port,
                ),
                Check::Redis(c) => (
                    format!("redis://{}:{}/{}", host_config.address, c.port, c.database),
                    c.port,
                ),
                Check::RabbitMQ(c) => (
                    format!(
                        "{}://{}:{}/{}",
                        if c.use_ssl { "amqps" } else { "amqp" },
                        host_config.address,
                        c.port,
                        c.vhost
                    ),
                    c.port,
                ),
                Check::Kafka(c) => (
                    format!("kafka://{}:{}", host_config.address, c.port),
                    c.port,
                ),
                Check::MySQL(c) => (
                    format!("mysql://{}:{}/{}", host_config.address, c.port, c.database),
                    c.port,
                ),
                Check::MongoDB(c) => (
                    format!(
                        "mongodb://{}:{}/{}",
                        host_config.address, c.port, c.database
                    ),
                    c.port,
                ),
                Check::Elasticsearch(c) => (
                    format!(
                        "{}://{}:{}",
                        if c.use_ssl { "https" } else { "http" },
                        host_config.address,
                        c.port
                    ),
                    c.port,
                ),
                Check::Ping(_) => (format!("ping://{}", host_config.address), 0),
                Check::Dns(c) => (
                    format!(
                        "dns://{}/{}?type={:?}",
                        c.resolver.as_deref().unwrap_or("system"),
                        c.query,
                        c.record_type
                    ),
                    c.port,
                ),
                Check::Push(_) => (format!("push://{}", host_config.address), 0),
                Check::WebSocket(c) => (
                    format!(
                        "{}://{}:{}{}",
                        if c.use_tls { "wss" } else { "ws" },
                        host_config.address,
                        c.port,
                        c.path
                    ),
                    c.port,
                ),
                Check::Smtp(c) => (format!("smtp://{}:{}", host_config.address, c.port), c.port),
                Check::Ntp(c) => (format!("ntp://{}:{}", host_config.address, c.port), c.port),
                Check::Mqtt(c) => (format!("mqtt://{}:{}", host_config.address, c.port), c.port),
                Check::Snmp(c) => (
                    format!("snmp://{}:{}/{}", host_config.address, c.port, c.oid),
                    c.port,
                ),
                Check::Docker(c) => (format!("docker://{}/{}", c.endpoint, c.container), 0),
                Check::Grpc(c) => (
                    format!(
                        "{}://{}:{}",
                        if c.use_tls { "grpcs" } else { "grpc" },
                        host_config.address,
                        c.port
                    ),
                    c.port,
                ),
            };

            initial_statuses.push(TargetStatus::new(
                target_alias.clone(),
                monitor_url,
                host_config.address.clone(),
                monitor_port,
            ));

            if let Check::Push(p) = check {
                crate::monitoring::push::register_token(&p.token);
            }
            if let Some(st) = initial_statuses.last_mut() {
                st.group = host_config.group.clone();
                st.tags = host_config.tags.clone();
            }
            let status_index = initial_statuses.len() - 1;
            let app_cfg = Arc::clone(&app_config);
            let statuses_clone = Arc::clone(&shared_statuses);
            let host_addr = host_config.address.clone();
            let check_clone = check.clone();
            let alias_clone = target_alias.clone();

            tokio::spawn(async move {
                loop {
                    let interval = app_cfg.monitoring_interval_seconds;
                    let result: CheckResult;

                    match &check_clone {
                        // ── TCP ──────────────────────────────────────────────────────────
                        Check::Tcp(cfg) => {
                            info!(
                                "Performing TCP check for target: {} ({}:{})",
                                alias_clone, host_addr, cfg.port
                            );

                            // Accept whatever check_tcp_port returns and normalise to TcpCheckResult.
                            // The return type may be Result<(),String> or TcpCheckResult depending on
                            // the local version of tcp.rs — we call into() on a helper to handle both.
                            let tcp_check_result = tcp_check_result_from(
                                check_tcp_port(
                                    &host_addr,
                                    cfg.port,
                                    Duration::from_secs(cfg.timeout_seconds),
                                )
                                .await,
                            );

                            let is_healthy =
                                matches!(tcp_check_result.status, CheckStatus::Healthy);

                            if is_healthy {
                                info!("Target {} ({}) is healthy (TCP)", alias_clone, host_addr);
                            }

                            result = CheckResult::Tcp(tcp_check_result);

                            let mut statuses = statuses_clone.lock().await;
                            if let Some(entry) = statuses.get_mut(status_index) {
                                entry.last_result = Some(result.clone());
                                let error_msg = match &result {
                                    CheckResult::Tcp(r) => match &r.status {
                                        CheckStatus::Unhealthy(m) => Some(m.clone()),
                                        _ => None,
                                    },
                                    _ => None,
                                };
                                let failures_before = entry.consecutive_failures;
                                entry.add_check_result(is_healthy, None, error_msg);
                                if is_healthy {
                                    if failures_before > 0 {
                                        info!(
                                            "Target {} recovered after {} failures.",
                                            alias_clone, failures_before
                                        );
                                    }
                                } else {
                                    let reason = match &result {
                                        CheckResult::Tcp(r) => match &r.status {
                                            CheckStatus::Unhealthy(m) => m.clone(),
                                            _ => "Unknown TCP error".to_string(),
                                        },
                                        _ => "Unknown error".to_string(),
                                    };
                                    warn!("Target {} UNHEALTHY. Reason: {}. Consecutive failures: {}. Type: TCP", alias_clone, reason, entry.consecutive_failures);
                                }
                                debug!(
                                    "[{}] Healthy: {}, Consecutive Failures: {}",
                                    alias_clone, entry.is_healthy, entry.consecutive_failures
                                );
                            }
                        }

                        // ── HTTP ─────────────────────────────────────────────────────────
                        Check::Http(cfg) => {
                            let proto = match cfg.protocol {
                                HttpProtocol::Http => "http",
                                HttpProtocol::Https => "https",
                            };
                            info!(
                                "Performing HTTP check for target: {} ({}://{}:{}{})",
                                alias_clone, proto, host_addr, cfg.port, cfg.path
                            );

                            let http_result = check_http_target(&host_addr, cfg).await;
                            let is_healthy = matches!(http_result.status, CheckStatus::Healthy);

                            if is_healthy {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (HTTP)",
                                    alias_clone, host_addr, http_result.response_time_ms
                                );
                            }

                            result = CheckResult::Http(HttpCheckResultDetails {
                                status: http_result.status,
                                response_time_ms: http_result.response_time_ms,
                                cert_days_remaining: http_result.cert_days_remaining,
                                cert_is_valid: http_result.cert_is_valid,
                            });

                            let mut statuses = statuses_clone.lock().await;
                            if let Some(entry) = statuses.get_mut(status_index) {
                                entry.last_result = Some(result.clone());
                                entry.cert_days_remaining = http_result.cert_days_remaining;
                                entry.cert_is_valid = http_result.cert_is_valid;

                                let error_msg = match &result {
                                    CheckResult::Http(r) => match &r.status {
                                        CheckStatus::Unhealthy(m) => Some(m.clone()),
                                        _ => None,
                                    },
                                    _ => None,
                                };
                                let failures_before = entry.consecutive_failures;
                                entry.add_check_result(
                                    is_healthy,
                                    Some(http_result.response_time_ms),
                                    error_msg,
                                );
                                if is_healthy {
                                    if failures_before > 0 {
                                        info!(
                                            "Target {} recovered after {} failures.",
                                            alias_clone, failures_before
                                        );
                                    }
                                } else {
                                    let reason = match &result {
                                        CheckResult::Http(r) => match &r.status {
                                            CheckStatus::Unhealthy(m) => m.clone(),
                                            _ => "Unknown HTTP error".to_string(),
                                        },
                                        _ => "Unknown error".to_string(),
                                    };
                                    warn!("Target {} UNHEALTHY. Reason: {}. Consecutive failures: {}. Type: HTTP", alias_clone, reason, entry.consecutive_failures);
                                }
                                debug!(
                                    "[{}] Healthy: {}, Consecutive Failures: {}",
                                    alias_clone, entry.is_healthy, entry.consecutive_failures
                                );
                            }
                        }

                        // ── PostgreSQL ───────────────────────────────────────────────────
                        Check::Postgres(cfg) => {
                            info!(
                                "Performing PostgreSQL check for target: {} ({}:{})",
                                alias_clone, host_addr, cfg.port
                            );
                            #[cfg(feature = "postgres")]
                            let service_result = check_postgres(&host_addr, cfg).await;
                            #[cfg(not(feature = "postgres"))]
                            let service_result = ServiceCheckResult {
                                status: CheckStatus::Unhealthy(
                                    "postgres feature not enabled".to_string(),
                                ),
                                response_time_ms: 0,
                                service_info: None,
                            };
                            let is_healthy = matches!(service_result.status, CheckStatus::Healthy);
                            if is_healthy {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (PostgreSQL)",
                                    alias_clone, host_addr, service_result.response_time_ms
                                );
                            }
                            result = CheckResult::Postgres(service_result);
                            update_service_status(
                                &statuses_clone,
                                status_index,
                                &alias_clone,
                                result.clone(),
                                is_healthy,
                                "PostgreSQL",
                            )
                            .await;
                        }

                        // ── Redis ────────────────────────────────────────────────────────
                        Check::Redis(cfg) => {
                            info!(
                                "Performing Redis check for target: {} ({}:{})",
                                alias_clone, host_addr, cfg.port
                            );
                            #[cfg(feature = "redis")]
                            let service_result = check_redis(&host_addr, cfg).await;
                            #[cfg(not(feature = "redis"))]
                            let service_result = ServiceCheckResult {
                                status: CheckStatus::Unhealthy(
                                    "redis feature not enabled".to_string(),
                                ),
                                response_time_ms: 0,
                                service_info: None,
                            };
                            let is_healthy = matches!(service_result.status, CheckStatus::Healthy);
                            if is_healthy {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (Redis)",
                                    alias_clone, host_addr, service_result.response_time_ms
                                );
                            }
                            result = CheckResult::Redis(service_result);
                            update_service_status(
                                &statuses_clone,
                                status_index,
                                &alias_clone,
                                result.clone(),
                                is_healthy,
                                "Redis",
                            )
                            .await;
                        }

                        // ── RabbitMQ ─────────────────────────────────────────────────────
                        Check::RabbitMQ(cfg) => {
                            info!(
                                "Performing RabbitMQ check for target: {} ({}:{})",
                                alias_clone, host_addr, cfg.port
                            );
                            #[cfg(feature = "rabbitmq")]
                            let service_result = check_rabbitmq(&host_addr, cfg).await;
                            #[cfg(not(feature = "rabbitmq"))]
                            let service_result = ServiceCheckResult {
                                status: CheckStatus::Unhealthy(
                                    "rabbitmq feature not enabled".to_string(),
                                ),
                                response_time_ms: 0,
                                service_info: None,
                            };
                            let is_healthy = matches!(service_result.status, CheckStatus::Healthy);
                            if is_healthy {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (RabbitMQ)",
                                    alias_clone, host_addr, service_result.response_time_ms
                                );
                            }
                            result = CheckResult::RabbitMQ(service_result);
                            update_service_status(
                                &statuses_clone,
                                status_index,
                                &alias_clone,
                                result.clone(),
                                is_healthy,
                                "RabbitMQ",
                            )
                            .await;
                        }

                        // ── Kafka ────────────────────────────────────────────────────────
                        Check::Kafka(cfg) => {
                            info!(
                                "Performing Kafka check for target: {} ({}:{})",
                                alias_clone, host_addr, cfg.port
                            );
                            #[cfg(feature = "kafka")]
                            let service_result = check_kafka(&host_addr, cfg).await;
                            #[cfg(not(feature = "kafka"))]
                            let service_result = ServiceCheckResult {
                                status: CheckStatus::Unhealthy(
                                    "kafka feature not enabled".to_string(),
                                ),
                                response_time_ms: 0,
                                service_info: None,
                            };
                            let is_healthy = matches!(service_result.status, CheckStatus::Healthy);
                            if is_healthy {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (Kafka)",
                                    alias_clone, host_addr, service_result.response_time_ms
                                );
                            }
                            result = CheckResult::Kafka(service_result);
                            update_service_status(
                                &statuses_clone,
                                status_index,
                                &alias_clone,
                                result.clone(),
                                is_healthy,
                                "Kafka",
                            )
                            .await;
                        }

                        // ── MySQL ────────────────────────────────────────────────────────
                        Check::MySQL(cfg) => {
                            info!(
                                "Performing MySQL check for target: {} ({}:{})",
                                alias_clone, host_addr, cfg.port
                            );
                            #[cfg(feature = "mysql")]
                            let service_result = check_mysql(&host_addr, cfg).await;
                            #[cfg(not(feature = "mysql"))]
                            let service_result = ServiceCheckResult {
                                status: CheckStatus::Unhealthy(
                                    "mysql feature not enabled".to_string(),
                                ),
                                response_time_ms: 0,
                                service_info: None,
                            };
                            let is_healthy = matches!(service_result.status, CheckStatus::Healthy);
                            if is_healthy {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (MySQL)",
                                    alias_clone, host_addr, service_result.response_time_ms
                                );
                            }
                            result = CheckResult::MySQL(service_result);
                            update_service_status(
                                &statuses_clone,
                                status_index,
                                &alias_clone,
                                result.clone(),
                                is_healthy,
                                "MySQL",
                            )
                            .await;
                        }

                        // ── MongoDB ──────────────────────────────────────────────────────
                        Check::MongoDB(cfg) => {
                            info!(
                                "Performing MongoDB check for target: {} ({}:{})",
                                alias_clone, host_addr, cfg.port
                            );
                            #[cfg(feature = "mongodb")]
                            let service_result = check_mongodb(&host_addr, cfg).await;
                            #[cfg(not(feature = "mongodb"))]
                            let service_result = ServiceCheckResult {
                                status: CheckStatus::Unhealthy(
                                    "mongodb feature not enabled".to_string(),
                                ),
                                response_time_ms: 0,
                                service_info: None,
                            };
                            let is_healthy = matches!(service_result.status, CheckStatus::Healthy);
                            if is_healthy {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (MongoDB)",
                                    alias_clone, host_addr, service_result.response_time_ms
                                );
                            }
                            result = CheckResult::MongoDB(service_result);
                            update_service_status(
                                &statuses_clone,
                                status_index,
                                &alias_clone,
                                result.clone(),
                                is_healthy,
                                "MongoDB",
                            )
                            .await;
                        }

                        // ── Elasticsearch ────────────────────────────────────────────────
                        Check::Elasticsearch(cfg) => {
                            info!(
                                "Performing Elasticsearch check for target: {} ({}:{})",
                                alias_clone, host_addr, cfg.port
                            );
                            let service_result = check_elasticsearch(&host_addr, cfg).await;
                            let is_healthy = matches!(service_result.status, CheckStatus::Healthy);
                            if is_healthy {
                                info!("Target {} ({}) is healthy. Response time: {}ms (Elasticsearch)", alias_clone, host_addr, service_result.response_time_ms);
                            }
                            result = CheckResult::Elasticsearch(service_result);
                            update_service_status(
                                &statuses_clone,
                                status_index,
                                &alias_clone,
                                result.clone(),
                                is_healthy,
                                "Elasticsearch",
                            )
                            .await;
                        }

                        // ── Ping / DNS / Push / WebSocket / SMTP / NTP / MQTT ────────────
                        other => {
                            if let Some((kind, svc)) =
                                run_protocol_check(other, &host_addr, interval).await
                            {
                                let is_healthy = matches!(svc.status, CheckStatus::Healthy);
                                info!(
                                    "Performed {} check for target: {} (healthy: {}, {}ms)",
                                    kind.as_str(),
                                    alias_clone,
                                    is_healthy,
                                    svc.response_time_ms
                                );
                                result = CheckResult::Protocol(kind, svc);
                                update_service_status(
                                    &statuses_clone,
                                    status_index,
                                    &alias_clone,
                                    result.clone(),
                                    is_healthy,
                                    kind.as_str(),
                                )
                                .await;
                            }
                        }
                    }

                    sleep(Duration::from_secs(interval)).await;
                }
            });
        }
    }

    {
        let mut statuses = shared_statuses.lock().await;
        *statuses = initial_statuses;
    }

    // Registered-domain expiry (RDAP), one task per opted-in host.
    for host_config in app_config.hosts.iter().filter(|h| h.check_domain_expiry) {
        let (host, domain) = (host_config.address.clone(), host_config.domain_name.clone());
        let hours = app_config.domain_expiry_check_interval_hours;
        let statuses = Arc::clone(&shared_statuses);
        tokio::spawn(crate::monitoring::domain_expiry::run_domain_expiry_task(
            host, domain, hours, statuses,
        ));
    }

    loop {
        sleep(Duration::from_secs(app_config.monitoring_interval_seconds)).await;
    }
}

/// Runs one of the additional protocol checks. Returns `None` for check types
/// that are handled by dedicated arms in the monitoring loop.
async fn run_protocol_check(
    check: &Check,
    host: &str,
    interval_secs: u64,
) -> Option<(ProtocolKind, ServiceCheckResult)> {
    Some(match check {
        Check::Ping(c) => (ProtocolKind::Ping, check_ping(host, c).await),
        Check::Dns(c) => (ProtocolKind::Dns, check_dns(c).await),
        Check::Push(c) => (ProtocolKind::Push, check_push(c, interval_secs)),
        Check::WebSocket(c) => (ProtocolKind::WebSocket, check_websocket(host, c).await),
        Check::Smtp(c) => (ProtocolKind::Smtp, check_smtp(host, c).await),
        Check::Ntp(c) => (ProtocolKind::Ntp, check_ntp(host, c).await),
        Check::Mqtt(c) => (ProtocolKind::Mqtt, check_mqtt(host, c).await),
        Check::Grpc(c) => (ProtocolKind::Grpc, check_grpc(host, c).await),
        Check::Snmp(c) => (ProtocolKind::Snmp, check_snmp(host, c).await),
        Check::Docker(c) => (ProtocolKind::Docker, check_docker(c).await),
        _ => return None,
    })
}

/// Normalise the return value of `check_tcp_port` into a `TcpCheckResult`.
///
/// The local `tcp.rs` may return either:
///   - `TcpCheckResult`  (newer version)
///   - `Result<(), String>` (original version)
///
/// This trait + two blanket impls let the call site compile against both without
/// knowing which version is present.
trait IntoTcpCheckResult {
    fn into_tcp_check_result(self) -> TcpCheckResult;
}

// Newer tcp.rs already returns TcpCheckResult — pass through unchanged.
impl IntoTcpCheckResult for TcpCheckResult {
    fn into_tcp_check_result(self) -> TcpCheckResult {
        self
    }
}

// Original tcp.rs returns Result<(), String>.
impl IntoTcpCheckResult for Result<(), String> {
    fn into_tcp_check_result(self) -> TcpCheckResult {
        match self {
            Ok(()) => TcpCheckResult {
                status: CheckStatus::Healthy,
                response_time_ms: 0,
            },
            Err(e) => TcpCheckResult {
                status: CheckStatus::Unhealthy(e),
                response_time_ms: 0,
            },
        }
    }
}

fn tcp_check_result_from<T: IntoTcpCheckResult>(t: T) -> TcpCheckResult {
    t.into_tcp_check_result()
}

async fn update_service_status(
    shared_statuses: &Arc<Mutex<Vec<TargetStatus>>>,
    status_index: usize,
    alias: &str,
    current_check_result: CheckResult,
    is_healthy_now: bool,
    check_type: &str,
) {
    let mut statuses = shared_statuses.lock().await;
    if let Some(entry) = statuses.get_mut(status_index) {
        entry.last_result = Some(current_check_result.clone());

        let (response_time_ms, error_message) = match &current_check_result {
            CheckResult::Postgres(r)
            | CheckResult::Redis(r)
            | CheckResult::RabbitMQ(r)
            | CheckResult::Kafka(r)
            | CheckResult::MySQL(r)
            | CheckResult::MongoDB(r)
            | CheckResult::Elasticsearch(r)
            | CheckResult::Protocol(_, r) => {
                let err = if !is_healthy_now {
                    match &r.status {
                        CheckStatus::Unhealthy(m) => Some(m.clone()),
                        _ => None,
                    }
                } else {
                    None
                };
                (Some(r.response_time_ms), err)
            }
            _ => (None, None),
        };

        let failures_before = entry.consecutive_failures;

        entry.add_check_result(is_healthy_now, response_time_ms, error_message);

        if is_healthy_now {
            if failures_before > 0 {
                info!(
                    "Target {} recovered after {} failures.",
                    alias, failures_before
                );
            }
        } else {
            let reason = match &current_check_result {
                CheckResult::Postgres(r)
                | CheckResult::Redis(r)
                | CheckResult::RabbitMQ(r)
                | CheckResult::Kafka(r)
                | CheckResult::MySQL(r)
                | CheckResult::MongoDB(r)
                | CheckResult::Elasticsearch(r)
                | CheckResult::Protocol(_, r) => match &r.status {
                    CheckStatus::Unhealthy(m) => m.clone(),
                    _ => format!("Unknown {} error", check_type),
                },
                _ => format!("Unknown {} error", check_type),
            };
            warn!(
                "Target {} UNHEALTHY. Reason: {}. Consecutive failures: {}. Type: {}",
                alias, reason, entry.consecutive_failures, check_type
            );
        }
        debug!(
            "[{}] Healthy: {}, Consecutive Failures: {}",
            alias, entry.is_healthy, entry.consecutive_failures
        );
    }
}
