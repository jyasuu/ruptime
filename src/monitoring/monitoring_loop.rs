use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use log::{info, warn, debug};
use crate::config::{AppConfig, Check, HttpProtocol};
use crate::monitoring::types::{TargetStatus, CheckResult, TcpCheckResult, HttpCheckResultDetails, CheckStatus};
use crate::monitoring::checks::{check_tcp_port, check_http_target, check_postgres, check_redis, check_rabbitmq, check_kafka, check_mysql, check_mongodb, check_elasticsearch};

// --- Main Monitoring Loop ---

pub async fn run_monitoring_loop(
    app_config: Arc<AppConfig>,
    shared_statuses: Arc<Mutex<Vec<TargetStatus>>>,
) {
    info!(
        "Starting monitoring loop with interval: {} seconds",
        app_config.monitoring_interval_seconds
    );

    // Initialize statuses for all targets
    let mut initial_statuses = Vec::new();
    for host_config in app_config.hosts.iter() {
        let host_alias = host_config.alias.as_deref().unwrap_or(&host_config.address);
        for check in &host_config.checks {
            // Use check name if provided, otherwise fall back to host alias with check type
            let check_name = match check {
                Check::Tcp(tcp_check) => tcp_check.name.as_deref(),
                Check::Http(http_check) => http_check.name.as_deref(),
                Check::Postgres(postgres_check) => postgres_check.name.as_deref(),
                Check::Redis(redis_check) => redis_check.name.as_deref(),
                Check::RabbitMQ(rabbitmq_check) => rabbitmq_check.name.as_deref(),
                Check::Kafka(kafka_check) => kafka_check.name.as_deref(),
                Check::MySQL(mysql_check) => mysql_check.name.as_deref(),
                Check::MongoDB(mongodb_check) => mongodb_check.name.as_deref(),
                Check::Elasticsearch(es_check) => es_check.name.as_deref(),
            };
            
            let target_alias = if let Some(name) = check_name {
                name.to_string()
            } else {
                // Generate default name: "HostAlias (CheckType:Port)"
                let check_type_port = match check {
                    Check::Tcp(tcp_check) => format!("TCP:{}", tcp_check.port),
                    Check::Http(http_check) => format!("HTTP:{}", http_check.port),
                    Check::Postgres(postgres_check) => format!("Postgres:{}", postgres_check.port),
                    Check::Redis(redis_check) => format!("Redis:{}", redis_check.port),
                    Check::RabbitMQ(rabbitmq_check) => format!("RabbitMQ:{}", rabbitmq_check.port),
                    Check::Kafka(kafka_check) => format!("Kafka:{}", kafka_check.port),
                    Check::MySQL(mysql_check) => format!("MySQL:{}", mysql_check.port),
                    Check::MongoDB(mongodb_check) => format!("MongoDB:{}", mongodb_check.port),
                    Check::Elasticsearch(es_check) => format!("Elasticsearch:{}", es_check.port),
                };
                format!("{} ({})", host_alias, check_type_port)
            };
            let (monitor_url, monitor_port) = match check {
                Check::Tcp(tcp_check) => (
                    format!("tcp://{}:{}", host_config.address, tcp_check.port),
                    tcp_check.port,
                ),
                Check::Http(http_check) => {
                    let protocol_str = match http_check.protocol {
                        HttpProtocol::Http => "http",
                        HttpProtocol::Https => "https",
                    };
                    (
                        format!("{}://{}:{}{}", protocol_str, host_config.address, http_check.port, http_check.path),
                        http_check.port,
                    )
                },
                Check::Postgres(postgres_check) => (
                    format!("postgres://{}:{}/{}", host_config.address, postgres_check.port, postgres_check.database),
                    postgres_check.port,
                ),
                Check::Redis(redis_check) => (
                    format!("redis://{}:{}/{}", host_config.address, redis_check.port, redis_check.database),
                    redis_check.port,
                ),
                Check::RabbitMQ(rabbitmq_check) => (
                    format!("{}://{}:{}/{}", 
                        if rabbitmq_check.use_ssl { "amqps" } else { "amqp" },
                        host_config.address, rabbitmq_check.port, rabbitmq_check.vhost),
                    rabbitmq_check.port,
                ),
                Check::Kafka(kafka_check) => (
                    format!("kafka://{}:{}", host_config.address, kafka_check.port),
                    kafka_check.port,
                ),
                Check::MySQL(mysql_check) => (
                    format!("mysql://{}:{}/{}", host_config.address, mysql_check.port, mysql_check.database),
                    mysql_check.port,
                ),
                Check::MongoDB(mongodb_check) => (
                    format!("mongodb://{}:{}/{}", host_config.address, mongodb_check.port, mongodb_check.database),
                    mongodb_check.port,
                ),
                Check::Elasticsearch(es_check) => (
                    format!("{}://{}:{}", 
                        if es_check.use_ssl { "https" } else { "http" },
                        host_config.address, es_check.port),
                    es_check.port,
                ),
            };

            initial_statuses.push(TargetStatus::new(
                target_alias.clone(),
                monitor_url,
                host_config.address.clone(),
                monitor_port,
            ));

            let status_index = initial_statuses.len() - 1;
            let app_config_clone = Arc::clone(&app_config);
            let shared_statuses_clone = Arc::clone(&shared_statuses);
            let host_address_clone = host_config.address.clone();
            let check_clone = check.clone(); // Check enum should derive Clone
            let alias_clone = target_alias.clone();

            tokio::spawn(async move {
                loop {
                    let interval_seconds = app_config_clone.monitoring_interval_seconds;
                    let current_check_result: CheckResult;

                    // Use the already cloned alias

                    match &check_clone {
                        Check::Tcp(tcp_check) => {
                            let check_type_str = "TCP";
                            info!(
                                "Performing {} check for target: {} ({}:{})",
                                check_type_str, alias_clone, host_address_clone, tcp_check.port
                            );
                            let tcp_result = check_tcp_port(
                                &host_address_clone,
                                tcp_check.port,
                                Duration::from_secs(tcp_check.timeout_seconds),
                            )
                            .await;

                            let is_healthy_now = tcp_result.is_ok();

                            if is_healthy_now {
                                info!(
                                    "Target {} ({}) is healthy (TCP)",
                                    alias_clone, host_address_clone
                                );
                            }

                            current_check_result = CheckResult::Tcp(TcpCheckResult { result: tcp_result });

                            // Update shared state for TCP
                            {
                                let mut statuses = shared_statuses_clone.lock().await;
                                if let Some(status_entry) = statuses.get_mut(status_index) {
                                    status_entry.last_result = Some(current_check_result.clone());

                                    let error_message = if !is_healthy_now {
                                        match &current_check_result {
                                            CheckResult::Tcp(tcp_res) => tcp_res.result.as_ref().err().cloned(),
                                            _ => None,
                                        }
                                    } else {
                                        None
                                    };
                                    status_entry.add_check_result(is_healthy_now, None, error_message);

                                    if status_entry.is_healthy {
                                        if status_entry.consecutive_failures > 0 {
                                            info!("Target {} has recovered. Was unhealthy for {} checks.", alias_clone, status_entry.consecutive_failures);
                                        }
                                        status_entry.consecutive_failures = 0;
                                    } else {
                                        status_entry.consecutive_failures += 1;
                                        let _reason_str = match &current_check_result {
                                            CheckResult::Tcp(tcp_res) => tcp_res.result.as_ref().err().cloned().unwrap_or_else(|| "Unknown TCP error".to_string()),
                                            _ => "Unknown error".to_string(), // Should not happen here
                                        };
                                        warn!("Target {} is UNHEALTHY. Reason: {}. Consecutive failures: {}. Check type: TCP", alias_clone, _reason_str, status_entry.consecutive_failures);
                                    }
                                    debug!("[{}] Updated status. Healthy: {}, Consecutive Failures: {}", alias_clone, status_entry.is_healthy, status_entry.consecutive_failures);
                                }
                            }
                        }
                        Check::Http(http_check_config) => {
                            let check_type_str = "HTTP";
                            let protocol_str = match http_check_config.protocol { HttpProtocol::Http => "http", HttpProtocol::Https => "https" };
                            info!(
                                "Performing {} check for target: {} ({}://{}:{}{})",
                                check_type_str, alias_clone, protocol_str, host_address_clone, http_check_config.port, http_check_config.path
                            );
                            let http_result =
                                check_http_target(&host_address_clone, http_check_config).await;

                            let is_healthy_now = matches!(http_result.status, CheckStatus::Healthy);

                            if is_healthy_now {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (HTTP)",
                                    alias_clone, host_address_clone, http_result.response_time_ms
                                );
                            }
                            // Unhealthy reason is logged later if not healthy_now

                            current_check_result = CheckResult::Http(HttpCheckResultDetails {
                                status: http_result.status, // http_result.status is cloned here
                                response_time_ms: http_result.response_time_ms,
                                cert_days_remaining: http_result.cert_days_remaining,
                                cert_is_valid: http_result.cert_is_valid,
                            });

                            // Update shared state for HTTP
                            {
                                let mut statuses = shared_statuses_clone.lock().await;
                                if let Some(status_entry) = statuses.get_mut(status_index) {
                                    status_entry.last_result = Some(current_check_result.clone());
                                    status_entry.cert_days_remaining = http_result.cert_days_remaining;
                                    status_entry.cert_is_valid = http_result.cert_is_valid;

                                    let error_message = if !is_healthy_now {
                                        match &current_check_result {
                                            CheckResult::Http(http_res) => {
                                                match &http_res.status {
                                                    CheckStatus::Unhealthy(msg) => Some(msg.clone()),
                                                    _ => None,
                                                }
                                            },
                                            _ => None,
                                        }
                                    } else {
                                        None
                                    };

                                    status_entry.add_check_result(is_healthy_now, Some(http_result.response_time_ms), error_message);

                                    if status_entry.is_healthy {
                                        if status_entry.consecutive_failures > 0 {
                                            info!("Target {} has recovered. Was unhealthy for {} checks.", alias_clone, status_entry.consecutive_failures);
                                        }
                                        status_entry.consecutive_failures = 0;
                                    } else {
                                        status_entry.consecutive_failures += 1;
                                        let reason_str = match &current_check_result {
                                            CheckResult::Http(http_res) => {
                                                match &http_res.status {
                                                    CheckStatus::Unhealthy(msg) => msg.clone(),
                                                    _ => "Unknown HTTP error".to_string(),
                                                }
                                            },
                                            _ => "Unknown error".to_string(),
                                        };
                                        warn!("Target {} is UNHEALTHY. Reason: {}. Consecutive failures: {}. Check type: HTTP", alias_clone, reason_str, status_entry.consecutive_failures);
                                    }
                                    debug!("[{}] Updated status. Healthy: {}, Consecutive Failures: {}", alias_clone, status_entry.is_healthy, status_entry.consecutive_failures);
                                }
                            }
                        }
                        Check::Postgres(postgres_config) => {
                            let check_type_str = "PostgreSQL";
                            info!(
                                "Performing {} check for target: {} ({}:{})",
                                check_type_str, alias_clone, host_address_clone, postgres_config.port
                            );
                            let service_result = check_postgres(&host_address_clone, postgres_config).await;
                            let is_healthy_now = matches!(service_result.status, CheckStatus::Healthy);

                            if is_healthy_now {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (PostgreSQL)",
                                    alias_clone, host_address_clone, service_result.response_time_ms
                                );
                            }

                            current_check_result = CheckResult::Postgres(service_result);
                            update_service_status(&shared_statuses_clone, status_index, &alias_clone, current_check_result.clone(), is_healthy_now, check_type_str).await;
                        }
                        Check::Redis(redis_config) => {
                            let check_type_str = "Redis";
                            info!(
                                "Performing {} check for target: {} ({}:{})",
                                check_type_str, alias_clone, host_address_clone, redis_config.port
                            );
                            let service_result = check_redis(&host_address_clone, redis_config).await;
                            let is_healthy_now = matches!(service_result.status, CheckStatus::Healthy);

                            if is_healthy_now {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (Redis)",
                                    alias_clone, host_address_clone, service_result.response_time_ms
                                );
                            }

                            current_check_result = CheckResult::Redis(service_result);
                            update_service_status(&shared_statuses_clone, status_index, &alias_clone, current_check_result.clone(), is_healthy_now, check_type_str).await;
                        }
                        Check::RabbitMQ(rabbitmq_config) => {
                            let check_type_str = "RabbitMQ";
                            info!(
                                "Performing {} check for target: {} ({}:{})",
                                check_type_str, alias_clone, host_address_clone, rabbitmq_config.port
                            );
                            let service_result = check_rabbitmq(&host_address_clone, rabbitmq_config).await;
                            let is_healthy_now = matches!(service_result.status, CheckStatus::Healthy);

                            if is_healthy_now {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (RabbitMQ)",
                                    alias_clone, host_address_clone, service_result.response_time_ms
                                );
                            }

                            current_check_result = CheckResult::RabbitMQ(service_result);
                            update_service_status(&shared_statuses_clone, status_index, &alias_clone, current_check_result.clone(), is_healthy_now, check_type_str).await;
                        }
                        Check::Kafka(kafka_config) => {
                            let check_type_str = "Kafka";
                            info!(
                                "Performing {} check for target: {} ({}:{})",
                                check_type_str, alias_clone, host_address_clone, kafka_config.port
                            );
                            let service_result = check_kafka(&host_address_clone, kafka_config).await;
                            let is_healthy_now = matches!(service_result.status, CheckStatus::Healthy);

                            if is_healthy_now {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (Kafka)",
                                    alias_clone, host_address_clone, service_result.response_time_ms
                                );
                            }

                            current_check_result = CheckResult::Kafka(service_result);
                            update_service_status(&shared_statuses_clone, status_index, &alias_clone, current_check_result.clone(), is_healthy_now, check_type_str).await;
                        }
                        Check::MySQL(mysql_config) => {
                            let check_type_str = "MySQL";
                            info!(
                                "Performing {} check for target: {} ({}:{})",
                                check_type_str, alias_clone, host_address_clone, mysql_config.port
                            );
                            let service_result = check_mysql(&host_address_clone, mysql_config).await;
                            let is_healthy_now = matches!(service_result.status, CheckStatus::Healthy);

                            if is_healthy_now {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (MySQL)",
                                    alias_clone, host_address_clone, service_result.response_time_ms
                                );
                            }

                            current_check_result = CheckResult::MySQL(service_result);
                            update_service_status(&shared_statuses_clone, status_index, &alias_clone, current_check_result.clone(), is_healthy_now, check_type_str).await;
                        }
                        Check::MongoDB(mongodb_config) => {
                            let check_type_str = "MongoDB";
                            info!(
                                "Performing {} check for target: {} ({}:{})",
                                check_type_str, alias_clone, host_address_clone, mongodb_config.port
                            );
                            let service_result = check_mongodb(&host_address_clone, mongodb_config).await;
                            let is_healthy_now = matches!(service_result.status, CheckStatus::Healthy);

                            if is_healthy_now {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (MongoDB)",
                                    alias_clone, host_address_clone, service_result.response_time_ms
                                );
                            }

                            current_check_result = CheckResult::MongoDB(service_result);
                            update_service_status(&shared_statuses_clone, status_index, &alias_clone, current_check_result.clone(), is_healthy_now, check_type_str).await;
                        }
                        Check::Elasticsearch(es_config) => {
                            let check_type_str = "Elasticsearch";
                            info!(
                                "Performing {} check for target: {} ({}:{})",
                                check_type_str, alias_clone, host_address_clone, es_config.port
                            );
                            let service_result = check_elasticsearch(&host_address_clone, es_config).await;
                            let is_healthy_now = matches!(service_result.status, CheckStatus::Healthy);

                            if is_healthy_now {
                                info!(
                                    "Target {} ({}) is healthy. Response time: {}ms (Elasticsearch)",
                                    alias_clone, host_address_clone, service_result.response_time_ms
                                );
                            }

                            current_check_result = CheckResult::Elasticsearch(service_result);
                            update_service_status(&shared_statuses_clone, status_index, &alias_clone, current_check_result.clone(), is_healthy_now, check_type_str).await;
                        }
                    }

                    sleep(Duration::from_secs(interval_seconds)).await;
                }
            });
        }
    }

    // Initialize the shared statuses
    {
        let mut statuses = shared_statuses.lock().await;
        *statuses = initial_statuses;
    }

    // Keep the main loop alive
    loop {
        sleep(Duration::from_secs(app_config.monitoring_interval_seconds)).await;
    }
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
    if let Some(status_entry) = statuses.get_mut(status_index) {
        status_entry.last_result = Some(current_check_result.clone());

        let (response_time_ms, error_message) = match &current_check_result {
            CheckResult::Postgres(service_res) | 
            CheckResult::Redis(service_res) | 
            CheckResult::RabbitMQ(service_res) | 
            CheckResult::Kafka(service_res) | 
            CheckResult::MySQL(service_res) | 
            CheckResult::MongoDB(service_res) | 
            CheckResult::Elasticsearch(service_res) => {
                let error_msg = if !is_healthy_now {
                    match &service_res.status {
                        CheckStatus::Unhealthy(msg) => Some(msg.clone()),
                        _ => None,
                    }
                } else {
                    None
                };
                (Some(service_res.response_time_ms), error_msg)
            },
            _ => (None, None),
        };

        status_entry.add_check_result(is_healthy_now, response_time_ms, error_message);

        if status_entry.is_healthy {
            if status_entry.consecutive_failures > 0 {
                info!("Target {} has recovered. Was unhealthy for {} checks.", alias, status_entry.consecutive_failures);
            }
            status_entry.consecutive_failures = 0;
        } else {
            status_entry.consecutive_failures += 1;
            let reason_str = match &current_check_result {
                CheckResult::Postgres(service_res) | 
                CheckResult::Redis(service_res) | 
                CheckResult::RabbitMQ(service_res) | 
                CheckResult::Kafka(service_res) | 
                CheckResult::MySQL(service_res) | 
                CheckResult::MongoDB(service_res) | 
                CheckResult::Elasticsearch(service_res) => {
                    match &service_res.status {
                        CheckStatus::Unhealthy(msg) => msg.clone(),
                        _ => format!("Unknown {} error", check_type),
                    }
                },
                _ => format!("Unknown {} error", check_type),
            };
            warn!("Target {} is UNHEALTHY. Reason: {}. Consecutive failures: {}. Check type: {}", alias, reason_str, status_entry.consecutive_failures, check_type);
        }
        debug!("[{}] Updated status. Healthy: {}, Consecutive Failures: {}", alias, status_entry.is_healthy, status_entry.consecutive_failures);
    }
}