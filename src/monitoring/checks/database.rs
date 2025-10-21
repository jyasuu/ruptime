use std::time::Duration;
use tokio::time::timeout;
use crate::config::{PostgresCheck, RedisCheck, RabbitMQCheck, KafkaCheck, MySQLCheck, MongoDBCheck, ElasticsearchCheck, PostgresSslMode};
use crate::monitoring::types::{ServiceCheckResult, CheckStatus};
use rdkafka::consumer::Consumer;
use mysql_async::prelude::*;

// PostgreSQL health check
pub async fn check_postgres(address: &str, config: &PostgresCheck) -> ServiceCheckResult {
    let start_time = std::time::Instant::now();
    
    let ssl_mode = match config.ssl_mode {
        PostgresSslMode::Disable => "disable",
        PostgresSslMode::Prefer => "prefer",
        PostgresSslMode::Require => "require",
    };
    
    let connection_string = format!(
        "host={} port={} user={} password={} dbname={} sslmode={}",
        address, config.port, config.username, config.password, config.database, ssl_mode
    );
    
    match timeout(
        Duration::from_secs(config.timeout_seconds),
        tokio_postgres::connect(&connection_string, tokio_postgres::NoTls)
    ).await {
        Ok(Ok((client, connection))) => {
            // Spawn the connection task
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("PostgreSQL connection error: {}", e);
                }
            });
            
            // Execute a simple query to verify connectivity
            match client.query_one("SELECT version()", &[]).await {
                Ok(row) => {
                    let version: String = row.get(0);
                    ServiceCheckResult {
                        status: CheckStatus::Healthy,
                        response_time_ms: start_time.elapsed().as_millis(),
                        service_info: Some(format!("PostgreSQL: {}", version.split_whitespace().take(2).collect::<Vec<_>>().join(" "))),
                    }
                }
                Err(e) => ServiceCheckResult {
                    status: CheckStatus::Unhealthy(format!("Query failed: {}", e)),
                    response_time_ms: start_time.elapsed().as_millis(),
                    service_info: None,
                }
            }
        }
        Ok(Err(e)) => ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!("Connection failed: {}", e)),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
        Err(_) => ServiceCheckResult {
            status: CheckStatus::Unhealthy("Connection timeout".to_string()),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        }
    }
}

// Redis health check
pub async fn check_redis(address: &str, config: &RedisCheck) -> ServiceCheckResult {
    let start_time = std::time::Instant::now();
    
    let connection_info = redis::ConnectionInfo {
        addr: redis::ConnectionAddr::Tcp(address.to_string(), config.port),
        redis: redis::RedisConnectionInfo {
            db: config.database as i64,
            username: None,
            password: config.password.clone(),
        },
    };
    
    match timeout(
        Duration::from_secs(config.timeout_seconds),
        async {
            let client = redis::Client::open(connection_info)?;
            let mut conn = client.get_async_connection().await?;
            let info: String = redis::cmd("INFO").arg("server").query_async(&mut conn).await?;
            Ok::<_, redis::RedisError>((conn, info))
        }
    ).await {
        Ok(Ok((_, info))) => {
            let version = info.lines()
                .find(|line| line.starts_with("redis_version:"))
                .map(|line| line.replace("redis_version:", ""))
                .unwrap_or_else(|| "unknown".to_string());
            
            ServiceCheckResult {
                status: CheckStatus::Healthy,
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: Some(format!("Redis v{}", version)),
            }
        }
        Ok(Err(e)) => ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!("Redis error: {}", e)),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
        Err(_) => ServiceCheckResult {
            status: CheckStatus::Unhealthy("Redis connection timeout".to_string()),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        }
    }
}

// RabbitMQ health check
pub async fn check_rabbitmq(address: &str, config: &RabbitMQCheck) -> ServiceCheckResult {
    let start_time = std::time::Instant::now();
    
    let protocol = if config.use_ssl { "https" } else { "http" };
    // RabbitMQ management port is typically port + 10000 (e.g., 5672 -> 15672)
    let management_port = config.port + 10000;
    let url = format!("{}://{}:{}/api/overview", protocol, address, management_port);
    
    let mut client_builder = reqwest::Client::builder();
    if config.use_ssl {
        // Accept self-signed certificates for monitoring purposes
        client_builder = client_builder.danger_accept_invalid_certs(true);
    }
    
    let client = match client_builder.build() {
        Ok(c) => c,
        Err(e) => {
            return ServiceCheckResult {
                status: CheckStatus::Unhealthy(format!("Failed to build HTTP client: {}", e)),
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: None,
            };
        }
    };
    
    let request_builder = client.get(&url)
        .basic_auth(&config.username, Some(&config.password))
        .timeout(Duration::from_secs(config.timeout_seconds));
    
    match timeout(
        Duration::from_secs(config.timeout_seconds),
        request_builder.send()
    ).await {
        Ok(Ok(response)) => {
            let status_code = response.status();
            if status_code.is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(json) => {
                        let version = json.get("rabbitmq_version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        ServiceCheckResult {
                            status: CheckStatus::Healthy,
                            response_time_ms: start_time.elapsed().as_millis(),
                            service_info: Some(format!("RabbitMQ v{}", version)),
                        }
                    }
                    Err(_) => ServiceCheckResult {
                        status: CheckStatus::Healthy,
                        response_time_ms: start_time.elapsed().as_millis(),
                        service_info: Some("RabbitMQ Management API responding".to_string()),
                    }
                }
            } else {
                ServiceCheckResult {
                    status: CheckStatus::Unhealthy(format!("RabbitMQ Management API returned status: {}", status_code)),
                    response_time_ms: start_time.elapsed().as_millis(),
                    service_info: None,
                }
            }
        }
        Ok(Err(e)) => ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!("RabbitMQ Management API request failed: {}", e)),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
        Err(_) => ServiceCheckResult {
            status: CheckStatus::Unhealthy("RabbitMQ Management API timeout".to_string()),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        }
    }
}

// Kafka health check
pub async fn check_kafka(address: &str, config: &KafkaCheck) -> ServiceCheckResult {
    let start_time = std::time::Instant::now();
    
    let mut client_config = rdkafka::ClientConfig::new();
    client_config.set("bootstrap.servers", &format!("{}:{}", address, config.port));
    client_config.set("message.timeout.ms", &(config.timeout_seconds * 1000).to_string());
    
    if config.use_ssl {
        client_config.set("security.protocol", "SSL");
    }
    
    match timeout(
        Duration::from_secs(config.timeout_seconds),
        async {
            let consumer: rdkafka::consumer::BaseConsumer = client_config.create()?;
            let metadata = consumer.fetch_metadata(config.topic.as_deref(), Duration::from_secs(5))?;
            Ok::<_, rdkafka::error::KafkaError>(metadata)
        }
    ).await {
        Ok(Ok(metadata)) => {
            let broker_count = metadata.brokers().len();
            let topic_count = metadata.topics().len();
            ServiceCheckResult {
                status: CheckStatus::Healthy,
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: Some(format!("Kafka: {} brokers, {} topics", broker_count, topic_count)),
            }
        }
        Ok(Err(e)) => ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!("Kafka error: {}", e)),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
        Err(_) => ServiceCheckResult {
            status: CheckStatus::Unhealthy("Kafka connection timeout".to_string()),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        }
    }
}

// MySQL health check
pub async fn check_mysql(address: &str, config: &MySQLCheck) -> ServiceCheckResult {
    let start_time = std::time::Instant::now();
    
    let opts = mysql_async::OptsBuilder::default()
        .ip_or_hostname(address)
        .tcp_port(config.port)
        .user(Some(&config.username))
        .pass(Some(&config.password))
        .db_name(Some(&config.database))
        .ssl_opts(if config.use_ssl {
            Some(mysql_async::SslOpts::default())
        } else {
            None
        });
    
    match timeout(
        Duration::from_secs(config.timeout_seconds),
        mysql_async::Conn::new(opts)
    ).await {
        Ok(Ok(mut conn)) => {
            match conn.query_first::<String, _>("SELECT VERSION()").await {
                Ok(Some(version)) => {
                    let _ = conn.disconnect().await;
                    ServiceCheckResult {
                        status: CheckStatus::Healthy,
                        response_time_ms: start_time.elapsed().as_millis(),
                        service_info: Some(format!("MySQL v{}", version.split('-').next().unwrap_or(&version))),
                    }
                }
                Ok(None) => ServiceCheckResult {
                    status: CheckStatus::Unhealthy("MySQL VERSION() query returned no result".to_string()),
                    response_time_ms: start_time.elapsed().as_millis(),
                    service_info: None,
                },
                Err(e) => ServiceCheckResult {
                    status: CheckStatus::Unhealthy(format!("MySQL query failed: {}", e)),
                    response_time_ms: start_time.elapsed().as_millis(),
                    service_info: None,
                }
            }
        }
        Ok(Err(e)) => ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!("MySQL connection failed: {}", e)),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
        Err(_) => ServiceCheckResult {
            status: CheckStatus::Unhealthy("MySQL connection timeout".to_string()),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        }
    }
}

// MongoDB health check
pub async fn check_mongodb(address: &str, config: &MongoDBCheck) -> ServiceCheckResult {
    let start_time = std::time::Instant::now();
    
    let mut uri = if config.use_ssl {
        format!("mongodb://{}:{}/{}?ssl=true", address, config.port, config.database)
    } else {
        format!("mongodb://{}:{}/{}", address, config.port, config.database)
    };
    
    if let (Some(username), Some(password)) = (&config.username, &config.password) {
        uri = uri.replace("mongodb://", &format!("mongodb://{}:{}@", username, password));
    }
    
    match timeout(
        Duration::from_secs(config.timeout_seconds),
        mongodb::Client::with_uri_str(&uri)
    ).await {
        Ok(Ok(client)) => {
            let db = client.database(&config.database);
            match db.run_command(mongodb::bson::doc! { "ping": 1 }, None).await {
                Ok(_) => {
                    // Try to get server info
                    match db.run_command(mongodb::bson::doc! { "buildInfo": 1 }, None).await {
                        Ok(build_info) => {
                            let version = build_info.get_str("version").unwrap_or("unknown");
                            ServiceCheckResult {
                                status: CheckStatus::Healthy,
                                response_time_ms: start_time.elapsed().as_millis(),
                                service_info: Some(format!("MongoDB v{}", version)),
                            }
                        }
                        Err(_) => ServiceCheckResult {
                            status: CheckStatus::Healthy,
                            response_time_ms: start_time.elapsed().as_millis(),
                            service_info: Some("MongoDB connection successful".to_string()),
                        }
                    }
                }
                Err(e) => ServiceCheckResult {
                    status: CheckStatus::Unhealthy(format!("MongoDB ping failed: {}", e)),
                    response_time_ms: start_time.elapsed().as_millis(),
                    service_info: None,
                }
            }
        }
        Ok(Err(e)) => ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!("MongoDB connection failed: {}", e)),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
        Err(_) => ServiceCheckResult {
            status: CheckStatus::Unhealthy("MongoDB connection timeout".to_string()),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        }
    }
}

// Elasticsearch health check using HTTP client
pub async fn check_elasticsearch(address: &str, config: &ElasticsearchCheck) -> ServiceCheckResult {
    let start_time = std::time::Instant::now();
    
    let protocol = if config.use_ssl { "https" } else { "http" };
    let url = format!("{}://{}:{}/_cluster/health", protocol, address, config.port);
    
    let mut client_builder = reqwest::Client::builder();
    if config.use_ssl {
        // Accept self-signed certificates for monitoring purposes
        client_builder = client_builder.danger_accept_invalid_certs(true);
    }
    
    let client = match client_builder.build() {
        Ok(c) => c,
        Err(e) => {
            return ServiceCheckResult {
                status: CheckStatus::Unhealthy(format!("Failed to build HTTP client: {}", e)),
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: None,
            };
        }
    };
    
    let mut request_builder = client.get(&url)
        .timeout(Duration::from_secs(config.timeout_seconds));
    
    // Add authentication if configured
    if let (Some(username), Some(password)) = (&config.username, &config.password) {
        request_builder = request_builder.basic_auth(username, Some(password));
    }
    
    match timeout(
        Duration::from_secs(config.timeout_seconds),
        request_builder.send()
    ).await {
        Ok(Ok(response)) => {
            let status_code = response.status();
            if status_code.is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(json) => {
                        let cluster_name = json.get("cluster_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let status = json.get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        
                        ServiceCheckResult {
                            status: CheckStatus::Healthy,
                            response_time_ms: start_time.elapsed().as_millis(),
                            service_info: Some(format!("Elasticsearch cluster '{}' status: {}", cluster_name, status)),
                        }
                    }
                    Err(_) => ServiceCheckResult {
                        status: CheckStatus::Healthy,
                        response_time_ms: start_time.elapsed().as_millis(),
                        service_info: Some("Elasticsearch cluster responding".to_string()),
                    }
                }
            } else {
                ServiceCheckResult {
                    status: CheckStatus::Unhealthy(format!("Elasticsearch returned status: {}", status_code)),
                    response_time_ms: start_time.elapsed().as_millis(),
                    service_info: None,
                }
            }
        }
        Ok(Err(e)) => ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!("Elasticsearch request failed: {}", e)),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
        Err(_) => ServiceCheckResult {
            status: CheckStatus::Unhealthy("Elasticsearch connection timeout".to_string()),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        }
    }
}