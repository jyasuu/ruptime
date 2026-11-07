use crate::config::{AppConfig, Check};
use crate::monitoring::checks::{
    check_dns, check_docker, check_elasticsearch, check_grpc, check_http_target, check_kafka,
    check_mongodb, check_mqtt, check_mysql, check_ntp, check_ping, check_postgres, check_push,
    check_rabbitmq, check_redis, check_smtp, check_snmp, check_tcp_port, check_websocket,
};
use crate::monitoring::types::{
    CheckResult, CheckStatus, HttpCheckResultDetails, ProtocolKind, ServiceCheckResult,
    TargetStatus,
};
use log::{debug, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

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
            let target_alias = match check.name() {
                Some(name) => name.to_string(),
                None => format!("{} ({})", host_alias, check.type_label()),
            };
            let (monitor_url, monitor_port) = check.monitor_target(&host_config.address);

            let mut status = TargetStatus::new(
                target_alias.clone(),
                monitor_url.clone(),
                host_config.address.clone(),
                monitor_port,
            );
            status.group = host_config.group.clone();
            status.tags = host_config.tags.clone();
            initial_statuses.push(status);
            let status_index = initial_statuses.len() - 1;

            if let Check::Push(p) = check {
                crate::monitoring::push::register_token(&p.token);
            }

            let app_cfg = Arc::clone(&app_config);
            let statuses = Arc::clone(&shared_statuses);
            let host_addr = host_config.address.clone();
            let check = check.clone();

            tokio::spawn(async move {
                loop {
                    let interval = app_cfg.monitoring_interval_seconds;
                    info!(
                        "Performing {} check for target: {} ({})",
                        check.type_name(),
                        target_alias,
                        monitor_url
                    );
                    let outcome = run_check(&check, &host_addr, interval).await;
                    record_outcome(
                        &statuses,
                        status_index,
                        &target_alias,
                        &host_addr,
                        check.type_name(),
                        outcome,
                    )
                    .await;
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

/// What one execution of a check produced, normalised so that every check type goes
/// through the same bookkeeping in [`record_outcome`].
struct Outcome {
    result: CheckResult,
    healthy: bool,
    /// Response time stored in the target's history. `None` leaves it unrecorded.
    history_ms: Option<u128>,
    /// Response time shown in logs.
    log_ms: u128,
    /// Failure reason; only set when unhealthy.
    error: Option<String>,
    /// `(days_remaining, is_valid)` for checks that probe a TLS certificate.
    cert: Option<(Option<i64>, Option<bool>)>,
}

impl Outcome {
    fn new(result: CheckResult, status: &CheckStatus, response_time_ms: u128) -> Self {
        let (healthy, error) = match status {
            CheckStatus::Healthy => (true, None),
            CheckStatus::Unhealthy(msg) => (false, Some(msg.clone())),
        };
        Self {
            result,
            healthy,
            history_ms: Some(response_time_ms),
            log_ms: response_time_ms,
            error,
            cert: None,
        }
    }

    /// Wraps a `ServiceCheckResult` in the `CheckResult` variant given by `wrap`.
    fn service(
        svc: ServiceCheckResult,
        wrap: impl FnOnce(ServiceCheckResult) -> CheckResult,
    ) -> Self {
        let (status, ms) = (svc.status.clone(), svc.response_time_ms);
        Self::new(wrap(svc), &status, ms)
    }
}

/// Runs a single check of any type.
async fn run_check(check: &Check, host: &str, interval_secs: u64) -> Outcome {
    match check {
        Check::Tcp(c) => {
            let r = check_tcp_port(host, c.port, Duration::from_secs(c.timeout_seconds)).await;
            let mut outcome =
                Outcome::new(CheckResult::Tcp(r.clone()), &r.status, r.response_time_ms);
            // TCP latency has never been written to the history; keep that behaviour.
            outcome.history_ms = None;
            outcome
        }
        Check::Http(c) => {
            let r = check_http_target(host, c).await;
            let mut outcome = Outcome::new(
                CheckResult::Http(HttpCheckResultDetails {
                    status: r.status.clone(),
                    response_time_ms: r.response_time_ms,
                    cert_days_remaining: r.cert_days_remaining,
                    cert_is_valid: r.cert_is_valid,
                }),
                &r.status,
                r.response_time_ms,
            );
            outcome.cert = Some((r.cert_days_remaining, r.cert_is_valid));
            outcome
        }
        Check::Postgres(c) => {
            Outcome::service(check_postgres(host, c).await, CheckResult::Postgres)
        }
        Check::Redis(c) => Outcome::service(check_redis(host, c).await, CheckResult::Redis),
        Check::RabbitMQ(c) => {
            Outcome::service(check_rabbitmq(host, c).await, CheckResult::RabbitMQ)
        }
        Check::Kafka(c) => Outcome::service(check_kafka(host, c).await, CheckResult::Kafka),
        Check::MySQL(c) => Outcome::service(check_mysql(host, c).await, CheckResult::MySQL),
        Check::MongoDB(c) => Outcome::service(check_mongodb(host, c).await, CheckResult::MongoDB),
        Check::Elasticsearch(c) => Outcome::service(
            check_elasticsearch(host, c).await,
            CheckResult::Elasticsearch,
        ),
        Check::Ping(c) => protocol(ProtocolKind::Ping, check_ping(host, c).await),
        Check::Dns(c) => protocol(ProtocolKind::Dns, check_dns(c).await),
        Check::Push(c) => protocol(ProtocolKind::Push, check_push(c, interval_secs)),
        Check::WebSocket(c) => protocol(ProtocolKind::WebSocket, check_websocket(host, c).await),
        Check::Smtp(c) => protocol(ProtocolKind::Smtp, check_smtp(host, c).await),
        Check::Ntp(c) => protocol(ProtocolKind::Ntp, check_ntp(host, c).await),
        Check::Mqtt(c) => protocol(ProtocolKind::Mqtt, check_mqtt(host, c).await),
        Check::Grpc(c) => protocol(ProtocolKind::Grpc, check_grpc(host, c).await),
        Check::Snmp(c) => protocol(ProtocolKind::Snmp, check_snmp(host, c).await),
        Check::Docker(c) => protocol(ProtocolKind::Docker, check_docker(c).await),
    }
}

fn protocol(kind: ProtocolKind, svc: ServiceCheckResult) -> Outcome {
    Outcome::service(svc, |r| CheckResult::Protocol(kind, r))
}

/// Stores an outcome on the target's shared status and logs state changes.
async fn record_outcome(
    shared_statuses: &Arc<Mutex<Vec<TargetStatus>>>,
    status_index: usize,
    alias: &str,
    host: &str,
    check_type: &str,
    outcome: Outcome,
) {
    if outcome.healthy {
        info!(
            "Target {} ({}) is healthy. Response time: {}ms ({})",
            alias, host, outcome.log_ms, check_type
        );
    }

    let mut statuses = shared_statuses.lock().await;
    let Some(entry) = statuses.get_mut(status_index) else {
        return;
    };

    entry.last_result = Some(outcome.result);
    if let Some((days_remaining, is_valid)) = outcome.cert {
        entry.cert_days_remaining = days_remaining;
        entry.cert_is_valid = is_valid;
    }

    let failures_before = entry.consecutive_failures;
    entry.add_check_result(outcome.healthy, outcome.history_ms, outcome.error.clone());

    if outcome.healthy {
        if failures_before > 0 {
            info!(
                "Target {} recovered after {} failures.",
                alias, failures_before
            );
        }
    } else {
        warn!(
            "Target {} UNHEALTHY. Reason: {}. Consecutive failures: {}. Type: {}",
            alias,
            outcome.error.as_deref().unwrap_or("unknown error"),
            entry.consecutive_failures,
            check_type
        );
    }
    debug!(
        "[{}] Healthy: {}, Consecutive Failures: {}",
        alias, entry.is_healthy, entry.consecutive_failures
    );
}
