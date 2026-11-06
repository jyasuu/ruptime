use crate::config::{AuthConfig, HttpCheck, HttpMethod as ConfigHttpMethod, HttpProtocol};
use crate::monitoring::assertions::evaluate_assertions_with_data;
use crate::monitoring::auth::get_oauth2_token;
use crate::monitoring::types::{CheckStatus, HttpTargetCheckResult};
use log::{info, warn};
#[cfg(not(target_os = "windows"))]
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

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
        pub fn SSL_CTX_ctrl(
            ctx: *mut c_void,
            cmd: c_int,
            larg: c_long,
            parg: *mut c_void,
        ) -> c_long;
        pub fn SSL_new(ctx: *mut c_void) -> *mut c_void;
        pub fn SSL_free(ssl: *mut c_void);
        pub fn SSL_set_fd(ssl: *mut c_void, fd: c_int) -> c_int;
        pub fn SSL_connect(ssl: *mut c_void) -> c_int;
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
                if method.is_null() {
                    return Err("TLS_client_method failed".into());
                }

                let ctx = SSL_CTX_new(method);
                if ctx.is_null() {
                    return Err("SSL_CTX_new failed".into());
                }

                SSL_CTX_set_default_verify_paths(ctx);
                SSL_CTX_set_verify(ctx, if verify_cert { 1 } else { 0 }, std::ptr::null());
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

                let chost = CString::new(hostname).map_err(|e| e.to_string())?;
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
                    return Err(format!("SSL_connect failed, error code: {}", err));
                }

                let tcp_back: TcpStream = std::os::unix::io::FromRawFd::from_raw_fd(fd);
                Ok(TlsStream {
                    ssl,
                    ctx,
                    _tcp: std::mem::ManuallyDrop::new(tcp_back),
                })
            }
        }

        /// Returns the number of days until cert expiry (negative = already expired).
        /// Uses OpenSSL 3.x borrowed-reference API (no free required).
        pub fn cert_expiry_days(&self) -> Option<i64> {
            unsafe {
                // Borrowed ref — do NOT call X509_free on it.
                let cert = SSL_get0_peer_certificate(self.ssl);
                if cert.is_null() {
                    return None;
                }

                // Borrowed ref — do NOT call ASN1_TIME_free on it.
                let not_after = X509_get0_notAfter(cert);
                if not_after.is_null() {
                    return None;
                }

                // Allocate an ASN1_TIME for "now" — this one we must free.
                let now = X509_gmtime_adj(std::ptr::null_mut(), 0);
                if now.is_null() {
                    return None;
                }

                let mut pday: std::os::raw::c_int = 0;
                let mut psec: std::os::raw::c_int = 0;
                // diff = not_after − now
                let ok = ASN1_TIME_diff(&mut pday, &mut psec, now, not_after);
                ASN1_TIME_free(now);

                if ok == 0 {
                    None
                } else {
                    Some(pday as i64)
                }
            }
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
    if addrs.is_empty() {
        return (None, Some(false));
    }

    let tcp = match TcpStream::connect_timeout(&addrs[0], Duration::from_secs(5)) {
        Ok(s) => s,
        Err(e) => {
            warn!(
                "TCP connect failed for SSL cert probe on {}:{}: {}",
                address, port, e
            );
            return (None, Some(false));
        }
    };

    // verify_cert = false so we always get the cert even for self-signed
    match tls_ffi::TlsStream::new(tcp, address, false) {
        Ok(tls) => match tls.cert_expiry_days() {
            Some(days) => {
                info!(
                    "SSL cert for {}: {} days remaining, valid: {}",
                    address,
                    days,
                    days > 0
                );
                (Some(days), Some(days > 0))
            }
            None => {
                warn!(
                    "Could not extract certificate from TLS stream for {}",
                    address
                );
                (None, Some(false))
            }
        },
        Err(e) => {
            warn!(
                "TLS connection for cert probe failed for {}:{}: {}",
                address, port, e
            );
            (None, Some(false))
        }
    }
}

#[cfg(target_os = "windows")]
fn probe_ssl_cert(_address: &str, _port: u16) -> (Option<i64>, Option<bool>) {
    (None, None)
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Returns true when `code` matches one of the accepted specs ("200", "200-299").
pub(crate) fn status_accepted(code: u16, specs: &[String]) -> Result<bool, String> {
    for spec in specs {
        let spec = spec.trim();
        if let Some((lo, hi)) = spec.split_once('-') {
            let lo: u16 = lo
                .trim()
                .parse()
                .map_err(|_| format!("Invalid status range '{}'", spec))?;
            let hi: u16 = hi
                .trim()
                .parse()
                .map_err(|_| format!("Invalid status range '{}'", spec))?;
            if (lo..=hi).contains(&code) {
                return Ok(true);
            }
        } else {
            let one: u16 = spec
                .parse()
                .map_err(|_| format!("Invalid status code '{}'", spec))?;
            if one == code {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(crate) fn keyword_found(body: &str, keyword: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        body.contains(keyword)
    } else {
        body.to_lowercase().contains(&keyword.to_lowercase())
    }
}

fn method_of(method: &ConfigHttpMethod) -> reqwest::Method {
    match method {
        ConfigHttpMethod::Get => reqwest::Method::GET,
        ConfigHttpMethod::Post => reqwest::Method::POST,
        ConfigHttpMethod::Put => reqwest::Method::PUT,
        ConfigHttpMethod::Delete => reqwest::Method::DELETE,
        ConfigHttpMethod::Head => reqwest::Method::HEAD,
        ConfigHttpMethod::Options => reqwest::Method::OPTIONS,
    }
}

fn build_url(address: &str, cfg: &HttpCheck) -> String {
    let scheme = if cfg.protocol == HttpProtocol::Https {
        "https"
    } else {
        "http"
    };
    let host = if address.contains(':') && !address.starts_with('[') {
        format!("[{}]", address) // bare IPv6
    } else {
        address.to_string()
    };
    let path = if cfg.path.starts_with('/') {
        cfg.path.clone()
    } else {
        format!("/{}", cfg.path)
    };
    format!("{}://{}:{}{}", scheme, host, cfg.port, path)
}

fn build_client(cfg: &HttpCheck) -> Result<reqwest::Client, String> {
    let mut b = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(cfg.timeout_seconds))
        .user_agent(
            cfg.user_agent
                .clone()
                .unwrap_or_else(|| "ruptime/1.0".to_string()),
        )
        .danger_accept_invalid_certs(cfg.ignore_tls_errors)
        .redirect(if cfg.follow_redirects {
            reqwest::redirect::Policy::limited(cfg.max_redirects)
        } else {
            reqwest::redirect::Policy::none()
        });

    if let Some(p) = &cfg.proxy {
        let mut proxy =
            reqwest::Proxy::all(&p.url).map_err(|e| format!("Invalid proxy '{}': {}", p.url, e))?;
        if let (Some(u), Some(pw)) = (&p.username, &p.password) {
            proxy = proxy.basic_auth(u, pw);
        }
        b = b.proxy(proxy);
    }
    if let Some(path) = &cfg.ca_cert_path {
        let pem =
            std::fs::read(path).map_err(|e| format!("Cannot read CA file '{}': {}", path, e))?;
        let cert = reqwest::Certificate::from_pem(&pem)
            .map_err(|e| format!("Invalid CA file '{}': {}", path, e))?;
        b = b.add_root_certificate(cert);
    }
    if let Some(path) = &cfg.client_identity_path {
        let pem = std::fs::read(path)
            .map_err(|e| format!("Cannot read client identity '{}': {}", path, e))?;
        let id = reqwest::Identity::from_pem(&pem)
            .map_err(|e| format!("Invalid client identity '{}': {}", path, e))?;
        b = b.identity(id);
    }
    b.build()
        .map_err(|e| format!("HTTP client setup failed: {}", e))
}

/// reqwest 0.11 uses `http` 0.2; the assertion engine uses `http` 1.x.
fn convert_headers(h: &reqwest::header::HeaderMap) -> http::HeaderMap {
    let mut out = http::HeaderMap::new();
    for (k, v) in h.iter() {
        if let (Ok(name), Ok(val)) = (
            http::header::HeaderName::from_bytes(k.as_str().as_bytes()),
            http::header::HeaderValue::from_bytes(v.as_bytes()),
        ) {
            out.append(name, val);
        }
    }
    out
}

// ─── Public entry point ───────────────────────────────────────────────────────

pub async fn check_http_target(address: &str, cfg: &HttpCheck) -> HttpTargetCheckResult {
    let start_time = Instant::now();

    // 1. SSL certificate expiry probe (blocking OpenSSL FFI → off the async workers)
    let (cert_days_remaining, cert_is_valid) =
        if cfg.protocol == HttpProtocol::Https && cfg.check_ssl_certificate {
            info!(
                "Probing SSL certificate for {} on port {}",
                address, cfg.port
            );
            let (a, p) = (address.to_string(), cfg.port);
            tokio::task::spawn_blocking(move || probe_ssl_cert(&a, p))
                .await
                .unwrap_or((None, None))
        } else {
            (None, None)
        };

    let (status, response_time_ms) = evaluate_request(address, cfg, start_time).await;

    // Upside-down mode: DOWN when the check passes, UP when it fails.
    let status = if cfg.upside_down {
        match status {
            CheckStatus::Healthy => CheckStatus::Unhealthy(
                "Upside-down mode: target responded successfully but was expected to fail".into(),
            ),
            CheckStatus::Unhealthy(_) => CheckStatus::Healthy,
        }
    } else {
        status
    };

    HttpTargetCheckResult {
        status,
        response_time_ms,
        cert_days_remaining,
        cert_is_valid,
    }
}

async fn evaluate_request(address: &str, cfg: &HttpCheck, start: Instant) -> (CheckStatus, u128) {
    let elapsed = || start.elapsed().as_millis();
    let bad = |m: String| (CheckStatus::Unhealthy(m), start.elapsed().as_millis());

    let client = match build_client(cfg) {
        Ok(c) => c,
        Err(e) => return bad(e),
    };
    let url = build_url(address, cfg);
    let mut req = client.request(method_of(&cfg.method), &url);

    if let Some(headers) = &cfg.headers {
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
    }
    match &cfg.auth {
        Some(AuthConfig::Basic { username, password }) => {
            req = req.basic_auth(username, Some(password));
        }
        Some(AuthConfig::Bearer { token }) => {
            req = req.bearer_auth(token);
        }
        Some(AuthConfig::OAuth2 {
            client_id,
            client_secret,
            token_url,
        }) => match get_oauth2_token(client_id, client_secret, token_url).await {
            Ok(token) => req = req.bearer_auth(token),
            Err(e) => return bad(format!("OAuth2 authentication failed: {}", e)),
        },
        None => {}
    }
    if let Some(body) = &cfg.body {
        req = req.body(body.clone());
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let kind = if e.is_timeout() {
                format!("timed out after {}s", cfg.timeout_seconds)
            } else if e.is_redirect() {
                format!("too many redirects (max {})", cfg.max_redirects)
            } else {
                e.to_string()
            };
            return bad(format!("Request to {} failed: {}", url, kind));
        }
    };

    let status_code = resp.status().as_u16();
    let headers = convert_headers(resp.headers());
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => return bad(format!("Failed reading response body from {}: {}", url, e)),
    };
    let response_time_ms = elapsed();
    let unhealthy = |m: String| (CheckStatus::Unhealthy(m), response_time_ms);

    // Status code
    if cfg.accepted_status_codes.is_empty() {
        if status_code != cfg.expected_status_code {
            return unhealthy(format!(
                "Unexpected status code: {} (expected {})",
                status_code, cfg.expected_status_code
            ));
        }
    } else {
        match status_accepted(status_code, &cfg.accepted_status_codes) {
            Ok(true) => {}
            Ok(false) => {
                return unhealthy(format!(
                    "Unexpected status code: {} (accepted: {})",
                    status_code,
                    cfg.accepted_status_codes.join(", ")
                ))
            }
            Err(e) => return unhealthy(e),
        }
    }

    // Keyword
    if let Some(keyword) = &cfg.keyword {
        let found = keyword_found(&body, keyword, cfg.keyword_case_sensitive);
        if cfg.invert_keyword && found {
            return unhealthy(format!(
                "Response body contains forbidden keyword '{}'",
                keyword
            ));
        }
        if !cfg.invert_keyword && !found {
            return unhealthy(format!(
                "Response body does not contain keyword '{}'",
                keyword
            ));
        }
    }

    // Body regex
    if let Some(pattern) = &cfg.body_regex_check {
        match regex::Regex::new(pattern) {
            Ok(re) if re.is_match(&body) => {}
            Ok(_) => {
                return unhealthy(format!(
                    "Response body does not match regex pattern: '{}'",
                    pattern
                ))
            }
            Err(e) => return unhealthy(format!("Invalid regex pattern: '{}' - {}", pattern, e)),
        }
    }

    // Assertions
    if let Some(assertions) = &cfg.assertions {
        let http_status = http::StatusCode::from_u16(status_code).unwrap_or(http::StatusCode::OK);
        let results = evaluate_assertions_with_data(
            assertions,
            http_status,
            &headers,
            &body,
            response_time_ms,
            None,
        );
        let failed: Vec<String> = results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.message.clone())
            .collect();
        if !failed.is_empty() {
            return unhealthy(format!("Assertion failures: {}", failed.join("; ")));
        }
    }

    (CheckStatus::Healthy, response_time_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_ranges() {
        let specs = vec!["200-299".to_string(), "301".to_string()];
        assert_eq!(status_accepted(204, &specs), Ok(true));
        assert_eq!(status_accepted(301, &specs), Ok(true));
        assert_eq!(status_accepted(302, &specs), Ok(false));
        assert!(status_accepted(200, &["abc".to_string()]).is_err());
    }

    #[test]
    fn keyword_case() {
        assert!(keyword_found("Hello World", "World", true));
        assert!(!keyword_found("Hello World", "world", true));
        assert!(keyword_found("Hello World", "world", false));
    }

    #[test]
    fn url_building() {
        let mut c = HttpCheck::default();
        c.port = 8443;
        c.protocol = HttpProtocol::Https;
        c.path = "health".into();
        assert_eq!(
            build_url("example.com", &c),
            "https://example.com:8443/health"
        );
        assert_eq!(build_url("::1", &c), "https://[::1]:8443/health");
    }
}
