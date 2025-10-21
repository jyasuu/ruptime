use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

// --- TCP Check implementation ---
pub async fn check_tcp_port(address: &str, port: u16, request_timeout: Duration) -> Result<(), String> {
    let target = format!("{}:{}", address, port);

    match timeout(request_timeout, TcpStream::connect(&target)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(format!("Connection to {} failed: {}", target, e)),
        Err(_) => Err(format!(
            "Connection to {} timed out after {} seconds",
            target,
            request_timeout.as_secs()
        )),
    }
}