use crate::monitoring::types::{CheckStatus, TcpCheckResult};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub async fn check_tcp_port(address: &str, port: u16, request_timeout: Duration) -> TcpCheckResult {
    let target = format!("{}:{}", address, port);
    let start_time = Instant::now();

    let connect_result = timeout(request_timeout, TcpStream::connect(&target)).await;
    let response_time_ms = start_time.elapsed().as_millis();

    let status = match connect_result {
        Ok(Ok(_)) => CheckStatus::Healthy,
        Ok(Err(e)) => CheckStatus::Unhealthy(format!("Connection to {} failed: {}", target, e)),
        Err(_) => CheckStatus::Unhealthy(format!(
            "Connection to {} timed out after {} seconds",
            target,
            request_timeout.as_secs()
        )),
    };

    TcpCheckResult {
        status,
        response_time_ms,
    }
}
