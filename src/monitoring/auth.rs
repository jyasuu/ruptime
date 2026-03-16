use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

// Simple in-memory cache for OAuth2 tokens
static OAUTH2_TOKEN_CACHE: std::sync::LazyLock<Mutex<HashMap<String, (String, SystemTime)>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Perform a raw HTTP/HTTPS POST to `url` with `application/x-www-form-urlencoded` body.
/// Returns the full response body as a String.
fn raw_http_post(url: &str, body: &str) -> Result<String, String> {
    // Parse URL
    let (is_https, host, port, path) = parse_url(url)?;

    let addr_str = format!("{}:{}", host, port);
    let addrs: Vec<_> = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("DNS lookup failed for {}: {}", addr_str, e))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("No addresses resolved for {}", addr_str));
    }

    let tcp = TcpStream::connect_timeout(&addrs[0], Duration::from_secs(10))
        .map_err(|e| format!("TCP connect failed: {}", e))?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("set_read_timeout: {}", e))?;
    tcp.set_nodelay(true).ok();

    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path, host, body.len(), body
    );

    let full_response = if is_https {
        raw_https_exchange(tcp, &host, &request)?
    } else {
        let mut tcp = tcp;
        tcp.write_all(request.as_bytes())
            .map_err(|e| format!("Write failed: {}", e))?;
        let mut buf = Vec::with_capacity(16 * 1024);
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

    let response_str = String::from_utf8_lossy(&full_response).into_owned();
    Ok(response_str)
}

fn parse_url(url: &str) -> Result<(bool, String, u16, String), String> {
    let (is_https, rest) = if url.starts_with("https://") {
        (true, &url[8..])
    } else if url.starts_with("http://") {
        (false, &url[7..])
    } else {
        return Err(format!("URL must start with http:// or https://: {}", url));
    };

    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };

    let (host, port) = match host_port.rfind(':') {
        Some(i) => {
            let p: u16 = host_port[i + 1..].parse().map_err(|_| "Invalid port")?;
            (host_port[..i].to_string(), p)
        }
        None => (host_port.to_string(), if is_https { 443 } else { 80 }),
    };

    Ok((is_https, host, port, path))
}

#[cfg(not(target_os = "windows"))]
fn raw_https_exchange(tcp: TcpStream, host: &str, request: &str) -> Result<Vec<u8>, String> {
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
        SSL_CTX_set_verify(ctx, 1, std::ptr::null());
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
            let owned = std::os::unix::io::OwnedFd::from_raw_fd(fd);
            drop(owned);
            return Err(format!("SSL_connect failed: {}", err));
        }

        // Write
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

        // Read
        let mut buf = Vec::with_capacity(16 * 1024);
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

        // restore TcpStream to drop it properly
        let _tcp_back: TcpStream = std::os::unix::io::FromRawFd::from_raw_fd(fd);

        Ok(buf)
    }
}

#[cfg(target_os = "windows")]
fn raw_https_exchange(_tcp: TcpStream, _host: &str, _request: &str) -> Result<Vec<u8>, String> {
    Err("HTTPS on Windows requires native-tls".into())
}

/// Extract the response body (after \r\n\r\n) from a raw HTTP response string.
fn extract_body(response: &str) -> &str {
    if let Some(pos) = response.find("\r\n\r\n") {
        &response[pos + 4..]
    } else {
        response
    }
}

/// Extract HTTP status code from first line.
fn parse_status(response: &str) -> u16 {
    response
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// URL-encode a list of key-value pairs.
fn url_encode_form(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

pub async fn get_oauth2_token(
    client_id: &str,
    client_secret: &str,
    token_url: &str,
) -> Result<String, String> {
    let cache_key = format!("{}:{}", client_id, token_url);

    // Check cache first
    {
        let cache = OAUTH2_TOKEN_CACHE.lock().unwrap();
        if let Some((token, timestamp)) = cache.get(&cache_key) {
            if timestamp.elapsed().unwrap_or(Duration::from_secs(3900)) < Duration::from_secs(3300)
            {
                return Ok(token.clone());
            }
        }
    }

    // Build form body
    let body = url_encode_form(&[
        ("grant_type", "client_credentials"),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ]);

    let response_str = raw_http_post(token_url, &body)?;
    let status = parse_status(&response_str);

    if status / 100 != 2 {
        return Err(format!(
            "OAuth2 token request failed with status: {}",
            status
        ));
    }

    let body_str = extract_body(&response_str);
    let json: serde_json::Value = serde_json::from_str(body_str)
        .map_err(|e| format!("Failed to parse OAuth2 response: {}", e))?;

    let access_token = json
        .get("access_token")
        .and_then(|v: &serde_json::Value| v.as_str())
        .ok_or_else(|| "No access_token in OAuth2 response".to_string())?;

    // Cache it
    {
        let mut cache = OAUTH2_TOKEN_CACHE.lock().unwrap();
        cache.insert(cache_key, (access_token.to_string(), SystemTime::now()));
    }

    Ok(access_token.to_string())
}
