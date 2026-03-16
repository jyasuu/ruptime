use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::time::Instant;

#[cfg(not(target_os = "windows"))]
mod tls_ffi {
    use std::os::fd::FromRawFd;
    use std::ffi::CString;
    use std::os::raw::{c_int, c_long, c_void};
    use std::os::unix::io::IntoRawFd;
    use std::net::TcpStream;

    #[repr(C)]
    pub struct SSL_CTX(c_void);
    #[repr(C)]
    pub struct SSL(c_void);
    #[repr(C)]
    pub struct SSL_METHOD(c_void);

    #[link(name = "ssl")]
    #[link(name = "crypto")]
    extern "C" {
        pub fn TLS_client_method() -> *const SSL_METHOD;
        pub fn SSL_CTX_new(method: *const SSL_METHOD) -> *mut SSL_CTX;
        pub fn SSL_CTX_free(ctx: *mut SSL_CTX);
        pub fn SSL_new(ctx: *mut SSL_CTX) -> *mut SSL;
        pub fn SSL_free(ssl: *mut SSL);
        pub fn SSL_set_fd(ssl: *mut SSL, fd: c_int) -> c_int;
        pub fn SSL_connect(ssl: *mut SSL) -> c_int;
        pub fn SSL_read(ssl: *mut SSL, buf: *mut c_void, num: c_int) -> c_int;
        pub fn SSL_write(ssl: *mut SSL, buf: *const c_void, num: c_int) -> c_int;
        // SSL_set_tlsext_host_name is a macro over SSL_ctrl in OpenSSL headers
        pub fn SSL_ctrl(ssl: *mut SSL, cmd: c_int, larg: c_long, parg: *mut c_void) -> c_long;
        pub fn SSL_CTX_set_verify(ctx: *mut SSL_CTX, mode: c_int, cb: *const c_void);
        pub fn SSL_CTX_set_default_verify_paths(ctx: *mut SSL_CTX) -> c_int;
        pub fn SSL_get_error(ssl: *const SSL, ret: c_int) -> c_int;
    }

    pub struct TlsStream {
        pub ssl: *mut SSL,
        pub ctx: *mut SSL_CTX,
        // Keep the TcpStream alive so the fd isn't closed
        _tcp: std::mem::ManuallyDrop<TcpStream>,
    }

    impl TlsStream {
        pub fn new(tcp: TcpStream, hostname: &str) -> Result<Self, String> {
            unsafe {
                let method = TLS_client_method();
                if method.is_null() {
                    return Err("TLS_client_method failed".into());
                }
                let ctx = SSL_CTX_new(method);
                if ctx.is_null() {
                    return Err("SSL_CTX_new failed".into());
                }
                // Load system CA certs
                SSL_CTX_set_default_verify_paths(ctx);
                // Enable peer verification (mode=1)
                SSL_CTX_set_verify(ctx, 1, std::ptr::null());

                let ssl = SSL_new(ctx);
                if ssl.is_null() {
                    SSL_CTX_free(ctx);
                    return Err("SSL_new failed".into());
                }

                // SNI — SSL_set_tlsext_host_name is a macro: SSL_ctrl(ssl, 55, 0, hostname)
                // 55 = SSL_CTRL_SET_TLSEXT_HOSTNAME, TLSEXT_NAMETYPE_host_name = 0
                let chost = CString::new(hostname).map_err(|e| e.to_string())?;
                SSL_ctrl(ssl, 55, 0, chost.as_ptr() as *mut c_void);

                let fd = tcp.into_raw_fd();
                SSL_set_fd(ssl, fd);

                let ret = SSL_connect(ssl);
                if ret != 1 {
                    let err = SSL_get_error(ssl, ret);
                    SSL_free(ssl);
                    SSL_CTX_free(ctx);
                    // Close the fd properly
                    let owned = std::os::unix::io::OwnedFd::from_raw_fd(fd);
                    drop(owned);
                    return Err(format!("SSL_connect failed, error code: {}", err));
                }

                // Wrap fd back so it gets closed on drop
                let tcp_back: TcpStream = std::os::unix::io::FromRawFd::from_raw_fd(fd);

                Ok(TlsStream {
                    ssl,
                    ctx,
                    _tcp: std::mem::ManuallyDrop::new(tcp_back),
                })
            }
        }

        pub fn write_all(&mut self, data: &[u8]) -> Result<(), String> {
            let mut written = 0;
            while written < data.len() {
                let ret = unsafe {
                    SSL_write(
                        self.ssl,
                        data[written..].as_ptr() as *const c_void,
                        (data.len() - written) as c_int,
                    )
                };
                if ret <= 0 {
                    return Err(format!("SSL_write failed: {}", ret));
                }
                written += ret as usize;
            }
            Ok(())
        }

        pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
            let ret = unsafe {
                SSL_read(self.ssl, buf.as_mut_ptr() as *mut c_void, buf.len() as c_int)
            };
            if ret < 0 {
                Err(format!("SSL_read failed: {}", ret))
            } else {
                Ok(ret as usize)
            }
        }
    }

    impl Drop for TlsStream {
        fn drop(&mut self) {
            unsafe {
                SSL_free(self.ssl);
                SSL_CTX_free(self.ctx);
                // ManuallyDrop ensures TcpStream (and thus fd) is closed here
                std::mem::ManuallyDrop::drop(&mut self._tcp);
            }
        }
    }
}

// Returns (t_ssl, t_ttfb, t_total, response_buf)
#[cfg(not(target_os = "windows"))]
fn do_https(
    tcp: TcpStream,
    host: &str,
    request: &str,
    t_start: Instant,
) -> Result<(f64, f64, f64, Vec<u8>), String> {
    use tls_ffi::TlsStream;
    let mut tls = TlsStream::new(tcp, host)
        .map_err(|e| format!("TLS handshake failed: {}", e))?;
    let t_ssl = Instant::now().duration_since(t_start).as_secs_f64();

    tls.write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {}", e))?;

    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let mut first_byte = false;
    let mut ttfb_time = 0f64;

    // Read until end-of-headers, recording TTFB on first byte
    loop {
        let n = tls.read(&mut tmp).map_err(|e| format!("Read failed: {}", e))?;
        if n == 0 { break; }
        if !first_byte {
            ttfb_time = Instant::now().duration_since(t_start).as_secs_f64();
            first_byte = true;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    // Drain remaining body
    loop {
        let n = tls.read(&mut tmp).unwrap_or(0);
        if n == 0 { break; }
        buf.extend_from_slice(&tmp[..n]);
    }

    let t_total = Instant::now().duration_since(t_start).as_secs_f64();
    Ok((t_ssl, ttfb_time, t_total, buf))
}

#[cfg(target_os = "windows")]
fn do_https(
    _tcp: TcpStream,
    _host: &str,
    _request: &str,
    _t_start: Instant,
) -> Result<(f64, f64, f64, Vec<u8>), String> {
    Err("HTTPS on Windows requires native-tls crate (no zero-dep option)".into())
}


fn parse_url(url: &str) -> Result<(bool, String, u16, String), String> {
    let (is_https, rest) = if url.starts_with("https://") {
        (true, &url[8..])
    } else if url.starts_with("http://") {
        (false, &url[7..])
    } else {
        return Err("URL must start with http:// or https://".into());
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
        None => (
            host_port.to_string(),
            if is_https { 443 } else { 80 },
        ),
    };

    Ok((is_https, host, port, path))
}

struct Timings {
    dns_lookup: f64,
    connect: f64,
    ssl_handshake: f64,
    ttfb: f64,
    total: f64,
}

fn measure(url: &str, follow_redirects: bool) -> Result<(Timings, u16), String> {
    measure_inner(url, follow_redirects, 0)
}

fn measure_inner(url: &str, follow_redirects: bool, depth: usize) -> Result<(Timings, u16), String> {
    if depth > 10 {
        return Err("Too many redirects".into());
    }

    let (is_https, host, port, path) = parse_url(url)?;

    let t_start = Instant::now();

    // --- DNS lookup ---
    let addr_str = format!("{}:{}", host, port);
    let addrs: Vec<_> = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("DNS lookup failed: {}", e))?
        .collect();
    let t_dns = Instant::now();

    if addrs.is_empty() {
        return Err("No addresses resolved".into());
    }

    // --- TCP connect ---
    let tcp = TcpStream::connect(addrs[0])
        .map_err(|e| format!("TCP connect failed: {}", e))?;
    let t_connect = Instant::now();

    // Build HTTP request
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: http-timing/1.0\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        path, host
    );

    let t_ssl: f64;
    let t_ttfb: f64;
    let t_total: f64;
    let response_buf: Vec<u8>;

    if is_https {
        let result = do_https(tcp, &host, &request, t_start)?;
        t_ssl = result.0;
        t_ttfb = result.1;
        t_total = result.2;
        response_buf = result.3;
    } else {
        t_ssl = t_connect.duration_since(t_start).as_secs_f64(); // no SSL

        let mut tcp = tcp;
        tcp.write_all(request.as_bytes())
            .map_err(|e| format!("Write failed: {}", e))?;

        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        let mut first_byte = false;
        let mut ttfb_time = 0f64;

        loop {
            let n = tcp.read(&mut tmp).map_err(|e| format!("Read failed: {}", e))?;
            if n == 0 { break; }
            if !first_byte {
                ttfb_time = Instant::now().duration_since(t_start).as_secs_f64();
                first_byte = true;
            }
            buf.extend_from_slice(&tmp[..n]);
        }

        let t_end = Instant::now();
        t_ttfb = ttfb_time;
        t_total = t_end.duration_since(t_start).as_secs_f64();
        response_buf = buf;
    };

    let response_str = String::from_utf8_lossy(&response_buf);
    let first_line = response_str.lines().next().unwrap_or("");

    // Parse status code
    let sc: u16 = first_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Handle redirects
    if follow_redirects && (sc == 301 || sc == 302 || sc == 303 || sc == 307 || sc == 308) {
        let location = response_str
            .lines()
            .find(|l| l.to_lowercase().starts_with("location:"))
            .and_then(|l| l.splitn(2, ':').nth(1))
            .map(|s| s.trim().to_string());

        if let Some(loc) = location {
            let new_url = if loc.starts_with("http") {
                loc
            } else {
                format!("{}://{}{}", if is_https { "https" } else { "http" }, host, loc)
            };
            eprintln!("→ Redirect to {}", new_url);
            return measure_inner(&new_url, follow_redirects, depth + 1);
        }
    }

    Ok((
        Timings {
            dns_lookup: t_dns.duration_since(t_start).as_secs_f64(),
            connect: t_connect.duration_since(t_start).as_secs_f64(),
            ssl_handshake: t_ssl,
            ttfb: t_ttfb,
            total: t_total,
        },
        sc,
    ))
}

fn print_usage(prog: &str) {
    eprintln!("Usage: {} [OPTIONS] <URL>", prog);
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -L, --follow-redirects   Follow HTTP redirects");
    eprintln!("  -n, --count <N>          Run N times and show stats (default: 1)");
    eprintln!("  -h, --help               Show this help");
    eprintln!();
    eprintln!("Example:");
    eprintln!("  {} https://example.com", prog);
    eprintln!("  {} -L -n 3 https://example.com", prog);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = &args[0];

    if args.len() < 2 {
        print_usage(prog);
        std::process::exit(1);
    }

    let mut url = None;
    let mut follow_redirects = false;
    let mut count = 1usize;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-L" | "--follow-redirects" => follow_redirects = true,
            "-h" | "--help" => {
                print_usage(prog);
                return;
            }
            "-n" | "--count" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--count requires a value");
                    std::process::exit(1);
                }
                count = args[i].parse().unwrap_or_else(|_| {
                    eprintln!("Invalid count value");
                    std::process::exit(1);
                });
            }
            arg if !arg.starts_with('-') => {
                url = Some(arg.to_string());
            }
            arg => {
                eprintln!("Unknown option: {}", arg);
                print_usage(prog);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let url = url.unwrap_or_else(|| {
        eprintln!("No URL provided");
        print_usage(prog);
        std::process::exit(1);
    });

    if count == 1 {
        match measure(&url, follow_redirects) {
            Ok((t, status)) => {
                println!("URL:           {}", url);
                println!("Status:        {}", status);
                println!("DNS lookup:    {:.6}s", t.dns_lookup);
                println!("Connect:       {:.6}s", t.connect);
                println!("SSL handshake: {:.6}s", t.ssl_handshake);
                println!("TTFB:          {:.6}s", t.ttfb);
                println!("Total:         {:.6}s", t.total);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        let mut results = Vec::new();
        for run in 1..=count {
            print!("Run {}/{}... ", run, count);
            std::io::stdout().flush().ok();
            match measure(&url, follow_redirects) {
                Ok((t, status)) => {
                    println!("OK ({})", status);
                    results.push(t);
                }
                Err(e) => {
                    println!("FAILED: {}", e);
                }
            }
        }

        if results.is_empty() {
            eprintln!("All runs failed.");
            std::process::exit(1);
        }

        let n = results.len() as f64;
        macro_rules! stats {
            ($field:ident) => {{
                let vals: Vec<f64> = results.iter().map(|t| t.$field).collect();
                let mean = vals.iter().sum::<f64>() / n;
                let mut sorted = vals.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let min = sorted[0];
                let max = *sorted.last().unwrap();
                let median = sorted[sorted.len() / 2];
                (mean, min, max, median)
            }};
        }

        println!("\n{} runs — Summary:", results.len());
        println!("{:<16} {:>10} {:>10} {:>10} {:>10}", "Metric", "Mean", "Min", "Max", "Median");
        println!("{}", "-".repeat(58));

        let metrics = [
            ("DNS lookup", stats!(dns_lookup)),
            ("Connect", stats!(connect)),
            ("SSL handshake", stats!(ssl_handshake)),
            ("TTFB", stats!(ttfb)),
            ("Total", stats!(total)),
        ];

        for (name, (mean, min, max, median)) in &metrics {
            println!(
                "{:<16} {:>9.4}s {:>9.4}s {:>9.4}s {:>9.4}s",
                name, mean, min, max, median
            );
        }
    }
}