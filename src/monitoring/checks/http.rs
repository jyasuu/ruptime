use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use log::{info, warn};
use crate::config::{HttpCheck, HttpProtocol, HttpMethod as ConfigHttpMethod, AuthConfig};
use crate::monitoring::types::{HttpTargetCheckResult, CheckStatus};
use crate::monitoring::assertions::evaluate_assertions_with_data;
use crate::monitoring::auth::get_oauth2_token;

// ─── Low-level TLS (Unix only) ──────────────────────────────────────────────
//
// All opaque OpenSSL types are represented as *mut/*const c_void so the extern
// "C" signatures are identical to those in auth.rs and database.rs.  This
// avoids the clashing_extern_declarations warnings that arise when the same
// symbol is declared with different (but ABI-compatible) pointer types.

#[cfg(not(target_os = "windows"))]
mod tls_ffi {
    use std::ffi::CString;
    use std::net::TcpStream;
    use std::os::fd::FromRawFd;
    use std::os::raw::{c_int, c_long, c_void};
    use std::os::unix::io::IntoRawFd;

    pub const SSL_MODE_RELEASE_BUFFERS: c_long = 0x00000010;
    pub const SSL_CTRL_SET_TLSEXT_HOSTNAME: c_int = 55;
    pub const SSL_CTRL_MODE: c_int = 33;

    // All pointer types use c_void — same as auth.rs — to avoid duplicate
    // extern symbol warnings.
    #[link(name = "ssl")]
    #[link(name = "crypto")]
    extern "C" {
        pub fn TLS_client_method() -> *const c_void;
        pub fn SSL_CTX_new(method: *const c_void) -> *mut c_void;
        pub fn SSL_CTX_free(ctx: *mut c_void);
        pub fn SSL_CTX_ctrl(ctx: *mut c_void, cmd: c_int, larg: c_long, parg: *mut c_void) -> c_long;
        pub fn SSL_new(ctx: *mut c_void) -> *mut c_void;
        pub fn SSL_free(ssl: *mut c_void);
        pub fn SSL_set_fd(ssl: *mut c_void, fd: c_int) -> c_int;
        pub fn SSL_connect(ssl: *mut c_void) -> c_int;
        pub fn SSL_read(ssl: *mut c_void, buf: *mut c_void, num: c_int) -> c_int;
        pub fn SSL_write(ssl: *mut c_void, buf: *const c_void, num: c_int) -> c_int;
        pub fn SSL_ctrl(ssl: *mut c_void, cmd: c_int, larg: c_long, parg: *mut c_void) -> c_long;
        pub fn SSL_CTX_set_verify(ctx: *mut c_void, mode: c_int, cb: *const c_void);
        pub fn SSL_CTX_set_default_verify_paths(ctx: *mut c_void) -> c_int;
        pub fn SSL_get_error(ssl: *const c_void, ret: c_int) -> c_int;

        // Certificate access — OpenSSL 3.x names (borrowed refs, no free needed).
        // SSL_get0_peer_certificate returns a borrowed X509*; caller must NOT free it.
        pub fn SSL_get0_peer_certificate(ssl: *const c_void) -> *const c_void;
        // X509_get0_notAfter returns a borrowed ASN1_TIME*; caller must NOT free it.
        pub fn X509_get0_notAfter(cert: *const c_void) -> *const c_void;
        // ASN1_TIME_diff: diff = to - from, result in days+seconds.
        pub fn ASN1_TIME_diff(
            pday: *mut c_int,
            psec: *mut c_int,
            from: *const c_void,
            to: *const c_void,
        ) -> c_int;
        // X509_gmtime_adj(NULL, 0) returns a newly-allocated ASN1_TIME for "now".
        pub fn X509_gmtime_adj(s: *mut c_void, adj: c_long) -> *mut c_void;
        pub fn ASN1_TIME_free(t: *mut c_void);
    }

    pub struct TlsStream {
        pub ssl: *mut c_void,
        pub ctx: *mut c_void,
        _tcp: std::mem::ManuallyDrop<TcpStream>,
    }

    impl TlsStream {
        pub fn new(tcp: TcpStream, hostname: &str, verify_cert: bool) -> Result<Self, String> {
            unsafe {
                let method = TLS_client_method();
                if method.is_null() { return Err("TLS_client_method failed".into()); }

                let ctx = SSL_CTX_new(method);
                if ctx.is_null() { return Err("SSL_CTX_new failed".into()); }

                SSL_CTX_set_default_verify_paths(ctx);
                SSL_CTX_set_verify(ctx, if verify_cert { 1 } else { 0 }, std::ptr::null());
                SSL_CTX_ctrl(ctx, SSL_CTRL_MODE, SSL_MODE_RELEASE_BUFFERS, std::ptr::null_mut());

                let ssl = SSL_new(ctx);
                if ssl.is_null() {
                    SSL_CTX_free(ctx);
                    return Err("SSL_new failed".into());
                }

                let chost = CString::new(hostname).map_err(|e| e.to_string())?;
                SSL_ctrl(ssl, SSL_CTRL_SET_TLSEXT_HOSTNAME, 0, chost.as_ptr() as *mut c_void);

                let fd = tcp.into_raw_fd();
                SSL_set_fd(ssl, fd);

                let ret = SSL_connect(ssl);
                if ret != 1 {
                    let err = SSL_get_error(ssl, ret);
                    SSL_free(ssl);
                    SSL_CTX_free(ctx);
                    drop(std::os::unix::io::OwnedFd::from_raw_fd(fd));
                    return Err(format!("SSL_connect failed, error code: {}", err));
                }

                let tcp_back: TcpStream = std::os::unix::io::FromRawFd::from_raw_fd(fd);
                Ok(TlsStream { ssl, ctx, _tcp: std::mem::ManuallyDrop::new(tcp_back) })
            }
        }

        /// Returns the number of days until cert expiry (negative = already expired).
        /// Uses OpenSSL 3.x borrowed-reference API (no free required).
        pub fn cert_expiry_days(&self) -> Option<i64> {
            unsafe {
                // Borrowed ref — do NOT call X509_free on it.
                let cert = SSL_get0_peer_certificate(self.ssl);
                if cert.is_null() { return None; }

                // Borrowed ref — do NOT call ASN1_TIME_free on it.
                let not_after = X509_get0_notAfter(cert);
                if not_after.is_null() { return None; }

                // Allocate an ASN1_TIME for "now" — this one we must free.
                let now = X509_gmtime_adj(std::ptr::null_mut(), 0);
                if now.is_null() { return None; }

                let mut pday: std::os::raw::c_int = 0;
                let mut psec: std::os::raw::c_int = 0;
                // diff = not_after − now
                let ok = ASN1_TIME_diff(&mut pday, &mut psec, now, not_after);
                ASN1_TIME_free(now);

                if ok == 0 { None } else { Some(pday as i64) }
            }
        }

        pub fn write_all(&mut self, data: &[u8]) -> Result<(), String> {
            let mut written = 0;
            while written < data.len() {
                let ret = unsafe {
                    SSL_write(
                        self.ssl,
                        data[written..].as_ptr() as *const c_void,
                        (data.len() - written) as std::os::raw::c_int,
                    )
                };
                if ret <= 0 { return Err(format!("SSL_write failed: {}", ret)); }
                written += ret as usize;
            }
            Ok(())
        }

        pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
            let ret = unsafe {
                SSL_read(
                    self.ssl,
                    buf.as_mut_ptr() as *mut c_void,
                    buf.len() as std::os::raw::c_int,
                )
            };
            if ret < 0 { Err(format!("SSL_read failed: {}", ret)) } else { Ok(ret as usize) }
        }
    }

    impl Drop for TlsStream {
        fn drop(&mut self) {
            unsafe {
                SSL_free(self.ssl);
                SSL_CTX_free(self.ctx);
                std::mem::ManuallyDrop::drop(&mut self._tcp);
            }
        }
    }
}

// ─── Response helpers ────────────────────────────────────────────────────────

fn parse_status_code(header_buf: &[u8]) -> u16 {
    String::from_utf8_lossy(header_buf)
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn parse_headers(header_buf: &[u8]) -> http::HeaderMap {
    let mut map = http::HeaderMap::new();
    let text = String::from_utf8_lossy(header_buf);
    for line in text.lines().skip(1) {
        if line.is_empty() { break; }
        if let Some(colon) = line.find(':') {
            let name  = line[..colon].trim();
            let value = line[colon + 1..].trim();
            if let (Ok(hn), Ok(hv)) = (
                http::header::HeaderName::from_bytes(name.as_bytes()),
                http::header::HeaderValue::from_str(value),
            ) {
                map.insert(hn, hv);
            }
        }
    }
    map
}

// ─── Raw HTTP/HTTPS request ───────────────────────────────────────────────────

struct RawResponse {
    status_code: u16,
    headers: http::HeaderMap,
    body: String,
}

fn do_http_request(
    address: &str,
    port: u16,
    path: &str,
    method: &str,
    extra_headers: &[(String, String)],
    timeout: Duration,
    is_https: bool,
    accept_invalid_certs: bool,
) -> Result<RawResponse, String> {
    let addr_str = format!("{}:{}", address, port);
    let addrs: Vec<_> = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("DNS lookup failed: {}", e))?
        .collect();
    if addrs.is_empty() {
        return Err("No addresses resolved".into());
    }

    let tcp = TcpStream::connect_timeout(&addrs[0], timeout)
        .map_err(|e| format!("TCP connect failed: {}", e))?;
    tcp.set_read_timeout(Some(timeout))
        .map_err(|e| format!("set_read_timeout failed: {}", e))?;
    tcp.set_nodelay(true).ok();

    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: ruptime/1.0\r\nAccept: */*\r\nConnection: close\r\n",
        method, path, address
    );
    for (k, v) in extra_headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");

    let full_response: Vec<u8> = if is_https {
        #[cfg(not(target_os = "windows"))]
        {
            let mut tls = tls_ffi::TlsStream::new(tcp, address, !accept_invalid_certs)
                .map_err(|e| format!("TLS handshake failed: {}", e))?;
            tls.write_all(req.as_bytes())?;
            let mut buf = Vec::with_capacity(32 * 1024);
            let mut tmp = [0u8; 16 * 1024];
            loop {
                let n = tls.read(&mut tmp)?;
                if n == 0 { break; }
                buf.extend_from_slice(&tmp[..n]);
            }
            buf
        }
        #[cfg(target_os = "windows")]
        return Err("HTTPS on Windows requires native-tls (not available in zero-dep mode)".into());
    } else {
        let mut tcp = tcp;
        tcp.write_all(req.as_bytes())
            .map_err(|e| format!("Write failed: {}", e))?;
        let mut buf = Vec::with_capacity(32 * 1024);
        let mut tmp = [0u8; 16 * 1024];
        loop {
            let n = tcp.read(&mut tmp).unwrap_or(0);
            if n == 0 { break; }
            buf.extend_from_slice(&tmp[..n]);
        }
        buf
    };

    let split_pos = full_response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(full_response.len());

    let header_buf = &full_response[..split_pos];
    let body_buf   = if split_pos + 4 <= full_response.len() { &full_response[split_pos + 4..] } else { &[] };

    Ok(RawResponse {
        status_code: parse_status_code(header_buf),
        headers:     parse_headers(header_buf),
        body:        String::from_utf8_lossy(body_buf).into_owned(),
    })
}

// ─── SSL certificate probe ────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
fn probe_ssl_cert(address: &str, port: u16) -> (Option<i64>, Option<bool>) {
    let addr_str = format!("{}:{}", address, port);
    let addrs: Vec<_> = match addr_str.to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(e) => {
            warn!("DNS lookup failed for SSL cert probe on {}: {}", address, e);
            return (None, Some(false));
        }
    };
    if addrs.is_empty() { return (None, Some(false)); }

    let tcp = match TcpStream::connect_timeout(&addrs[0], Duration::from_secs(5)) {
        Ok(s) => s,
        Err(e) => {
            warn!("TCP connect failed for SSL cert probe on {}:{}: {}", address, port, e);
            return (None, Some(false));
        }
    };

    // verify_cert = false so we always get the cert even for self-signed
    match tls_ffi::TlsStream::new(tcp, address, false) {
        Ok(tls) => match tls.cert_expiry_days() {
            Some(days) => {
                info!("SSL cert for {}: {} days remaining, valid: {}", address, days, days > 0);
                (Some(days), Some(days > 0))
            }
            None => {
                warn!("Could not extract certificate from TLS stream for {}", address);
                (None, Some(false))
            }
        },
        Err(e) => {
            warn!("TLS connection for cert probe failed for {}:{}: {}", address, port, e);
            (None, Some(false))
        }
    }
}

#[cfg(target_os = "windows")]
fn probe_ssl_cert(_address: &str, _port: u16) -> (Option<i64>, Option<bool>) {
    (None, None)
}

// ─── HTTP method mapping ──────────────────────────────────────────────────────

fn method_str(method: &ConfigHttpMethod) -> &'static str {
    match method {
        ConfigHttpMethod::Get     => "GET",
        ConfigHttpMethod::Post    => "POST",
        ConfigHttpMethod::Put     => "PUT",
        ConfigHttpMethod::Delete  => "DELETE",
        ConfigHttpMethod::Head    => "HEAD",
        ConfigHttpMethod::Options => "OPTIONS",
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

pub async fn check_http_target(
    address: &str,
    http_check_config: &HttpCheck,
) -> HttpTargetCheckResult {
    let start_time = Instant::now();

    // 1. SSL certificate probe
    let (cert_days_remaining, cert_is_valid) = if http_check_config.protocol == HttpProtocol::Https {
        info!("Probing SSL certificate for {} on port {}", address, http_check_config.port);
        probe_ssl_cert(address, http_check_config.port)
    } else {
        (None, None)
    };

    // 2. Build extra headers (custom + auth)
    let mut extra_headers: Vec<(String, String)> = Vec::new();

    if let Some(headers) = &http_check_config.headers {
        for (k, v) in headers {
            extra_headers.push((k.clone(), v.clone()));
        }
    }

    if let Some(auth) = &http_check_config.auth {
        match auth {
            AuthConfig::Basic { username, password } => {
                use base64::Engine as _;
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", username, password));
                extra_headers.push(("Authorization".into(), format!("Basic {}", encoded)));
            }
            AuthConfig::Bearer { token } => {
                extra_headers.push(("Authorization".into(), format!("Bearer {}", token)));
            }
            AuthConfig::OAuth2 { client_id, client_secret, token_url } => {
                match get_oauth2_token(client_id, client_secret, token_url).await {
                    Ok(token) => {
                        extra_headers.push(("Authorization".into(), format!("Bearer {}", token)));
                    }
                    Err(e) => {
                        return HttpTargetCheckResult {
                            status: CheckStatus::Unhealthy(format!("OAuth2 authentication failed: {}", e)),
                            response_time_ms: start_time.elapsed().as_millis(),
                            cert_days_remaining,
                            cert_is_valid,
                        };
                    }
                }
            }
        }
    }

    // 3. Perform the request
    let is_https       = http_check_config.protocol == HttpProtocol::Https;
    let accept_invalid = is_https && !http_check_config.check_ssl_certificate;

    let response = match do_http_request(
        address,
        http_check_config.port,
        &http_check_config.path,
        method_str(&http_check_config.method),
        &extra_headers,
        Duration::from_secs(http_check_config.timeout_seconds),
        is_https,
        accept_invalid,
    ) {
        Ok(r) => r,
        Err(e) => {
            return HttpTargetCheckResult {
                status: CheckStatus::Unhealthy(format!(
                    "Request to {}://{}:{}{} failed: {}",
                    if is_https { "https" } else { "http" },
                    address, http_check_config.port, http_check_config.path, e
                )),
                response_time_ms: start_time.elapsed().as_millis(),
                cert_days_remaining,
                cert_is_valid,
            };
        }
    };

    let response_time_ms = start_time.elapsed().as_millis();

    // 4. Status code check
    if response.status_code != http_check_config.expected_status_code {
        return HttpTargetCheckResult {
            status: CheckStatus::Unhealthy(format!(
                "Unexpected status code: {} (expected {})",
                response.status_code, http_check_config.expected_status_code
            )),
            response_time_ms,
            cert_days_remaining,
            cert_is_valid,
        };
    }

    // 5. Body regex check
    if let Some(regex_pattern) = &http_check_config.body_regex_check {
        match regex::Regex::new(regex_pattern) {
            Ok(re) => {
                if !re.is_match(&response.body) {
                    return HttpTargetCheckResult {
                        status: CheckStatus::Unhealthy(format!(
                            "Response body does not match regex pattern: '{}'",
                            regex_pattern
                        )),
                        response_time_ms,
                        cert_days_remaining,
                        cert_is_valid,
                    };
                }
            }
            Err(e) => {
                return HttpTargetCheckResult {
                    status: CheckStatus::Unhealthy(format!(
                        "Invalid regex pattern: '{}' - {}", regex_pattern, e
                    )),
                    response_time_ms,
                    cert_days_remaining,
                    cert_is_valid,
                };
            }
        }
    }

    // 6. Assertions
    if let Some(assertions) = &http_check_config.assertions {
        let http_status = http::StatusCode::from_u16(response.status_code)
            .unwrap_or(http::StatusCode::OK);

        let assertion_results = evaluate_assertions_with_data(
            assertions,
            http_status,
            &response.headers,
            &response.body,
            response_time_ms,
            None,
        );

        let failed: Vec<_> = assertion_results.iter().filter(|r| !r.passed).collect();
        if !failed.is_empty() {
            let msgs: Vec<String> = failed.iter().map(|r| r.message.clone()).collect();
            return HttpTargetCheckResult {
                status: CheckStatus::Unhealthy(format!("Assertion failures: {}", msgs.join("; "))),
                response_time_ms,
                cert_days_remaining,
                cert_is_valid,
            };
        }
    }

    HttpTargetCheckResult {
        status: CheckStatus::Healthy,
        response_time_ms,
        cert_days_remaining,
        cert_is_valid,
    }
}