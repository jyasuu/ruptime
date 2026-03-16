use crate::config::ElasticsearchCheck;
use crate::monitoring::types::{CheckStatus, ServiceCheckResult};

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

#[cfg(feature = "kafka")]
use crate::config::KafkaCheck;
#[cfg(feature = "mongodb")]
use crate::config::MongoDBCheck;
#[cfg(feature = "mysql")]
use crate::config::MySQLCheck;
#[cfg(feature = "rabbitmq")]
use crate::config::RabbitMQCheck;
#[cfg(feature = "redis")]
use crate::config::RedisCheck;
#[cfg(feature = "postgres")]
use crate::config::{PostgresCheck, PostgresSslMode};

// ─── Shared raw-HTTP helper ───────────────────────────────────────────────────

struct RawHttpResponse {
    status: u16,
    body: String,
}

fn raw_http_get(
    address: &str,
    port: u16,
    path: &str,
    use_ssl: bool,
    accept_invalid_certs: bool,
    basic_auth: Option<(&str, &str)>,
    timeout_secs: u64,
) -> Result<RawHttpResponse, String> {
    let addr_str = format!("{}:{}", address, port);
    let addrs: Vec<_> = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("DNS lookup failed: {}", e))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("No addresses resolved for {}", addr_str));
    }

    let connect_timeout = Duration::from_secs(timeout_secs);
    let tcp = TcpStream::connect_timeout(&addrs[0], connect_timeout)
        .map_err(|e| format!("TCP connect failed: {}", e))?;
    tcp.set_read_timeout(Some(connect_timeout))
        .map_err(|e| format!("set_read_timeout: {}", e))?;
    tcp.set_nodelay(true).ok();

    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: ruptime/1.0\r\nAccept: application/json\r\nConnection: close\r\n",
        path, address
    );
    if let Some((user, pass)) = basic_auth {
        use base64::Engine as _;
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", user, pass));
        request.push_str(&format!("Authorization: Basic {}\r\n", encoded));
    }
    request.push_str("\r\n");

    let full_response: Vec<u8> = if use_ssl {
        raw_tls_exchange(tcp, address, &request, accept_invalid_certs)?
    } else {
        let mut tcp = tcp;
        tcp.write_all(request.as_bytes())
            .map_err(|e| format!("Write failed: {}", e))?;
        let mut buf = Vec::with_capacity(32 * 1024);
        let mut tmp = [0u8; 4096];
        loop {
            let n = tcp.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        buf
    };

    let split = full_response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(full_response.len());
    let header_buf = &full_response[..split];
    let body_buf = if split + 4 <= full_response.len() {
        &full_response[split + 4..]
    } else {
        &[]
    };

    let status = String::from_utf8_lossy(header_buf)
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0u16);

    Ok(RawHttpResponse {
        status,
        body: String::from_utf8_lossy(body_buf).into_owned(),
    })
}

#[cfg(not(target_os = "windows"))]
fn raw_tls_exchange(
    tcp: TcpStream,
    host: &str,
    request: &str,
    accept_invalid: bool,
) -> Result<Vec<u8>, String> {
    use std::ffi::CString;
    use std::os::fd::FromRawFd;
    use std::os::raw::{c_int, c_long, c_void};
    use std::os::unix::io::IntoRawFd;

    const SSL_CTRL_SET_TLSEXT_HOSTNAME: c_int = 55;
    const SSL_CTRL_MODE: c_int = 33;
    const SSL_MODE_RELEASE_BUFFERS: c_long = 0x00000010;

    extern "C" {
        fn TLS_client_method() -> *const c_void;
        fn SSL_CTX_new(method: *const c_void) -> *mut c_void;
        fn SSL_CTX_free(ctx: *mut c_void);
        fn SSL_CTX_ctrl(ctx: *mut c_void, cmd: c_int, larg: c_long, parg: *mut c_void) -> c_long;
        fn SSL_new(ctx: *mut c_void) -> *mut c_void;
        fn SSL_free(ssl: *mut c_void);
        fn SSL_set_fd(ssl: *mut c_void, fd: c_int) -> c_int;
        fn SSL_connect(ssl: *mut c_void) -> c_int;
        fn SSL_read(ssl: *mut c_void, buf: *mut c_void, num: c_int) -> c_int;
        fn SSL_write(ssl: *mut c_void, buf: *const c_void, num: c_int) -> c_int;
        fn SSL_ctrl(ssl: *mut c_void, cmd: c_int, larg: c_long, parg: *mut c_void) -> c_long;
        fn SSL_CTX_set_verify(ctx: *mut c_void, mode: c_int, cb: *const c_void);
        fn SSL_CTX_set_default_verify_paths(ctx: *mut c_void) -> c_int;
        fn SSL_get_error(ssl: *const c_void, ret: c_int) -> c_int;
    }

    unsafe {
        let method = TLS_client_method();
        if method.is_null() {
            return Err("TLS_client_method failed".into());
        }
        let ctx = SSL_CTX_new(method);
        if ctx.is_null() {
            return Err("SSL_CTX_new failed".into());
        }
        SSL_CTX_set_default_verify_paths(ctx);
        SSL_CTX_set_verify(ctx, if accept_invalid { 0 } else { 1 }, std::ptr::null());
        SSL_CTX_ctrl(
            ctx,
            SSL_CTRL_MODE,
            SSL_MODE_RELEASE_BUFFERS,
            std::ptr::null_mut(),
        );

        let ssl = SSL_new(ctx);
        if ssl.is_null() {
            SSL_CTX_free(ctx);
            return Err("SSL_new failed".into());
        }

        let chost = CString::new(host).map_err(|e| e.to_string())?;
        SSL_ctrl(
            ssl,
            SSL_CTRL_SET_TLSEXT_HOSTNAME,
            0,
            chost.as_ptr() as *mut c_void,
        );

        let fd = tcp.into_raw_fd();
        SSL_set_fd(ssl, fd);

        let ret = SSL_connect(ssl);
        if ret != 1 {
            let err = SSL_get_error(ssl, ret);
            SSL_free(ssl);
            SSL_CTX_free(ctx);
            drop(std::os::unix::io::OwnedFd::from_raw_fd(fd));
            return Err(format!("SSL_connect failed: {}", err));
        }

        let data = request.as_bytes();
        let mut written = 0;
        while written < data.len() {
            let r = SSL_write(
                ssl,
                data[written..].as_ptr() as *const c_void,
                (data.len() - written) as c_int,
            );
            if r <= 0 {
                SSL_free(ssl);
                SSL_CTX_free(ctx);
                return Err("SSL_write failed".into());
            }
            written += r as usize;
        }

        let mut buf = Vec::with_capacity(32 * 1024);
        let mut tmp = [0u8; 4096];
        loop {
            let r = SSL_read(ssl, tmp.as_mut_ptr() as *mut c_void, tmp.len() as c_int);
            if r <= 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..r as usize]);
        }

        SSL_free(ssl);
        SSL_CTX_free(ctx);
        let _tcp_back: TcpStream = std::os::unix::io::FromRawFd::from_raw_fd(fd);
        Ok(buf)
    }
}

#[cfg(target_os = "windows")]
fn raw_tls_exchange(
    _tcp: TcpStream,
    _host: &str,
    _request: &str,
    _accept_invalid: bool,
) -> Result<Vec<u8>, String> {
    Err("HTTPS on Windows requires native-tls".into())
}

// ─── PostgreSQL ───────────────────────────────────────────────────────────────

#[cfg(feature = "postgres")]
pub async fn check_postgres(address: &str, config: &PostgresCheck) -> ServiceCheckResult {
    let start_time = Instant::now();

    let ssl_mode = match config.ssl_mode {
        PostgresSslMode::Disable => "disable",
        PostgresSslMode::Prefer => "prefer",
        PostgresSslMode::Require => "require",
    };
    let connection_string = format!(
        "host={} port={} user={} password={} dbname={} sslmode={}",
        address, config.port, config.username, config.password, config.database, ssl_mode
    );

    use tokio::time::timeout;
    match timeout(
        Duration::from_secs(config.timeout_seconds),
        tokio_postgres::connect(&connection_string, tokio_postgres::NoTls),
    )
    .await
    {
        Ok(Ok((client, connection))) => {
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("PostgreSQL connection error: {}", e);
                }
            });
            match client.query_one("SELECT version()", &[]).await {
                Ok(row) => {
                    let version: String = row.get(0);
                    ServiceCheckResult {
                        status: CheckStatus::Healthy,
                        response_time_ms: start_time.elapsed().as_millis(),
                        service_info: Some(format!(
                            "PostgreSQL: {}",
                            version
                                .split_whitespace()
                                .take(2)
                                .collect::<Vec<_>>()
                                .join(" ")
                        )),
                    }
                }
                Err(e) => ServiceCheckResult {
                    status: CheckStatus::Unhealthy(format!("Query failed: {}", e)),
                    response_time_ms: start_time.elapsed().as_millis(),
                    service_info: None,
                },
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
        },
    }
}

// ─── Redis ────────────────────────────────────────────────────────────────────

#[cfg(feature = "redis")]
pub async fn check_redis(address: &str, config: &RedisCheck) -> ServiceCheckResult {
    let start_time = Instant::now();

    let connection_info = redis::ConnectionInfo {
        addr: redis::ConnectionAddr::Tcp(address.to_string(), config.port),
        redis: redis::RedisConnectionInfo {
            db: config.database as i64,
            username: None,
            password: config.password.clone(),
            protocol: redis::ProtocolVersion::RESP2,
        },
    };

    use tokio::time::timeout;
    match timeout(Duration::from_secs(config.timeout_seconds), async {
        let client = redis::Client::open(connection_info)?;
        let mut conn = client.get_multiplexed_async_connection().await?;
        let info: String = redis::cmd("INFO")
            .arg("server")
            .query_async(&mut conn)
            .await?;
        Ok::<_, redis::RedisError>((conn, info))
    })
    .await
    {
        Ok(Ok((_, info))) => {
            let version = info
                .lines()
                .find(|l| l.starts_with("redis_version:"))
                .map(|l| l.replace("redis_version:", ""))
                .unwrap_or_else(|| "unknown".to_string());
            ServiceCheckResult {
                status: CheckStatus::Healthy,
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: Some(format!("Redis v{}", version.trim())),
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
        },
    }
}

// ─── RabbitMQ (raw HTTP) ──────────────────────────────────────────────────────

#[cfg(feature = "rabbitmq")]
pub async fn check_rabbitmq(address: &str, config: &RabbitMQCheck) -> ServiceCheckResult {
    let start_time = Instant::now();
    let management_port = config.port + 10000;

    let addr = address.to_string();
    let username = config.username.clone();
    let password = config.password.clone();
    let use_ssl = config.use_ssl;
    let timeout_secs = config.timeout_seconds;

    let result = tokio::task::spawn_blocking(move || {
        raw_http_get(
            &addr,
            management_port,
            "/api/overview",
            use_ssl,
            true,
            Some((&username, &password)),
            timeout_secs,
        )
    })
    .await;

    let response = match result {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            return ServiceCheckResult {
                status: CheckStatus::Unhealthy(format!("RabbitMQ request failed: {}", e)),
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: None,
            }
        }
        Err(e) => {
            return ServiceCheckResult {
                status: CheckStatus::Unhealthy(format!("RabbitMQ task failed: {}", e)),
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: None,
            }
        }
    };

    if response.status / 100 == 2 {
        let version = serde_json::from_str::<serde_json::Value>(&response.body)
            .ok()
            .and_then(|j| {
                j.get("rabbitmq_version")
                    .and_then(|v: &serde_json::Value| v.as_str())
                    .map(|s| s.to_string())
            });
        ServiceCheckResult {
            status: CheckStatus::Healthy,
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: Some(
                version
                    .map(|v| format!("RabbitMQ v{}", v))
                    .unwrap_or_else(|| "RabbitMQ Management API responding".to_string()),
            ),
        }
    } else {
        ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!(
                "RabbitMQ Management API returned status: {}",
                response.status
            )),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        }
    }
}

// ─── Kafka ────────────────────────────────────────────────────────────────────

#[cfg(feature = "kafka")]
use rdkafka::consumer::Consumer;
#[cfg(feature = "kafka")]
pub async fn check_kafka(address: &str, config: &KafkaCheck) -> ServiceCheckResult {
    let start_time = Instant::now();

    let mut client_config = rdkafka::ClientConfig::new();
    client_config.set("bootstrap.servers", &format!("{}:{}", address, config.port));
    client_config.set(
        "message.timeout.ms",
        &(config.timeout_seconds * 1000).to_string(),
    );
    if config.use_ssl {
        client_config.set("security.protocol", "SSL");
    }

    use tokio::time::timeout;
    match timeout(Duration::from_secs(config.timeout_seconds), async {
        let consumer: rdkafka::consumer::BaseConsumer = client_config.create()?;
        let metadata = consumer.fetch_metadata(config.topic.as_deref(), Duration::from_secs(5))?;
        Ok::<_, rdkafka::error::KafkaError>(metadata)
    })
    .await
    {
        Ok(Ok(m)) => ServiceCheckResult {
            status: CheckStatus::Healthy,
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: Some(format!(
                "Kafka: {} brokers, {} topics",
                m.brokers().len(),
                m.topics().len()
            )),
        },
        Ok(Err(e)) => ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!("Kafka error: {}", e)),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
        Err(_) => ServiceCheckResult {
            status: CheckStatus::Unhealthy("Kafka connection timeout".to_string()),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
    }
}

// ─── MySQL ────────────────────────────────────────────────────────────────────

#[cfg(feature = "mysql")]
use mysql_async::prelude::*;
#[cfg(feature = "mysql")]
pub async fn check_mysql(address: &str, config: &MySQLCheck) -> ServiceCheckResult {
    let start_time = Instant::now();

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

    use tokio::time::timeout;
    match timeout(
        Duration::from_secs(config.timeout_seconds),
        mysql_async::Conn::new(opts),
    )
    .await
    {
        Ok(Ok(mut conn)) => match conn.query_first::<String, _>("SELECT VERSION()").await {
            Ok(Some(v)) => {
                let _ = conn.disconnect().await;
                ServiceCheckResult {
                    status: CheckStatus::Healthy,
                    response_time_ms: start_time.elapsed().as_millis(),
                    service_info: Some(format!("MySQL v{}", v.split('-').next().unwrap_or(&v))),
                }
            }
            Ok(None) => ServiceCheckResult {
                status: CheckStatus::Unhealthy("MySQL VERSION() returned no result".to_string()),
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: None,
            },
            Err(e) => ServiceCheckResult {
                status: CheckStatus::Unhealthy(format!("MySQL query failed: {}", e)),
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: None,
            },
        },
        Ok(Err(e)) => ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!("MySQL connection failed: {}", e)),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
        Err(_) => ServiceCheckResult {
            status: CheckStatus::Unhealthy("MySQL connection timeout".to_string()),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        },
    }
}

// ─── MongoDB ──────────────────────────────────────────────────────────────────

#[cfg(feature = "mongodb")]
pub async fn check_mongodb(address: &str, config: &MongoDBCheck) -> ServiceCheckResult {
    let start_time = Instant::now();

    let mut uri = format!(
        "mongodb://{}:{}/{}{}",
        address,
        config.port,
        config.database,
        if config.use_ssl { "?ssl=true" } else { "" }
    );
    if let (Some(u), Some(p)) = (&config.username, &config.password) {
        uri = uri.replace("mongodb://", &format!("mongodb://{}:{}@", u, p));
    }

    use tokio::time::timeout;
    match timeout(
        Duration::from_secs(config.timeout_seconds),
        mongodb::Client::with_uri_str(&uri),
    )
    .await
    {
        Ok(Ok(client)) => {
            let db = client.database(&config.database);
            match db
                .run_command(mongodb::bson::doc! { "ping": 1 }, None)
                .await
            {
                Ok(_) => match db
                    .run_command(mongodb::bson::doc! { "buildInfo": 1 }, None)
                    .await
                {
                    Ok(info) => ServiceCheckResult {
                        status: CheckStatus::Healthy,
                        response_time_ms: start_time.elapsed().as_millis(),
                        service_info: Some(format!(
                            "MongoDB v{}",
                            info.get_str("version").unwrap_or("unknown")
                        )),
                    },
                    Err(_) => ServiceCheckResult {
                        status: CheckStatus::Healthy,
                        response_time_ms: start_time.elapsed().as_millis(),
                        service_info: Some("MongoDB connection successful".to_string()),
                    },
                },
                Err(e) => ServiceCheckResult {
                    status: CheckStatus::Unhealthy(format!("MongoDB ping failed: {}", e)),
                    response_time_ms: start_time.elapsed().as_millis(),
                    service_info: None,
                },
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
        },
    }
}

// ─── Elasticsearch (raw HTTP) ─────────────────────────────────────────────────

pub async fn check_elasticsearch(address: &str, config: &ElasticsearchCheck) -> ServiceCheckResult {
    let start_time = Instant::now();

    let addr = address.to_string();
    let port = config.port;
    let use_ssl = config.use_ssl;
    let username = config.username.clone();
    let password = config.password.clone();
    let timeout_secs = config.timeout_seconds;

    let result = tokio::task::spawn_blocking(move || {
        // Build auth tuple from owned strings so lifetimes work in the closure
        match (&username, &password) {
            (Some(u), Some(p)) => raw_http_get(
                &addr,
                port,
                "/_cluster/health",
                use_ssl,
                true,
                Some((u.as_str(), p.as_str())),
                timeout_secs,
            ),
            _ => raw_http_get(
                &addr,
                port,
                "/_cluster/health",
                use_ssl,
                true,
                None,
                timeout_secs,
            ),
        }
    })
    .await;

    let response = match result {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            return ServiceCheckResult {
                status: CheckStatus::Unhealthy(format!("Elasticsearch request failed: {}", e)),
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: None,
            }
        }
        Err(e) => {
            return ServiceCheckResult {
                status: CheckStatus::Unhealthy(format!("Elasticsearch task failed: {}", e)),
                response_time_ms: start_time.elapsed().as_millis(),
                service_info: None,
            }
        }
    };

    if response.status / 100 == 2 {
        let service_info = serde_json::from_str::<serde_json::Value>(&response.body)
            .ok()
            .map(|j| {
                format!(
                    "Elasticsearch cluster '{}' status: {}",
                    j.get("cluster_name")
                        .and_then(|v: &serde_json::Value| v.as_str())
                        .unwrap_or("unknown"),
                    j.get("status")
                        .and_then(|v: &serde_json::Value| v.as_str())
                        .unwrap_or("unknown"),
                )
            })
            .unwrap_or_else(|| "Elasticsearch cluster responding".to_string());
        ServiceCheckResult {
            status: CheckStatus::Healthy,
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: Some(service_info),
        }
    } else {
        ServiceCheckResult {
            status: CheckStatus::Unhealthy(format!(
                "Elasticsearch returned status: {}",
                response.status
            )),
            response_time_ms: start_time.elapsed().as_millis(),
            service_info: None,
        }
    }
}
