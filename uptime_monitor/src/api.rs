use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use std::sync::{Arc, Mutex};
use crate::monitoring::{TargetStatus, CheckResult};
use std::fmt::Write;
use log::{info, error}; // Added log macros

const CONTENT_TYPE_PROMETHEUS: &str = "text/plain; version=0.0.4; charset=utf-8";

// --- Prometheus Metric Definitions ---
const HELP_STATUS_HEALTH: &str = "# HELP uptime_status_health Current health status of the target (1=healthy, 0=unhealthy).";
const TYPE_STATUS_HEALTH: &str = "# TYPE uptime_status_health gauge";

const HELP_RESPONSE_TIME: &str = "# HELP uptime_response_time_seconds Last response time in seconds for HTTP/S checks.";
const TYPE_RESPONSE_TIME: &str = "# TYPE uptime_response_time_seconds gauge";

const HELP_CONSECUTIVE_FAILURES: &str = "# HELP uptime_consecutive_failures_total Total number of consecutive failures for the target.";
const TYPE_CONSECUTIVE_FAILURES: &str = "# TYPE uptime_consecutive_failures_total counter";
// --- End Prometheus Metric Definitions ---


// Helper to escape label values for Prometheus
fn escape_label_value(value: &str) -> String {
    value.replace("\\", "\\\\").replace("\"", "\\\"").replace("\n", "\\n")
}


#[get("/metrics")]
async fn metrics_handler(data: web::Data<Arc<Mutex<Vec<TargetStatus>>>>) -> impl Responder {
    let statuses = data.lock().unwrap(); // .unwrap() is okay for this example, but handle errors in prod

    let mut output = String::new();

    // Add HELP and TYPE lines only once
    output.push_str(HELP_STATUS_HEALTH);
    output.push_str("\n");
    output.push_str(TYPE_STATUS_HEALTH);
    output.push_str("\n");

    // Buffer for response time metrics, as they might not exist for all targets
    let mut response_time_metrics = String::new();
    let mut has_http_metrics = false;


    for status in statuses.iter() {
        let escaped_alias = escape_label_value(&status.target_alias);
        let check_type = match &status.last_result {
            Some(CheckResult::Tcp(_)) => "tcp",
            Some(CheckResult::Http(_)) => "http",
            None => "unknown", // Or skip if no result yet
        };

        // uptime_status_health
        let health_value = if status.is_healthy { 1 } else { 0 };
        let _ = writeln!(output, "uptime_status_health{{target_alias=\"{}\", check_type=\"{}\"}} {}",
                         escaped_alias, check_type, health_value);

        // uptime_response_time_seconds (only for HTTP)
        if let Some(CheckResult::Http(http_details)) = &status.last_result {
            if !has_http_metrics { // Add HELP/TYPE only if we have at least one HTTP metric
                response_time_metrics.push_str(HELP_RESPONSE_TIME);
                response_time_metrics.push_str("\n");
                response_time_metrics.push_str(TYPE_RESPONSE_TIME);
                response_time_metrics.push_str("\n");
                has_http_metrics = true;
            }
            let response_time_seconds = http_details.response_time_ms as f64 / 1000.0;
            let _ = writeln!(response_time_metrics, "uptime_response_time_seconds{{target_alias=\"{}\", check_type=\"{}\"}} {:.3}",
                             escaped_alias, check_type, response_time_seconds);
        }
    }

    // Append response time metrics if any
    if has_http_metrics {
        output.push_str(&response_time_metrics);
    }

    // Add HELP and TYPE for consecutive failures
    output.push_str(HELP_CONSECUTIVE_FAILURES);
    output.push_str("\n");
    output.push_str(TYPE_CONSECUTIVE_FAILURES);
    output.push_str("\n");

    for status in statuses.iter() {
        let escaped_alias = escape_label_value(&status.target_alias);
        let check_type = match &status.last_result {
            Some(CheckResult::Tcp(_)) => "tcp",
            Some(CheckResult::Http(_)) => "http",
            None => "unknown",
        };
        // uptime_consecutive_failures_total
        let _ = writeln!(output, "uptime_consecutive_failures_total{{target_alias=\"{}\", check_type=\"{}\"}} {}",
                         escaped_alias, check_type, status.consecutive_failures);
    }


    HttpResponse::Ok()
        .content_type(CONTENT_TYPE_PROMETHEUS)
        .body(output)
}

pub async fn start_web_server(
    address: String,
    shared_statuses: Arc<Mutex<Vec<TargetStatus>>>,
) -> std::io::Result<()> {
    info!("Starting HTTP server at http://{}", address);
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(shared_statuses.clone()))
            .service(metrics_handler)
    })
    .bind(&address)? // Borrow address
    .run()
    .await
    .map_err(|e| { // Log error if server fails to run
        error!("HTTP server run failed: {}", e);
        e
    })
}
