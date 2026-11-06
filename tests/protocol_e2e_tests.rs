//! End-to-end checks against local fake servers (no internet needed).
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use uptime_monitor::config::*;
use uptime_monitor::monitoring::checks::*;
use uptime_monitor::monitoring::CheckStatus;

fn healthy(s: &CheckStatus) -> bool {
    matches!(s, CheckStatus::Healthy)
}
fn reason(s: &CheckStatus) -> String {
    match s {
        CheckStatus::Unhealthy(m) => m.clone(),
        CheckStatus::Healthy => "healthy".into(),
    }
}

// ─── tiny HTTP server ────────────────────────────────────────────────────────

async fn spawn_http() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = match l.accept().await {
                Ok(x) => x,
                Err(_) => return,
            };
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let header_end = loop {
                    let n = s.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let first = head.lines().next().unwrap_or("").to_string();
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                let clen = head
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while buf.len() < header_end + clen {
                    let n = s.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let req_body = String::from_utf8_lossy(&buf[header_end..]).to_string();
                let ua = head
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("user-agent:")
                            .map(|v| v.trim().to_string())
                    })
                    .unwrap_or_default();

                let (status, extra, body): (&str, &str, String) = match path.as_str() {
                    "/ok" => ("200 OK", "", "hello World".into()),
                    "/created" => ("201 Created", "", "made".into()),
                    "/redir" => ("302 Found", "Location: /ok\r\n", String::new()),
                    "/loop" => ("302 Found", "Location: /loop\r\n", String::new()),
                    "/echo" => ("200 OK", "", req_body),
                    "/ua" => ("200 OK", "", ua),
                    "/chunked" => {
                        let resp = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n7\r\n{\"a\":1,\r\n5\r\n\"b\":2\r\n1\r\n}\r\n0\r\n\r\n";
                        let _ = s.write_all(resp.as_bytes()).await;
                        return;
                    }
                    _ => ("500 Internal Server Error", "", "boom".into()),
                };
                let resp = format!(
                    "HTTP/1.1 {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    extra,
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    port
}

fn http(port: u16, path: &str) -> HttpCheck {
    HttpCheck {
        port,
        path: path.into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn http_defaults_follow_redirects_and_status() {
    let port = spawn_http().await;
    // 302 → /ok is followed by default
    let r = check_http_target("127.0.0.1", &http(port, "/redir")).await;
    assert!(healthy(&r.status), "{}", reason(&r.status));
    // …but not when disabled (then the 302 itself is returned)
    let mut c = http(port, "/redir");
    c.follow_redirects = false;
    assert!(reason(&check_http_target("127.0.0.1", &c).await.status).contains("302"));
    // redirect loop is bounded
    let mut c = http(port, "/loop");
    c.max_redirects = 3;
    assert!(reason(&check_http_target("127.0.0.1", &c).await.status)
        .to_lowercase()
        .contains("redirect"));
}

#[tokio::test]
async fn http_accepted_status_ranges() {
    let port = spawn_http().await;
    let mut c = http(port, "/created");
    assert!(!healthy(&check_http_target("127.0.0.1", &c).await.status)); // expects 200
    c.accepted_status_codes = vec!["200-299".into()];
    assert!(healthy(&check_http_target("127.0.0.1", &c).await.status));
    let mut c = http(port, "/fail");
    c.accepted_status_codes = vec!["200-299".into(), "301".into()];
    assert!(reason(&check_http_target("127.0.0.1", &c).await.status).contains("500"));
}

#[tokio::test]
async fn http_keyword_and_invert() {
    let port = spawn_http().await;
    let mut c = http(port, "/ok");
    c.keyword = Some("World".into());
    assert!(healthy(&check_http_target("127.0.0.1", &c).await.status));
    c.keyword = Some("world".into());
    assert!(!healthy(&check_http_target("127.0.0.1", &c).await.status)); // case-sensitive
    c.keyword_case_sensitive = false;
    assert!(healthy(&check_http_target("127.0.0.1", &c).await.status));
    c.invert_keyword = true;
    assert!(reason(&check_http_target("127.0.0.1", &c).await.status).contains("forbidden"));
    c.keyword = Some("absent".into());
    assert!(healthy(&check_http_target("127.0.0.1", &c).await.status));
}

#[tokio::test]
async fn http_post_body_headers_user_agent() {
    let port = spawn_http().await;
    let mut c = http(port, "/echo");
    c.method = HttpMethod::Post;
    c.body = Some("{\"ping\":true}".into());
    c.keyword = Some("\"ping\":true".into());
    assert!(healthy(&check_http_target("127.0.0.1", &c).await.status));

    let mut c = http(port, "/ua");
    c.user_agent = Some("my-agent/9".into());
    c.keyword = Some("my-agent/9".into());
    assert!(healthy(&check_http_target("127.0.0.1", &c).await.status));
    let mut h = HashMap::new();
    h.insert("X-Test".to_string(), "1".to_string());
    c.headers = Some(h);
    assert!(healthy(&check_http_target("127.0.0.1", &c).await.status));
}

#[tokio::test]
async fn http_chunked_json_assertion() {
    // The old hand-rolled client could not decode chunked bodies.
    let port = spawn_http().await;
    let mut c = http(port, "/chunked");
    c.assertions = Some(vec![HttpAssertion {
        query: AssertionQuery::JsonPath { path: "$.b".into() },
        predicate: AssertionPredicate::Equals,
        value: AssertionValue::Integer(2),
    }]);
    let r = check_http_target("127.0.0.1", &c).await;
    assert!(healthy(&r.status), "{}", reason(&r.status));
}

#[tokio::test]
async fn http_upside_down() {
    let port = spawn_http().await;
    let mut c = http(port, "/ok");
    c.upside_down = true;
    assert!(reason(&check_http_target("127.0.0.1", &c).await.status).contains("Upside-down"));
    let mut c = http(port, "/fail");
    c.upside_down = true;
    assert!(healthy(&check_http_target("127.0.0.1", &c).await.status));
}

#[tokio::test]
async fn http_via_proxy() {
    // Fake forward proxy: any request is answered with 200/"via-proxy".
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                let mut b = [0u8; 4096];
                let _ = s.read(&mut b).await;
                let _ = s
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nvia-proxy")
                    .await;
            });
        }
    });
    // Target port is closed; the check only succeeds if the proxy is used.
    let mut c = http(9, "/anything");
    c.proxy = Some(ProxyConfig {
        url: format!("http://127.0.0.1:{}", proxy_port),
        username: None,
        password: None,
    });
    c.keyword = Some("via-proxy".into());
    let r = check_http_target("example.invalid", &c).await;
    assert!(healthy(&r.status), "{}", reason(&r.status));
}

#[tokio::test]
async fn http_rejects_bad_config() {
    let port = spawn_http().await;
    let mut c = http(port, "/ok");
    c.ca_cert_path = Some("/nonexistent/ca.pem".into());
    assert!(reason(&check_http_target("127.0.0.1", &c).await.status).contains("CA file"));
    let mut c = http(port, "/ok");
    c.proxy = Some(ProxyConfig {
        url: "not a url".into(),
        username: None,
        password: None,
    });
    assert!(!healthy(&check_http_target("127.0.0.1", &c).await.status));
}

// ─── WebSocket ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn websocket_handshake_and_expected_message() {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                if let Ok(mut ws) = tokio_tungstenite::accept_async(s).await {
                    let _ = ws.send(Message::Text("welcome aboard".into())).await;
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
            });
        }
    });
    let mut c = WebSocketCheck {
        name: None,
        port,
        path: "/ws".into(),
        use_tls: false,
        ignore_tls_errors: false,
        subprotocol: None,
        send_message: None,
        expect_message: None,
        timeout_seconds: 3,
    };
    assert!(healthy(&check_websocket("127.0.0.1", &c).await.status));
    c.expect_message = Some("welcome".into());
    assert!(healthy(&check_websocket("127.0.0.1", &c).await.status));
    c.expect_message = Some("goodbye".into());
    c.timeout_seconds = 1;
    assert!(!healthy(&check_websocket("127.0.0.1", &c).await.status));
    // Port with nothing behind it
    let dead = WebSocketCheck {
        port: 9,
        expect_message: None,
        ..c
    };
    assert!(!healthy(&check_websocket("127.0.0.1", &dead).await.status));
}

// ─── NTP ─────────────────────────────────────────────────────────────────────

async fn spawn_ntp(offset_secs: i64, stratum: u8) -> u16 {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = sock.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut b = [0u8; 64];
        loop {
            let (_, peer) = match sock.recv_from(&mut b).await {
                Ok(x) => x,
                Err(_) => return,
            };
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            let secs = (now.as_secs() as i64 + offset_secs) as u64 + 2_208_988_800;
            let frac = ((now.subsec_nanos() as u64) << 32) / 1_000_000_000;
            let mut r = [0u8; 48];
            r[0] = 0x24; // LI 0, VN 4, mode 4
            r[1] = stratum;
            r[40..44].copy_from_slice(&(secs as u32).to_be_bytes());
            r[44..48].copy_from_slice(&(frac as u32).to_be_bytes());
            let _ = sock.send_to(&r, peer).await;
        }
    });
    port
}

#[tokio::test]
async fn ntp_offset_and_stratum() {
    let cfg = |port, max| NtpCheck {
        name: None,
        port,
        max_offset_ms: max,
        timeout_seconds: 3,
    };
    let good = spawn_ntp(0, 2).await;
    let r = check_ntp("127.0.0.1", &cfg(good, Some(2000))).await;
    assert!(healthy(&r.status), "{}", reason(&r.status));
    let skewed = spawn_ntp(60, 2).await;
    assert!(reason(
        &check_ntp("127.0.0.1", &cfg(skewed, Some(1000)))
            .await
            .status
    )
    .contains("offset"));
    assert!(healthy(
        &check_ntp("127.0.0.1", &cfg(skewed, None)).await.status
    )); // no limit
    let kod = spawn_ntp(0, 0).await;
    assert!(reason(&check_ntp("127.0.0.1", &cfg(kod, None)).await.status).contains("kiss"));
}

// ─── DNS ─────────────────────────────────────────────────────────────────────

/// Answers every A query with 203.0.113.7.
async fn spawn_dns() -> u16 {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = sock.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut b = [0u8; 512];
        loop {
            let (n, peer) = match sock.recv_from(&mut b).await {
                Ok(x) => x,
                Err(_) => return,
            };
            if n < 12 {
                continue;
            }
            // question section = everything after the 12-byte header
            let mut resp = Vec::new();
            resp.extend_from_slice(&b[0..2]); // id
            resp.extend_from_slice(&[0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0]);
            resp.extend_from_slice(&b[12..n]);
            resp.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 203, 0, 113, 7]);
            let _ = sock.send_to(&resp, peer).await;
        }
    });
    port
}

#[tokio::test]
async fn dns_lookup_with_expected_value() {
    let port = spawn_dns().await;
    let mut c = DnsCheck {
        name: None,
        query: "svc.example.test".into(),
        record_type: DnsRecordType::A,
        resolver: Some("127.0.0.1".into()),
        port,
        expected: Some("203.0.113.7".into()),
        timeout_seconds: 3,
    };
    let r = check_dns(&c).await;
    assert!(healthy(&r.status), "{}", reason(&r.status));
    assert_eq!(r.service_info.as_deref(), Some("203.0.113.7"));
    c.expected = Some("198.51.100.1".into());
    assert!(reason(&check_dns(&c).await.status).contains("198.51.100.1"));
    c.resolver = Some("not-an-ip".into());
    assert!(reason(&check_dns(&c).await.status).contains("not an IP"));
}

// ─── config parsing ──────────────────────────────────────────────────────────

#[test]
fn toml_config_with_all_new_options() {
    let toml_src = r#"
default_language = "zh-TW"

[[hosts]]
address = "example.com"
alias = "Example"
group = "web"
tags = ["prod", "public"]
check_domain_expiry = true

[[hosts.checks]]
type = "Http"
port = 443
path = "/health"
protocol = "Https"
method = "Post"
body = "{}"
keyword = "ok"
invert_keyword = false
accepted_status_codes = ["200-299", "301"]
follow_redirects = true
max_redirects = 5
ignore_tls_errors = true
upside_down = false
proxy = { url = "socks5://127.0.0.1:1080" }

[[hosts.checks]]
type = "Ping"
count = 3

[[hosts.checks]]
type = "Dns"
query = "example.com"
record_type = "MX"
resolver = "1.1.1.1"

[[hosts.checks]]
type = "Push"
token = "abc123"

[[hosts.checks]]
type = "WebSocket"
port = 443
use_tls = true
expect_message = "hi"

[[hosts.checks]]
type = "Smtp"

[[hosts.checks]]
type = "Ntp"
max_offset_ms = 500

[[hosts.checks]]
type = "Mqtt"
username = "u"
password = "p"

[[hosts.checks]]
type = "Grpc"
port = 50051
service = "my.Service"

[[hosts.checks]]
type = "Snmp"
expected = "router"

[[hosts.checks]]
type = "Docker"
container = "web"
"#;
    let cfg: AppConfig = toml::from_str(toml_src).expect("config parses");
    assert_eq!(cfg.default_language.as_deref(), Some("zh-TW"));
    assert_eq!(cfg.domain_expiry_check_interval_hours, 12);
    let h = &cfg.hosts[0];
    assert_eq!(h.group.as_deref(), Some("web"));
    assert_eq!(h.tags, vec!["prod", "public"]);
    assert!(h.check_domain_expiry);
    assert_eq!(h.checks.len(), 11);
    match &h.checks[0] {
        Check::Http(c) => {
            assert_eq!(c.accepted_status_codes.len(), 2);
            assert_eq!(c.max_redirects, 5);
            assert!(c.ignore_tls_errors);
            assert_eq!(c.timeout_seconds, 10); // serde default preserved
            assert_eq!(c.proxy.as_ref().unwrap().url, "socks5://127.0.0.1:1080");
        }
        _ => panic!("expected Http"),
    }
    match &h.checks[1] {
        Check::Ping(c) => assert_eq!((c.count, c.timeout_seconds), (3, 5)),
        _ => panic!("expected Ping"),
    }
    match &h.checks[7] {
        Check::Mqtt(c) => assert_eq!(c.port, 1883),
        _ => panic!("expected Mqtt"),
    }
    match &h.checks[8] {
        Check::Grpc(c) => assert_eq!(
            (c.port, c.service.as_str(), c.use_tls),
            (50051, "my.Service", false)
        ),
        _ => panic!("expected Grpc"),
    }
    match &h.checks[9] {
        Check::Snmp(c) => assert_eq!(
            (c.port, c.community.as_str(), c.oid.as_str()),
            (161, "public", "1.3.6.1.2.1.1.3.0")
        ),
        _ => panic!("expected Snmp"),
    }
    match &h.checks[10] {
        Check::Docker(c) => assert_eq!(
            (c.container.as_str(), c.endpoint.as_str()),
            ("web", "/var/run/docker.sock")
        ),
        _ => panic!("expected Docker"),
    }
}

// ─── gRPC health (fake h2c server) ───────────────────────────────────────────

/// Serves grpc.health.v1.Health/Check. `status`: 1 = SERVING, 2 = NOT_SERVING.
async fn spawn_grpc(status: u8) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (s, _) = match l.accept().await {
                Ok(x) => x,
                Err(_) => return,
            };
            tokio::spawn(async move {
                let mut conn = match h2::server::handshake(s).await {
                    Ok(c) => c,
                    Err(_) => return,
                };
                while let Some(Ok((req, mut respond))) = conn.accept().await {
                    tokio::spawn(async move {
                        let (parts, mut body) = req.into_parts();
                        let mut received = Vec::new();
                        while let Some(Ok(chunk)) = body.data().await {
                            let _ = body.flow_control().release_capacity(chunk.len());
                            received.extend_from_slice(&chunk);
                        }
                        let ok_path = parts.uri.path() == "/grpc.health.v1.Health/Check";
                        // Echo back whether the requested service name was "known".
                        let known = received.len() < 6 || received.ends_with(b"known");
                        let resp = http02::Response::builder()
                            .status(200)
                            .header("content-type", "application/grpc")
                            .body(())
                            .unwrap();
                        let mut send = respond.send_response(resp, false).unwrap();
                        let st = if !ok_path {
                            0
                        } else if known {
                            status
                        } else {
                            3
                        };
                        let msg = [0u8, 0, 0, 0, 2, 0x08, st];
                        send.send_data(bytes::Bytes::copy_from_slice(&msg), false)
                            .unwrap();
                        let mut trailers = http02::HeaderMap::new();
                        trailers.insert("grpc-status", "0".parse().unwrap());
                        send.send_trailers(trailers).unwrap();
                    });
                }
            });
        }
    });
    port
}

fn grpc_cfg(port: u16, service: &str) -> GrpcCheck {
    GrpcCheck {
        name: None,
        port,
        use_tls: false,
        ignore_tls_errors: false,
        service: service.into(),
        timeout_seconds: 3,
    }
}

#[tokio::test]
async fn grpc_health_serving_and_not_serving() {
    let up = spawn_grpc(1).await;
    let r = check_grpc("127.0.0.1", &grpc_cfg(up, "")).await;
    assert!(healthy(&r.status), "{}", reason(&r.status));
    assert_eq!(r.service_info.as_deref(), Some("SERVING"));

    let down = spawn_grpc(2).await;
    assert!(
        reason(&check_grpc("127.0.0.1", &grpc_cfg(down, "")).await.status).contains("NOT_SERVING")
    );

    // Named service that the server does not know → SERVICE_UNKNOWN
    assert!(
        reason(&check_grpc("127.0.0.1", &grpc_cfg(up, "other")).await.status)
            .contains("SERVICE_UNKNOWN")
    );
    assert!(healthy(
        &check_grpc("127.0.0.1", &grpc_cfg(up, "known")).await.status
    ));

    // Nothing listening
    assert!(!healthy(
        &check_grpc("127.0.0.1", &grpc_cfg(9, "")).await.status
    ));
}
