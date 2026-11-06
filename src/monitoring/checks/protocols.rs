//! Additional protocol checks: Ping, DNS, Push, WebSocket, SMTP, NTP, MQTT.
use crate::config::{
    DnsCheck, DnsRecordType, DockerCheck, GrpcCheck, MqttCheck, NtpCheck, PingCheck, PushCheck,
    SmtpCheck, SnmpCheck, WebSocketCheck,
};
use crate::monitoring::push;
use crate::monitoring::types::{CheckStatus, ServiceCheckResult};
use std::net::IpAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;

fn ok(start: Instant, info: Option<String>) -> ServiceCheckResult {
    ServiceCheckResult {
        status: CheckStatus::Healthy,
        response_time_ms: start.elapsed().as_millis(),
        service_info: info,
    }
}

fn fail(start: Instant, msg: impl Into<String>) -> ServiceCheckResult {
    ServiceCheckResult {
        status: CheckStatus::Unhealthy(msg.into()),
        response_time_ms: start.elapsed().as_millis(),
        service_info: None,
    }
}

// ─── Ping ────────────────────────────────────────────────────────────────────

pub async fn check_ping(address: &str, cfg: &PingCheck) -> ServiceCheckResult {
    let start = Instant::now();
    // Never let a hostname be interpreted as a command-line option.
    if address.is_empty() || address.starts_with('-') || address.contains(char::is_whitespace) {
        return fail(start, format!("Invalid ping target '{}'", address));
    }
    let count = cfg.count.max(1);
    let t = cfg.timeout_seconds.max(1);

    let mut cmd = tokio::process::Command::new("ping");
    if cfg!(target_os = "windows") {
        cmd.args([
            "-n",
            &count.to_string(),
            "-w",
            &(t * 1000).to_string(),
            address,
        ]);
    } else if cfg!(target_os = "macos") {
        cmd.args([
            "-c",
            &count.to_string(),
            "-W",
            &(t * 1000).to_string(),
            address,
        ]);
    } else {
        cmd.args(["-c", &count.to_string(), "-W", &t.to_string(), address]);
    }
    cmd.kill_on_drop(true);

    let overall = Duration::from_secs(t * count as u64 + 3);
    match timeout(overall, cmd.output()).await {
        Err(_) => fail(start, format!("ping {} timed out", address)),
        Ok(Err(e)) => fail(
            start,
            format!("Failed to run ping: {} (is `ping` installed?)", e),
        ),
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if !out.status.success() {
                return fail(start, format!("Host {} unreachable (ping failed)", address));
            }
            // Prefer the RTT reported by ping over our own wall-clock time.
            let rtt = stdout
                .lines()
                .filter_map(|l| {
                    let i = l.find("time=").or_else(|| l.find("time<"))?;
                    let rest = &l[i + 5..];
                    let num: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.')
                        .collect();
                    num.parse::<f64>().ok()
                })
                .next_back();
            ServiceCheckResult {
                status: CheckStatus::Healthy,
                response_time_ms: rtt
                    .map(|r| r.round() as u128)
                    .unwrap_or_else(|| start.elapsed().as_millis()),
                service_info: None,
            }
        }
    }
}

// ─── DNS ─────────────────────────────────────────────────────────────────────

pub async fn check_dns(cfg: &DnsCheck) -> ServiceCheckResult {
    use hickory_resolver::config::{NameServerConfigGroup, ResolverConfig, ResolverOpts};
    use hickory_resolver::proto::rr::RecordType;
    use hickory_resolver::TokioAsyncResolver;

    let start = Instant::now();
    let mut opts = ResolverOpts::default();
    opts.timeout = Duration::from_secs(cfg.timeout_seconds.max(1));
    opts.attempts = 1;
    opts.cache_size = 0; // always hit the server; we are monitoring it

    let resolver = match &cfg.resolver {
        Some(r) => {
            let ip: IpAddr = match r.parse() {
                Ok(ip) => ip,
                Err(_) => return fail(start, format!("Resolver '{}' is not an IP address", r)),
            };
            let ns = NameServerConfigGroup::from_ips_clear(&[ip], cfg.port, true);
            TokioAsyncResolver::tokio(ResolverConfig::from_parts(None, vec![], ns), opts)
        }
        None => match hickory_resolver::system_conf::read_system_conf() {
            Ok((conf, mut sys_opts)) => {
                sys_opts.timeout = opts.timeout;
                sys_opts.attempts = 1;
                sys_opts.cache_size = 0;
                TokioAsyncResolver::tokio(conf, sys_opts)
            }
            Err(e) => return fail(start, format!("Cannot read system resolver config: {}", e)),
        },
    };

    let rt = match cfg.record_type {
        DnsRecordType::A => RecordType::A,
        DnsRecordType::AAAA => RecordType::AAAA,
        DnsRecordType::CNAME => RecordType::CNAME,
        DnsRecordType::MX => RecordType::MX,
        DnsRecordType::NS => RecordType::NS,
        DnsRecordType::TXT => RecordType::TXT,
        DnsRecordType::SRV => RecordType::SRV,
        DnsRecordType::CAA => RecordType::CAA,
        DnsRecordType::PTR => RecordType::PTR,
    };

    let records: Vec<String> = if cfg.record_type == DnsRecordType::PTR {
        let ip: IpAddr = match cfg.query.parse() {
            Ok(ip) => ip,
            Err(_) => return fail(start, "PTR query must be an IP address"),
        };
        match resolver.reverse_lookup(ip).await {
            Ok(l) => l.iter().map(|n| n.to_string()).collect(),
            Err(e) => {
                return fail(
                    start,
                    format!("DNS PTR lookup for {} failed: {}", cfg.query, e),
                )
            }
        }
    } else {
        match resolver.lookup(cfg.query.as_str(), rt).await {
            Ok(l) => l.iter().map(|r| r.to_string()).collect(),
            Err(e) => {
                return fail(
                    start,
                    format!(
                        "DNS {:?} lookup for {} failed: {}",
                        cfg.record_type, cfg.query, e
                    ),
                )
            }
        }
    };

    if records.is_empty() {
        return fail(
            start,
            format!("No {:?} records for {}", cfg.record_type, cfg.query),
        );
    }
    if let Some(expected) = &cfg.expected {
        if !records.iter().any(|r| r.contains(expected.as_str())) {
            return fail(
                start,
                format!(
                    "None of the records [{}] contain '{}'",
                    records.join(", "),
                    expected
                ),
            );
        }
    }
    ok(start, Some(records.join(", ")))
}

// ─── Push ────────────────────────────────────────────────────────────────────

pub fn check_push(cfg: &PushCheck, default_interval_secs: u64) -> ServiceCheckResult {
    let start = Instant::now();
    let max_silence =
        Duration::from_secs(cfg.max_silence_seconds.unwrap_or(default_interval_secs * 2));
    match push::last_push(&cfg.token) {
        None => {
            // Grace period after startup so a restart does not flap every push monitor.
            if push::started_at().elapsed() < max_silence {
                ok(start, Some("waiting for first push".into()))
            } else {
                fail(start, "No push received yet")
            }
        }
        Some(rec) => {
            let age = rec.at.elapsed();
            if age > max_silence {
                fail(
                    start,
                    format!(
                        "No push received for {}s (limit {}s)",
                        age.as_secs(),
                        max_silence.as_secs()
                    ),
                )
            } else if !rec.up {
                fail(start, format!("Job reported down: {}", rec.message))
            } else {
                ServiceCheckResult {
                    status: CheckStatus::Healthy,
                    response_time_ms: rec.ping_ms.unwrap_or(0),
                    service_info: Some(rec.message),
                }
            }
        }
    }
}

// ─── WebSocket ───────────────────────────────────────────────────────────────

pub async fn check_websocket(address: &str, cfg: &WebSocketCheck) -> ServiceCheckResult {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::{connect_async_tls_with_config, Connector};

    let start = Instant::now();
    let limit = Duration::from_secs(cfg.timeout_seconds.max(1));
    let url = format!(
        "{}://{}:{}{}",
        if cfg.use_tls { "wss" } else { "ws" },
        address,
        cfg.port,
        if cfg.path.starts_with('/') {
            cfg.path.clone()
        } else {
            format!("/{}", cfg.path)
        }
    );
    let mut req = match url.as_str().into_client_request() {
        Ok(r) => r,
        Err(e) => return fail(start, format!("Invalid WebSocket URL {}: {}", url, e)),
    };
    if let Some(sp) = &cfg.subprotocol {
        match sp.parse() {
            Ok(v) => {
                req.headers_mut().insert("Sec-WebSocket-Protocol", v);
            }
            Err(_) => return fail(start, "Invalid subprotocol value"),
        }
    }
    let connector = if cfg.use_tls && cfg.ignore_tls_errors {
        match native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
        {
            Ok(c) => Some(Connector::NativeTls(c)),
            Err(e) => return fail(start, format!("TLS setup failed: {}", e)),
        }
    } else {
        None
    };

    let session = async {
        let (mut ws, _resp) = connect_async_tls_with_config(req, None, false, connector)
            .await
            .map_err(|e| format!("WebSocket handshake with {} failed: {}", url, e))?;
        if let Some(msg) = &cfg.send_message {
            ws.send(Message::Text(msg.clone()))
                .await
                .map_err(|e| format!("Send failed: {}", e))?;
        }
        if let Some(expected) = &cfg.expect_message {
            loop {
                match ws.next().await {
                    Some(Ok(Message::Text(t))) if t.contains(expected.as_str()) => break,
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(format!("Read failed: {}", e)),
                    None => return Err("Connection closed before expected message".to_string()),
                }
            }
        }
        let _ = ws.close(None).await;
        Ok::<(), String>(())
    };

    match timeout(limit, session).await {
        Ok(Ok(())) => ok(start, None),
        Ok(Err(e)) => fail(start, e),
        Err(_) => fail(
            start,
            format!("WebSocket check timed out after {}s", limit.as_secs()),
        ),
    }
}

// ─── SMTP ────────────────────────────────────────────────────────────────────

/// Reads a (possibly multi-line) SMTP reply and returns (code, full text).
async fn read_smtp_reply<R: AsyncBufReadExt + Unpin>(r: &mut R) -> Result<(u16, String), String> {
    let mut text = String::new();
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("connection closed".into());
        }
        text.push_str(&line);
        let b = line.as_bytes();
        if b.len() >= 4 && b[3] == b' ' || b.len() == 3 {
            let code = line[..3]
                .parse::<u16>()
                .map_err(|_| format!("bad reply: {}", line.trim()))?;
            return Ok((code, text.trim().to_string()));
        }
        if b.len() < 4 {
            return Err(format!("bad reply: {}", line.trim()));
        }
    }
}

pub async fn check_smtp(address: &str, cfg: &SmtpCheck) -> ServiceCheckResult {
    let start = Instant::now();
    let limit = Duration::from_secs(cfg.timeout_seconds.max(1));
    let session = async {
        let stream = TcpStream::connect((address, cfg.port))
            .await
            .map_err(|e| format!("Connect to {}:{} failed: {}", address, cfg.port, e))?;
        let (rd, mut wr) = stream.into_split();
        let mut rd = BufReader::new(rd);
        let (code, banner) = read_smtp_reply(&mut rd).await?;
        if code != 220 {
            return Err(format!("Unexpected SMTP greeting {}: {}", code, banner));
        }
        if let Some(exp) = &cfg.expect_banner {
            if !banner.contains(exp.as_str()) {
                return Err(format!("Banner '{}' does not contain '{}'", banner, exp));
            }
        }
        wr.write_all(b"EHLO ruptime.local\r\n")
            .await
            .map_err(|e| e.to_string())?;
        let (code, _) = read_smtp_reply(&mut rd).await?;
        if code != 250 {
            return Err(format!("EHLO rejected with {}", code));
        }
        let _ = wr.write_all(b"QUIT\r\n").await;
        Ok::<String, String>(banner)
    };
    match timeout(limit, session).await {
        Ok(Ok(b)) => ok(start, Some(b.lines().next().unwrap_or("").to_string())),
        Ok(Err(e)) => fail(start, e),
        Err(_) => fail(
            start,
            format!("SMTP check timed out after {}s", limit.as_secs()),
        ),
    }
}

// ─── NTP ─────────────────────────────────────────────────────────────────────

const NTP_UNIX_OFFSET_SECS: u64 = 2_208_988_800; // 1900 → 1970

pub async fn check_ntp(address: &str, cfg: &NtpCheck) -> ServiceCheckResult {
    let start = Instant::now();
    let limit = Duration::from_secs(cfg.timeout_seconds.max(1));
    let session = async {
        let target = tokio::net::lookup_host((address, cfg.port))
            .await
            .map_err(|e| format!("DNS lookup failed: {}", e))?
            .next()
            .ok_or("No addresses resolved")?;
        let bind = if target.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        let sock = UdpSocket::bind(bind).await.map_err(|e| e.to_string())?;
        sock.connect(target).await.map_err(|e| e.to_string())?;

        let mut pkt = [0u8; 48];
        pkt[0] = 0x23; // LI=0, VN=4, Mode=3 (client)
        let sent_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis() as i128;
        let t0 = Instant::now();
        sock.send(&pkt).await.map_err(|e| e.to_string())?;
        let mut buf = [0u8; 128];
        let n = sock.recv(&mut buf).await.map_err(|e| e.to_string())?;
        let rtt_ms = t0.elapsed().as_millis() as i128;
        if n < 48 {
            return Err(format!("Short NTP reply ({} bytes)", n));
        }
        let leap = buf[0] >> 6;
        let mode = buf[0] & 0x07;
        let stratum = buf[1];
        if mode != 4 && mode != 5 {
            return Err(format!("Unexpected NTP mode {}", mode));
        }
        if stratum == 0 {
            return Err("NTP kiss-o'-death (stratum 0)".into());
        }
        if leap == 3 {
            return Err("NTP server is not synchronised".into());
        }
        let secs = u32::from_be_bytes([buf[40], buf[41], buf[42], buf[43]]) as u64;
        let frac = u32::from_be_bytes([buf[44], buf[45], buf[46], buf[47]]) as u64;
        if secs < NTP_UNIX_OFFSET_SECS {
            return Err("NTP reply has an invalid timestamp".into());
        }
        let server_ms = ((secs - NTP_UNIX_OFFSET_SECS) * 1000 + ((frac * 1000) >> 32)) as i128;
        let offset_ms = server_ms - (sent_ms + rtt_ms / 2);
        Ok::<(i128, u8), String>((offset_ms, stratum))
    };
    match timeout(limit, session).await {
        Ok(Ok((offset, stratum))) => {
            if let Some(max) = cfg.max_offset_ms {
                if offset.unsigned_abs() > max as u128 {
                    return fail(
                        start,
                        format!("Clock offset {}ms exceeds limit {}ms", offset, max),
                    );
                }
            }
            ok(
                start,
                Some(format!("stratum {}, offset {}ms", stratum, offset)),
            )
        }
        Ok(Err(e)) => fail(start, e),
        Err(_) => fail(
            start,
            format!("NTP check timed out after {}s", limit.as_secs()),
        ),
    }
}

// ─── MQTT ────────────────────────────────────────────────────────────────────

fn mqtt_push_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u16).to_be_bytes());
    buf.extend_from_slice(s.as_bytes());
}

fn mqtt_remaining_len(mut len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut b = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            b |= 0x80;
        }
        out.push(b);
        if len == 0 {
            break;
        }
    }
    out
}

/// Builds an MQTT 3.1.1 CONNECT packet.
pub(crate) fn build_mqtt_connect(cfg: &MqttCheck) -> Vec<u8> {
    let client_id = cfg
        .client_id
        .clone()
        .unwrap_or_else(|| "ruptime".to_string());
    let mut flags = 0x02u8; // clean session
    if cfg.username.is_some() {
        flags |= 0x80;
    }
    if cfg.password.is_some() {
        flags |= 0x40;
    }
    let mut body = Vec::new();
    mqtt_push_str(&mut body, "MQTT");
    body.push(4); // protocol level 3.1.1
    body.push(flags);
    body.extend_from_slice(&60u16.to_be_bytes());
    mqtt_push_str(&mut body, &client_id);
    if let Some(u) = &cfg.username {
        mqtt_push_str(&mut body, u);
    }
    if let Some(p) = &cfg.password {
        mqtt_push_str(&mut body, p);
    }
    let mut pkt = vec![0x10];
    pkt.extend(mqtt_remaining_len(body.len()));
    pkt.extend(body);
    pkt
}

pub async fn check_mqtt(address: &str, cfg: &MqttCheck) -> ServiceCheckResult {
    let start = Instant::now();
    let limit = Duration::from_secs(cfg.timeout_seconds.max(1));
    let session = async {
        let mut s = TcpStream::connect((address, cfg.port))
            .await
            .map_err(|e| format!("Connect to {}:{} failed: {}", address, cfg.port, e))?;
        s.write_all(&build_mqtt_connect(cfg))
            .await
            .map_err(|e| e.to_string())?;
        let mut ack = [0u8; 4];
        s.read_exact(&mut ack)
            .await
            .map_err(|e| format!("No CONNACK: {}", e))?;
        if ack[0] != 0x20 || ack[1] != 0x02 {
            return Err("Response is not an MQTT CONNACK".to_string());
        }
        let _ = s.write_all(&[0xE0, 0x00]).await; // DISCONNECT
        match ack[3] {
            0 => Ok(()),
            1 => Err("MQTT refused: unacceptable protocol version".into()),
            2 => Err("MQTT refused: client identifier rejected".into()),
            3 => Err("MQTT refused: server unavailable".into()),
            4 => Err("MQTT refused: bad username or password".into()),
            5 => Err("MQTT refused: not authorized".into()),
            c => Err(format!("MQTT refused: return code {}", c)),
        }
    };
    match timeout(limit, session).await {
        Ok(Ok(())) => ok(start, None),
        Ok(Err(e)) => fail(start, e),
        Err(_) => fail(
            start,
            format!("MQTT check timed out after {}s", limit.as_secs()),
        ),
    }
}

// ─── gRPC health ─────────────────────────────────────────────────────────────

/// Wraps a protobuf message in a gRPC length-prefixed frame.
pub(crate) fn grpc_frame(msg: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8];
    out.extend_from_slice(&(msg.len() as u32).to_be_bytes());
    out.extend_from_slice(msg);
    out
}

/// `HealthCheckRequest { string service = 1; }`
pub(crate) fn health_request(service: &str) -> Result<Vec<u8>, String> {
    if service.len() > 127 {
        return Err("gRPC service name longer than 127 bytes is not supported".into());
    }
    let mut msg = Vec::new();
    if !service.is_empty() {
        msg.push(0x0A);
        msg.push(service.len() as u8);
        msg.extend_from_slice(service.as_bytes());
    }
    Ok(grpc_frame(&msg))
}

/// Extracts `status` (field 1, varint) from a framed `HealthCheckResponse`.
pub(crate) fn parse_health_response(body: &[u8]) -> Result<u64, String> {
    if body.len() < 5 {
        return Err("empty gRPC response".into());
    }
    let len = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
    let msg = body.get(5..5 + len).ok_or("truncated gRPC message")?;
    // An absent field means the default value, UNKNOWN (0).
    if msg.is_empty() {
        return Ok(0);
    }
    if msg[0] != 0x08 {
        return Err("unexpected HealthCheckResponse layout".into());
    }
    let mut v = 0u64;
    for (i, b) in msg[1..].iter().take(9).enumerate() {
        v |= ((b & 0x7F) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err("bad varint in HealthCheckResponse".into())
}

pub async fn check_grpc(address: &str, cfg: &GrpcCheck) -> ServiceCheckResult {
    let start = Instant::now();
    let body = match health_request(&cfg.service) {
        Ok(b) => b,
        Err(e) => return fail(start, e),
    };
    let mut builder = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(cfg.timeout_seconds.max(1)))
        .danger_accept_invalid_certs(cfg.ignore_tls_errors);
    if !cfg.use_tls {
        builder = builder.http2_prior_knowledge();
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(e) => return fail(start, format!("gRPC client setup failed: {}", e)),
    };
    let host = if address.contains(':') && !address.starts_with('[') {
        format!("[{}]", address)
    } else {
        address.to_string()
    };
    let url = format!(
        "{}://{}:{}/grpc.health.v1.Health/Check",
        if cfg.use_tls { "https" } else { "http" },
        host,
        cfg.port
    );
    let resp = match client
        .post(&url)
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return fail(start, format!("gRPC request to {} failed: {}", url, e)),
    };
    if resp.status() != reqwest::StatusCode::OK {
        return fail(
            start,
            format!("gRPC endpoint answered HTTP {}", resp.status()),
        );
    }
    // "Trailers-only" error responses carry grpc-status in the headers.
    if let Some(code) = resp
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
    {
        if code != "0" {
            let msg = resp
                .headers()
                .get("grpc-message")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            return fail(
                start,
                format!("gRPC status {} {}", code, msg).trim().to_string(),
            );
        }
    }
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return fail(start, format!("Failed reading gRPC response: {}", e)),
    };
    match parse_health_response(&bytes) {
        Ok(1) => ok(start, Some("SERVING".into())),
        Ok(2) => fail(start, "gRPC health: NOT_SERVING"),
        Ok(3) => fail(
            start,
            format!("gRPC health: SERVICE_UNKNOWN '{}'", cfg.service),
        ),
        Ok(n) => fail(start, format!("gRPC health: UNKNOWN status {}", n)),
        Err(e) => fail(start, e),
    }
}

// ─── SNMP (v2c GET) ──────────────────────────────────────────────────────────

fn ber_len(len: usize) -> Vec<u8> {
    if len < 128 {
        vec![len as u8]
    } else if len < 256 {
        vec![0x81, len as u8]
    } else {
        vec![0x82, (len >> 8) as u8, len as u8]
    }
}

fn ber_tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend(ber_len(content.len()));
    v.extend_from_slice(content);
    v
}

fn ber_int(n: i64) -> Vec<u8> {
    let mut bytes = n.to_be_bytes().to_vec();
    while bytes.len() > 1
        && ((bytes[0] == 0 && bytes[1] & 0x80 == 0) || (bytes[0] == 0xFF && bytes[1] & 0x80 != 0))
    {
        bytes.remove(0);
    }
    ber_tlv(0x02, &bytes)
}

pub(crate) fn encode_oid(oid: &str) -> Result<Vec<u8>, String> {
    let arcs: Vec<u64> = oid
        .trim_start_matches('.')
        .split('.')
        .map(|p| {
            p.parse::<u64>()
                .map_err(|_| format!("Invalid OID '{}'", oid))
        })
        .collect::<Result<_, _>>()?;
    if arcs.len() < 2 || arcs[0] > 2 || (arcs[0] < 2 && arcs[1] >= 40) {
        return Err(format!("Invalid OID '{}'", oid));
    }
    let mut out = vec![(arcs[0] * 40 + arcs[1]) as u8];
    for &a in &arcs[2..] {
        let mut chunk = vec![(a & 0x7F) as u8];
        let mut n = a >> 7;
        while n > 0 {
            chunk.push(((n & 0x7F) as u8) | 0x80);
            n >>= 7;
        }
        chunk.reverse();
        out.extend(chunk);
    }
    Ok(ber_tlv(0x06, &out))
}

pub(crate) fn build_snmp_get(
    community: &str,
    oid: &str,
    request_id: i32,
) -> Result<Vec<u8>, String> {
    let varbind = ber_tlv(0x30, &[encode_oid(oid)?, vec![0x05, 0x00]].concat());
    let pdu_body = [
        ber_int(request_id as i64),
        ber_int(0),
        ber_int(0),
        ber_tlv(0x30, &varbind),
    ]
    .concat();
    let msg = [
        ber_int(1),
        ber_tlv(0x04, community.as_bytes()),
        ber_tlv(0xA0, &pdu_body),
    ]
    .concat();
    Ok(ber_tlv(0x30, &msg))
}

/// Reads one TLV, returning (tag, content, rest).
fn read_tlv(buf: &[u8]) -> Result<(u8, &[u8], &[u8]), String> {
    if buf.len() < 2 {
        return Err("truncated SNMP packet".into());
    }
    let tag = buf[0];
    let (len, hdr) = match buf[1] {
        l if l < 0x80 => (l as usize, 2),
        0x81 if buf.len() >= 3 => (buf[2] as usize, 3),
        0x82 if buf.len() >= 4 => (((buf[2] as usize) << 8) | buf[3] as usize, 4),
        _ => return Err("unsupported BER length".into()),
    };
    let end = hdr + len;
    if buf.len() < end {
        return Err("truncated SNMP packet".into());
    }
    Ok((tag, &buf[hdr..end], &buf[end..]))
}

fn ber_to_i64(content: &[u8]) -> i64 {
    let mut v: i64 = if content.first().is_some_and(|b| b & 0x80 != 0) {
        -1
    } else {
        0
    };
    for b in content {
        v = (v << 8) | *b as i64;
    }
    v
}

/// Parses a GetResponse, returning (request_id, value rendered as text).
pub(crate) fn parse_snmp_response(buf: &[u8]) -> Result<(i64, String), String> {
    let (tag, msg, _) = read_tlv(buf)?;
    if tag != 0x30 {
        return Err("not an SNMP message".into());
    }
    let (_, _version, rest) = read_tlv(msg)?;
    let (_, _community, rest) = read_tlv(rest)?;
    let (pdu_tag, pdu, _) = read_tlv(rest)?;
    if pdu_tag != 0xA2 {
        return Err(format!("unexpected SNMP PDU type 0x{:02X}", pdu_tag));
    }
    let (_, rid, rest) = read_tlv(pdu)?;
    let (_, err_status, rest) = read_tlv(rest)?;
    let (_, _err_index, rest) = read_tlv(rest)?;
    let err = ber_to_i64(err_status);
    if err != 0 {
        return Err(format!("SNMP error-status {}", err));
    }
    let (_, vbl, _) = read_tlv(rest)?;
    let (_, vb, _) = read_tlv(vbl)?;
    let (_, _oid, rest) = read_tlv(vb)?;
    let (vtag, val, _) = read_tlv(rest)?;
    let text = match vtag {
        0x02 | 0x41 | 0x42 | 0x43 | 0x46 => ber_to_i64(val).to_string(), // int, counter, gauge, timeticks, counter64
        0x04 => String::from_utf8_lossy(val).to_string(),
        0x40 if val.len() == 4 => format!("{}.{}.{}.{}", val[0], val[1], val[2], val[3]),
        0x06 => format!("oid({} bytes)", val.len()),
        0x05 => String::new(),
        0x80 => return Err("SNMP noSuchObject".into()),
        0x81 => return Err("SNMP noSuchInstance".into()),
        0x82 => return Err("SNMP endOfMibView".into()),
        t => return Err(format!("unsupported SNMP value type 0x{:02X}", t)),
    };
    Ok((ber_to_i64(rid), text))
}

pub async fn check_snmp(address: &str, cfg: &SnmpCheck) -> ServiceCheckResult {
    let start = Instant::now();
    let request_id = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(1)
        & 0x7FFF_FFFF) as i32;
    let pkt = match build_snmp_get(&cfg.community, &cfg.oid, request_id) {
        Ok(p) => p,
        Err(e) => return fail(start, e),
    };
    let limit = Duration::from_secs(cfg.timeout_seconds.max(1));
    let session = async {
        let target = tokio::net::lookup_host((address, cfg.port))
            .await
            .map_err(|e| format!("DNS lookup failed: {}", e))?
            .next()
            .ok_or("No addresses resolved")?;
        let sock = UdpSocket::bind(if target.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        })
        .await
        .map_err(|e| e.to_string())?;
        sock.connect(target).await.map_err(|e| e.to_string())?;
        sock.send(&pkt).await.map_err(|e| e.to_string())?;
        let mut buf = [0u8; 2048];
        loop {
            let n = sock.recv(&mut buf).await.map_err(|e| e.to_string())?;
            let (rid, value) = parse_snmp_response(&buf[..n])?;
            if rid == request_id as i64 {
                return Ok::<String, String>(value);
            }
        }
    };
    match timeout(limit, session).await {
        Ok(Ok(value)) => {
            if let Some(exp) = &cfg.expected {
                if !value.contains(exp.as_str()) {
                    return fail(
                        start,
                        format!("SNMP value '{}' does not contain '{}'", value, exp),
                    );
                }
            }
            ok(start, Some(value))
        }
        Ok(Err(e)) => fail(start, e),
        Err(_) => fail(
            start,
            format!(
                "SNMP request timed out after {}s (check community / firewall)",
                limit.as_secs()
            ),
        ),
    }
}

// ─── Docker ──────────────────────────────────────────────────────────────────

async fn docker_http_get<S>(mut s: S, path: &str) -> Result<(u16, String), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // HTTP/1.0 + close: the engine answers with Content-Length, never chunked.
    let req = format!(
        "GET {} HTTP/1.0\r\nHost: docker\r\nAccept: application/json\r\n\r\n",
        path
    );
    s.write_all(req.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or("malformed Docker API response")?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or("malformed Docker API status line")?;
    Ok((status, body.to_string()))
}

pub async fn check_docker(cfg: &DockerCheck) -> ServiceCheckResult {
    let start = Instant::now();
    // The name ends up in a URL path: allow only what Docker itself allows.
    if cfg.container.is_empty()
        || !cfg
            .container
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    {
        return fail(start, format!("Invalid container name '{}'", cfg.container));
    }
    let path = format!("/containers/{}/json", cfg.container);
    let limit = Duration::from_secs(cfg.timeout_seconds.max(1));
    let session = async {
        if let Some(addr) = cfg.endpoint.strip_prefix("tcp://") {
            let s = TcpStream::connect(addr)
                .await
                .map_err(|e| format!("Cannot reach Docker at {}: {}", addr, e))?;
            docker_http_get(s, &path).await
        } else {
            #[cfg(unix)]
            {
                let s = tokio::net::UnixStream::connect(&cfg.endpoint)
                    .await
                    .map_err(|e| format!("Cannot open Docker socket {}: {}", cfg.endpoint, e))?;
                docker_http_get(s, &path).await
            }
            #[cfg(not(unix))]
            {
                Err::<(u16, String), String>(
                    "Unix Docker sockets are not supported on this platform; use tcp://host:port"
                        .into(),
                )
            }
        }
    };
    let (status, body) = match timeout(limit, session).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return fail(start, e),
        Err(_) => {
            return fail(
                start,
                format!("Docker API timed out after {}s", limit.as_secs()),
            )
        }
    };
    if status == 404 {
        return fail(start, format!("Container '{}' not found", cfg.container));
    }
    if status != 200 {
        return fail(start, format!("Docker API returned HTTP {}", status));
    }
    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(j) => j,
        Err(e) => return fail(start, format!("Bad Docker API JSON: {}", e)),
    };
    let state = &json["State"];
    let state_name = state["Status"].as_str().unwrap_or("unknown");
    if state["Running"].as_bool() != Some(true) {
        return fail(
            start,
            format!("Container '{}' is {}", cfg.container, state_name),
        );
    }
    match state["Health"]["Status"].as_str() {
        Some("healthy") | None => ok(
            start,
            Some(format!(
                "{} ({})",
                state_name,
                state["Health"]["Status"]
                    .as_str()
                    .unwrap_or("no healthcheck")
            )),
        ),
        Some(h) => fail(
            start,
            format!(
                "Container '{}' is running but health is '{}'",
                cfg.container, h
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mqtt_connect_packet_layout() {
        let cfg = MqttCheck {
            name: None,
            port: 1883,
            username: Some("u".into()),
            password: Some("p".into()),
            client_id: Some("cid".into()),
            timeout_seconds: 5,
        };
        let p = build_mqtt_connect(&cfg);
        assert_eq!(p[0], 0x10);
        assert_eq!(p[1] as usize, p.len() - 2); // short packet: 1-byte remaining length
        assert_eq!(&p[2..8], &[0, 4, b'M', b'Q', b'T', b'T']);
        assert_eq!(p[8], 4);
        assert_eq!(p[9], 0xC2); // user + pass + clean session
    }

    #[test]
    fn mqtt_remaining_length_multibyte() {
        assert_eq!(mqtt_remaining_len(0), vec![0]);
        assert_eq!(mqtt_remaining_len(127), vec![127]);
        assert_eq!(mqtt_remaining_len(128), vec![0x80, 0x01]);
        assert_eq!(mqtt_remaining_len(16384), vec![0x80, 0x80, 0x01]);
    }

    #[test]
    fn push_grace_then_stale() {
        let cfg = PushCheck {
            name: None,
            token: "t-grace".into(),
            max_silence_seconds: Some(60),
        };
        // No push yet, within startup grace → healthy.
        assert!(matches!(check_push(&cfg, 30).status, CheckStatus::Healthy));
        push::record_push("t-grace", false, "boom".into(), None);
        assert!(matches!(
            check_push(&cfg, 30).status,
            CheckStatus::Unhealthy(_)
        ));
        push::record_push("t-grace", true, "fine".into(), Some(7));
        let r = check_push(&cfg, 30);
        assert!(matches!(r.status, CheckStatus::Healthy));
        assert_eq!(r.response_time_ms, 7);
    }

    #[test]
    fn snmp_oid_and_request_encoding() {
        // sysUpTime.0 = 1.3.6.1.2.1.1.3.0 → 2B 06 01 02 01 01 03 00
        assert_eq!(
            encode_oid("1.3.6.1.2.1.1.3.0").unwrap(),
            vec![0x06, 8, 0x2B, 6, 1, 2, 1, 1, 3, 0]
        );
        // multi-byte arc: 1.3.6.1.4.1.8072 → 8072 = 0xBF 0x08
        assert_eq!(
            encode_oid(".1.3.6.1.4.1.8072").unwrap(),
            vec![0x06, 7, 0x2B, 6, 1, 4, 1, 0xBF, 0x08]
        );
        assert!(encode_oid("1").is_err());
        assert!(encode_oid("3.1.2").is_err());
        assert!(encode_oid("1.x.2").is_err());
        let p = build_snmp_get("public", "1.3.6.1.2.1.1.3.0", 5).unwrap();
        assert_eq!(p[0], 0x30);
        assert_eq!(&p[2..5], &[0x02, 0x01, 0x01]); // version 1 = v2c
        assert!(p.windows(6).any(|w| w == b"public"));
    }

    /// Builds an SNMP GetResponse like an agent would.
    fn snmp_reply(request_id: i64, err: i64, value_tlv: Vec<u8>) -> Vec<u8> {
        let vb = ber_tlv(
            0x30,
            &[encode_oid("1.3.6.1.2.1.1.3.0").unwrap(), value_tlv].concat(),
        );
        let pdu = [
            ber_int(request_id),
            ber_int(err),
            ber_int(0),
            ber_tlv(0x30, &vb),
        ]
        .concat();
        ber_tlv(
            0x30,
            &[ber_int(1), ber_tlv(0x04, b"public"), ber_tlv(0xA2, &pdu)].concat(),
        )
    }

    #[test]
    fn snmp_response_parsing() {
        let r = snmp_reply(77, 0, ber_tlv(0x43, &[0x01, 0x00])); // TimeTicks 256
        assert_eq!(parse_snmp_response(&r), Ok((77, "256".to_string())));
        let r = snmp_reply(1, 0, ber_tlv(0x04, b"Linux box"));
        assert_eq!(parse_snmp_response(&r).unwrap().1, "Linux box");
        assert!(parse_snmp_response(&snmp_reply(1, 2, vec![0x05, 0]))
            .unwrap_err()
            .contains("error-status 2"));
        assert!(parse_snmp_response(&snmp_reply(1, 0, vec![0x81, 0]))
            .unwrap_err()
            .contains("noSuchInstance"));
        assert!(parse_snmp_response(&[0x30, 0x05, 1, 2]).is_err());
    }

    #[tokio::test]
    async fn snmp_against_fake_agent() {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = sock.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut b = [0u8; 512];
            loop {
                let (n, peer) = sock.recv_from(&mut b).await.unwrap();
                // request-id sits right after the PDU header: find A0 tag, then INTEGER
                let i = b[..n].iter().position(|&x| x == 0xA0).unwrap();
                let (_, rid, _) = read_tlv(&b[i + 2..n]).unwrap();
                let rid = ber_to_i64(rid);
                let _ = sock
                    .send_to(&snmp_reply(rid, 0, ber_tlv(0x04, b"router-7")), peer)
                    .await;
            }
        });
        let mut c = SnmpCheck {
            name: None,
            port,
            community: "public".into(),
            oid: "1.3.6.1.2.1.1.3.0".into(),
            expected: Some("router".into()),
            timeout_seconds: 3,
        };
        let r = check_snmp("127.0.0.1", &c).await;
        assert!(matches!(r.status, CheckStatus::Healthy), "{:?}", r.status);
        assert_eq!(r.service_info.as_deref(), Some("router-7"));
        c.expected = Some("switch".into());
        assert!(matches!(
            check_snmp("127.0.0.1", &c).await.status,
            CheckStatus::Unhealthy(_)
        ));
        c.oid = "bad".into();
        assert!(matches!(
            check_snmp("127.0.0.1", &c).await.status,
            CheckStatus::Unhealthy(_)
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn docker_over_unix_socket() {
        let dir = std::env::temp_dir().join(format!("ruptime-docker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock_path = dir.join("docker.sock");
        let _ = std::fs::remove_file(&sock_path);
        let l = tokio::net::UnixListener::bind(&sock_path).unwrap();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = l.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut b = [0u8; 1024];
                    let n = s.read(&mut b).await.unwrap();
                    let req = String::from_utf8_lossy(&b[..n]).to_string();
                    let (status, body) = if req.contains("/containers/web/json") {
                        (
                            "200 OK",
                            r#"{"State":{"Status":"running","Running":true,"Health":{"Status":"healthy"}}}"#,
                        )
                    } else if req.contains("/containers/sick/json") {
                        (
                            "200 OK",
                            r#"{"State":{"Status":"running","Running":true,"Health":{"Status":"unhealthy"}}}"#,
                        )
                    } else if req.contains("/containers/plain/json") {
                        ("200 OK", r#"{"State":{"Status":"running","Running":true}}"#)
                    } else if req.contains("/containers/stopped/json") {
                        ("200 OK", r#"{"State":{"Status":"exited","Running":false}}"#)
                    } else {
                        ("404 Not Found", r#"{"message":"No such container"}"#)
                    };
                    let _ = s
                        .write_all(
                            format!(
                                "HTTP/1.0 {}\r\nContent-Length: {}\r\n\r\n{}",
                                status,
                                body.len(),
                                body
                            )
                            .as_bytes(),
                        )
                        .await;
                });
            }
        });
        let cfg = |name: &str| DockerCheck {
            name: None,
            container: name.into(),
            endpoint: sock_path.to_string_lossy().to_string(),
            timeout_seconds: 3,
        };
        assert!(matches!(
            check_docker(&cfg("web")).await.status,
            CheckStatus::Healthy
        ));
        assert!(matches!(
            check_docker(&cfg("plain")).await.status,
            CheckStatus::Healthy
        ));
        let r = check_docker(&cfg("sick")).await;
        assert!(matches!(&r.status, CheckStatus::Unhealthy(m) if m.contains("unhealthy")));
        let r = check_docker(&cfg("stopped")).await;
        assert!(matches!(&r.status, CheckStatus::Unhealthy(m) if m.contains("exited")));
        let r = check_docker(&cfg("ghost")).await;
        assert!(matches!(&r.status, CheckStatus::Unhealthy(m) if m.contains("not found")));
        // path-injection attempt is rejected before any I/O
        let r = check_docker(&cfg("../../etc")).await;
        assert!(matches!(&r.status, CheckStatus::Unhealthy(m) if m.contains("Invalid container")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grpc_health_wire_format() {
        assert_eq!(health_request("").unwrap(), vec![0, 0, 0, 0, 0]);
        assert_eq!(
            health_request("ab").unwrap(),
            vec![0, 0, 0, 0, 4, 0x0A, 2, b'a', b'b']
        );
        assert!(health_request(&"x".repeat(200)).is_err());
        assert_eq!(parse_health_response(&[0, 0, 0, 0, 2, 0x08, 0x01]), Ok(1));
        assert_eq!(parse_health_response(&[0, 0, 0, 0, 0]), Ok(0));
        assert!(parse_health_response(&[0, 0, 0, 0, 9, 0x08]).is_err());
        assert!(parse_health_response(&[]).is_err());
    }

    #[tokio::test]
    async fn ping_rejects_option_injection() {
        let cfg = PingCheck {
            name: None,
            count: 1,
            timeout_seconds: 1,
        };
        assert!(matches!(
            check_ping("-f", &cfg).await.status,
            CheckStatus::Unhealthy(_)
        ));
    }

    #[tokio::test]
    async fn mqtt_against_fake_broker() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut b = [0u8; 64];
            let _ = s.read(&mut b).await;
            s.write_all(&[0x20, 0x02, 0x00, 0x00]).await.unwrap();
        });
        let cfg = MqttCheck {
            name: None,
            port,
            username: None,
            password: None,
            client_id: None,
            timeout_seconds: 3,
        };
        assert!(matches!(
            check_mqtt("127.0.0.1", &cfg).await.status,
            CheckStatus::Healthy
        ));
    }

    #[tokio::test]
    async fn smtp_against_fake_server() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            let (rd, mut wr) = s.into_split();
            let mut rd = BufReader::new(rd);
            wr.write_all(b"220 mail.test ESMTP ready\r\n")
                .await
                .unwrap();
            let mut line = String::new();
            rd.read_line(&mut line).await.unwrap();
            wr.write_all(b"250-mail.test\r\n250 OK\r\n").await.unwrap();
        });
        let cfg = SmtpCheck {
            name: None,
            port,
            expect_banner: Some("ESMTP".into()),
            timeout_seconds: 3,
        };
        assert!(matches!(
            check_smtp("127.0.0.1", &cfg).await.status,
            CheckStatus::Healthy
        ));
    }
}
