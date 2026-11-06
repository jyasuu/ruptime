use crate::i18n::{self, Lang, Msg};
use crate::monitoring::{CheckResult, TargetStatus};
use actix_web::{get, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use log::{error, info}; // Added log macros
use prometheus::process_collector::ProcessCollector;
use prometheus::{Encoder, Registry, TextEncoder};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Arc;
use tokio::sync::Mutex; // Added tokio::sync::Mutex
use urlencoding::decode;

const CONTENT_TYPE_PROMETHEUS: &str = "text/plain; version=0.0.4; charset=utf-8";
const CONTENT_TYPE_SVG: &str = "image/svg+xml; charset=utf-8";

// --- Prometheus Metric Definitions ---
const HELP_MONITOR_STATUS: &str =
    "# HELP monitor_status Current health status of the monitor (0=DOWN, 1=UP).";
const TYPE_MONITOR_STATUS: &str = "# TYPE monitor_status gauge";

const HELP_MONITOR_RESPONSE_TIME: &str =
    "# HELP monitor_response_time Last response time in milliseconds for HTTP/S checks.";
const TYPE_MONITOR_RESPONSE_TIME: &str = "# TYPE monitor_response_time gauge";

const HELP_MONITOR_CONSECUTIVE_FAILURES: &str =
    "# HELP monitor_consecutive_failures Total number of consecutive failures for the monitor.";
const TYPE_MONITOR_CONSECUTIVE_FAILURES: &str = "# TYPE monitor_consecutive_failures gauge"; // Changed to gauge as per common practice for this type of metric

const HELP_MONITOR_CERT_DAYS_REMAINING: &str =
    "# HELP monitor_cert_days_remaining The number of days remaining until the certificate expires";
const TYPE_MONITOR_CERT_DAYS_REMAINING: &str = "# TYPE monitor_cert_days_remaining gauge";

const HELP_MONITOR_CERT_IS_VALID: &str =
    "# HELP monitor_cert_is_valid Is the certificate still valid? (1 = Yes, 0 = No)";
const TYPE_MONITOR_CERT_IS_VALID: &str = "# TYPE monitor_cert_is_valid gauge";
// --- End Prometheus Metric Definitions ---

// Helper to escape label values for Prometheus
fn escape_label_value(value: &str) -> String {
    value
        .replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
}

// Helper to escape text for SVG
fn escape_svg_text(text: &str) -> String {
    text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&#39;")
}

/// Reads `?name=value` from a raw query string (percent-decoded).
fn query_param(req: &HttpRequest, name: &str) -> Option<String> {
    req.query_string().split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == name {
            Some(
                decode(&v.replace('+', " "))
                    .map(|d| d.to_string())
                    .unwrap_or_else(|_| v.to_string()),
            )
        } else {
            None
        }
    })
}

/// `?lang=` → `Accept-Language` → configured default → English.
fn request_lang(req: &HttpRequest) -> Lang {
    let accept = req
        .headers()
        .get("accept-language")
        .and_then(|h| h.to_str().ok());
    i18n::resolve(query_param(req, "lang").as_deref(), accept)
}

// Generate SVG badge for a target status
fn generate_svg_badge(
    label: &str,
    message: &str,
    color: &str,
    response_time: Option<u128>,
    uptime: Option<f32>,
    lang: Lang,
) -> String {
    let label_escaped = escape_svg_text(label);
    let message_escaped = escape_svg_text(message);

    // Calculate text widths (approximate)
    let label_width = i18n::text_px_width(label) + 10;
    let message_width = i18n::text_px_width(message) + 10;
    let total_width = label_width + message_width;

    let mut additional_info = String::new();
    let mut badge_height = 20;

    // Add response time and uptime if available
    if response_time.is_some() || uptime.is_some() {
        badge_height = 35;
        let mut info_parts = Vec::new();

        if let Some(rt) = response_time {
            info_parts.push(format!("{}ms", rt));
        }

        if let Some(up) = uptime {
            info_parts.push(i18n::fmt_uptime(lang, up));
        }

        let info_text = info_parts.join(" | ");
        additional_info = format!(
            "<text x=\"{}\" y=\"30\" text-anchor=\"middle\" font-family=\"Verdana,sans-serif\" font-size=\"9\" fill=\"#333\">{}</text>",
            total_width / 2,
            escape_svg_text(&info_text)
        );
    }

    format!(
        concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">",
            "<defs><linearGradient id=\"b\" x2=\"0\" y2=\"100%\">",
            "<stop offset=\"0\" stop-color=\"#bbb\" stop-opacity=\".1\"/>",
            "<stop offset=\"1\" stop-opacity=\".1\"/></linearGradient></defs>",
            "<clipPath id=\"a\"><rect width=\"{}\" height=\"{}\" rx=\"3\" fill=\"#fff\"/></clipPath>",
            "<g clip-path=\"url(#a)\">",
            "<path fill=\"#555\" d=\"M0 0h{}v{}H0z\"/>",
            "<path fill=\"{}\" d=\"M{} 0h{}v{}H{}z\"/>",
            "<path fill=\"url(#b)\" d=\"M0 0h{}v{}H0z\"/></g>",
            "<g fill=\"#fff\" text-anchor=\"middle\" font-family=\"Verdana,sans-serif\" font-size=\"11\">",
            "<text x=\"{}\" y=\"15\" fill=\"#010101\" fill-opacity=\".3\">{}</text>",
            "<text x=\"{}\" y=\"14\">{}</text>",
            "<text x=\"{}\" y=\"15\" fill=\"#010101\" fill-opacity=\".3\">{}</text>",
            "<text x=\"{}\" y=\"14\">{}</text></g>{}</svg>"
        ),
        total_width, badge_height,        // svg dimensions
        total_width, badge_height,        // rect dimensions
        label_width, badge_height,        // left background
        color, label_width, message_width, badge_height, label_width,  // right background
        total_width, badge_height,        // gradient overlay
        label_width / 2, label_escaped,   // label shadow
        label_width / 2, label_escaped,   // label text
        label_width + message_width / 2, message_escaped,  // message shadow
        label_width + message_width / 2, message_escaped,  // message text
        additional_info                   // additional info
    )
}

#[get("/badge/{target_alias}")]
async fn badge_handler(
    req: HttpRequest,
    path: web::Path<String>,
    data: web::Data<Arc<Mutex<Vec<TargetStatus>>>>,
) -> impl Responder {
    let target_alias = path.into_inner();
    let decoded_alias = match decode(&target_alias) {
        Ok(decoded) => decoded.to_string(),
        Err(_) => target_alias, // Use original if decoding fails
    };

    let statuses = data.lock().await;

    // Find the target status by alias
    let target_status = statuses
        .iter()
        .find(|status| status.target_alias == decoded_alias);

    match target_status {
        Some(status) => {
            let lang = request_lang(&req);
            let (message, color) = if status.is_healthy {
                (i18n::t(lang, Msg::Up), "#4c1")
            } else {
                (i18n::t(lang, Msg::Down), "#e05d44")
            };

            let response_time = match &status.last_result {
                Some(CheckResult::Http(http_details)) => Some(http_details.response_time_ms),
                Some(CheckResult::Tcp(tcp_details)) => Some(tcp_details.response_time_ms),
                Some(CheckResult::Protocol(_, r)) => Some(r.response_time_ms),
                _ => None,
            };

            let svg = generate_svg_badge(
                &status.target_alias,
                message,
                color,
                response_time,
                Some(status.uptime_percentage_24h as f32),
                lang,
            );

            HttpResponse::Ok().content_type(CONTENT_TYPE_SVG).body(svg)
        }
        None => {
            let svg = generate_svg_badge(
                &decoded_alias,
                i18n::t(request_lang(&req), Msg::NotFound),
                "#9f9f9f",
                None,
                None,
                request_lang(&req),
            );

            HttpResponse::NotFound()
                .content_type(CONTENT_TYPE_SVG)
                .body(svg)
        }
    }
}

#[get("/badge/{target_alias}/simple")]
async fn simple_badge_handler(
    req: HttpRequest,
    path: web::Path<String>,
    data: web::Data<Arc<Mutex<Vec<TargetStatus>>>>,
) -> impl Responder {
    let target_alias = path.into_inner();
    let decoded_alias = match decode(&target_alias) {
        Ok(decoded) => decoded.to_string(),
        Err(_) => target_alias,
    };

    let statuses = data.lock().await;
    let target_status = statuses
        .iter()
        .find(|status| status.target_alias == decoded_alias);

    match target_status {
        Some(status) => {
            let lang = request_lang(&req);
            let (message, color) = if status.is_healthy {
                (i18n::t(lang, Msg::Up), "#4c1")
            } else {
                (i18n::t(lang, Msg::Down), "#e05d44")
            };

            let svg = generate_svg_badge(
                &status.target_alias,
                message,
                color,
                None, // No additional info for simple badge
                None,
                lang,
            );

            HttpResponse::Ok().content_type(CONTENT_TYPE_SVG).body(svg)
        }
        None => {
            let svg = generate_svg_badge(
                &decoded_alias,
                i18n::t(request_lang(&req), Msg::NotFound),
                "#9f9f9f",
                None,
                None,
                request_lang(&req),
            );

            HttpResponse::NotFound()
                .content_type(CONTENT_TYPE_SVG)
                .body(svg)
        }
    }
}

#[get("/badges")]
async fn badges_list_handler(
    req: HttpRequest,
    data: web::Data<Arc<Mutex<Vec<TargetStatus>>>>,
) -> impl Responder {
    let lang = request_lang(&req);
    let statuses = data.lock().await;
    let title = i18n::t(lang, Msg::BadgesTitle);
    let lang_q = if query_param(&req, "lang").is_some() {
        format!("?lang={}", lang.code())
    } else {
        String::new()
    };

    let mut html = String::new();
    html.push_str(&format!(
        "<!DOCTYPE html><html lang=\"{}\"><head><meta charset=\"utf-8\"><title>{}</title>",
        lang.code(),
        escape_svg_text(title)
    ));
    html.push_str("<style>body { font-family: Arial, 'Noto Sans CJK TC', 'Noto Sans CJK SC', 'Noto Sans CJK JP', sans-serif; margin: 40px; }");
    html.push_str(".badge-item { margin: 15px 0; padding: 10px; border: 1px solid #ddd; border-radius: 5px; }");
    html.push_str(
        ".badge-url { font-family: monospace; background: #f5f5f5; padding: 5px; margin: 5px 0; }",
    );
    html.push_str("</style></head><body>");
    html.push_str(&format!("<h1>{}</h1>", escape_svg_text(title)));
    html.push_str(&format!(
        "<p>{}</p>",
        escape_svg_text(i18n::t(lang, Msg::BadgesIntro))
    ));

    for status in statuses.iter() {
        let encoded_alias = urlencoding::encode(&status.target_alias);
        let mut meta = String::new();
        if let Some(g) = &status.group {
            meta.push_str(&format!(
                " | <strong>{}</strong> {}",
                i18n::t(lang, Msg::Group),
                escape_svg_text(g)
            ));
        }
        if !status.tags.is_empty() {
            meta.push_str(&format!(
                " | <strong>{}</strong> {}",
                i18n::t(lang, Msg::Tags),
                escape_svg_text(&status.tags.join(", "))
            ));
        }
        if let Some(days) = status.domain_expiry_days {
            meta.push_str(&format!(
                " | {} {}d",
                i18n::t(lang, Msg::DomainExpires),
                days
            ));
        }
        html.push_str(&format!(
            r#"<div class="badge-item">
                <h3>{}</h3>
                <p><strong>{}</strong></p>
                <img src="/badge/{}{}" alt="Status badge for {}">
                <div class="badge-url">URL: /badge/{}</div>
                <p><strong>{}</strong></p>
                <img src="/badge/{}/simple{}" alt="Simple status badge for {}">
                <div class="badge-url">URL: /badge/{}/simple</div>
                <p><strong>{}</strong> {} | <strong>{}</strong> {}{}</p>
            </div>"#,
            escape_svg_text(&status.target_alias),
            i18n::t(lang, Msg::DetailedBadge),
            encoded_alias,
            lang_q,
            escape_svg_text(&status.target_alias),
            encoded_alias,
            i18n::t(lang, Msg::SimpleBadge),
            encoded_alias,
            lang_q,
            escape_svg_text(&status.target_alias),
            encoded_alias,
            i18n::t(lang, Msg::Status),
            if status.is_healthy {
                i18n::t(lang, Msg::Up)
            } else {
                i18n::t(lang, Msg::Down)
            },
            i18n::t(lang, Msg::MonitorUrl),
            escape_svg_text(&status.monitor_url),
            meta
        ));
    }

    html.push_str("</body></html>");

    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
}

// ─── JSON API: monitors, groups, tags ─────────────────────────────────────────

/// `GET /api/monitors?tag=prod&group=db&lang=ja`
#[get("/api/monitors")]
async fn monitors_api_handler(
    req: HttpRequest,
    data: web::Data<Arc<Mutex<Vec<TargetStatus>>>>,
) -> impl Responder {
    let lang = request_lang(&req);
    let tag_filter = query_param(&req, "tag");
    let group_filter = query_param(&req, "group");
    let statuses = data.lock().await;

    let selected: Vec<&TargetStatus> = statuses
        .iter()
        .filter(|s| {
            tag_filter
                .as_ref()
                .is_none_or(|t| s.tags.iter().any(|x| x == t))
        })
        .filter(|s| {
            group_filter
                .as_ref()
                .is_none_or(|g| s.group.as_deref() == Some(g.as_str()))
        })
        .collect();

    // Aggregate group status: a group is UP only when every member is UP.
    let mut groups: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for s in &selected {
        if let Some(g) = &s.group {
            let e = groups.entry(g.clone()).or_insert((0, 0));
            e.1 += 1;
            if s.is_healthy {
                e.0 += 1;
            }
        }
    }

    let monitors: Vec<serde_json::Value> = selected
        .iter()
        .map(|s| {
            let response_time = match &s.last_result {
                Some(CheckResult::Http(d)) => Some(d.response_time_ms),
                Some(CheckResult::Tcp(d)) => Some(d.response_time_ms),
                Some(CheckResult::Protocol(_, r)) => Some(r.response_time_ms),
                _ => None,
            };
            serde_json::json!({
                "name": s.target_alias,
                "type": s.last_result.as_ref().map(|r| r.type_label()).unwrap_or("unknown"),
                "group": s.group,
                "tags": s.tags,
                "status": if s.is_healthy { "up" } else { "down" },
                "status_text": if s.is_healthy { i18n::t(lang, Msg::Up) } else { i18n::t(lang, Msg::Down) },
                "url": s.monitor_url,
                "hostname": s.monitor_hostname,
                "port": s.monitor_port,
                "response_time_ms": response_time.map(|v| v as u64),
                "uptime_24h": s.uptime_percentage_24h,
                "consecutive_failures": s.consecutive_failures,
                "cert_days_remaining": s.cert_days_remaining,
                "domain_expiry_days": s.domain_expiry_days,
            })
        })
        .collect();

    let groups_json: Vec<serde_json::Value> = groups
        .into_iter()
        .map(|(name, (up, total))| {
            let healthy = up == total;
            serde_json::json!({
                "name": name,
                "status": if healthy { "up" } else { "down" },
                "status_text": if healthy { i18n::t(lang, Msg::Up) } else { i18n::t(lang, Msg::Down) },
                "up": up,
                "total": total,
            })
        })
        .collect();

    HttpResponse::Ok().json(serde_json::json!({
        "language": lang.code(),
        "groups": groups_json,
        "monitors": monitors,
    }))
}

/// `GET|POST /api/push/{token}?status=up|down&msg=OK&ping=12`
#[actix_web::route("/api/push/{token}", method = "GET", method = "POST")]
async fn push_handler(req: HttpRequest, path: web::Path<String>) -> impl Responder {
    let token = path.into_inner();
    if !crate::monitoring::push::is_registered(&token) {
        return HttpResponse::NotFound()
            .json(serde_json::json!({"ok": false, "msg": "Push monitor not found"}));
    }
    let up = !matches!(
        query_param(&req, "status").as_deref(),
        Some("down") | Some("DOWN")
    );
    let msg: String = query_param(&req, "msg")
        .unwrap_or_else(|| "OK".to_string())
        .chars()
        .take(200)
        .collect();
    let ping = query_param(&req, "ping")
        .and_then(|p| p.parse::<f64>().ok())
        .filter(|p| *p >= 0.0)
        .map(|p| p as u128);
    crate::monitoring::push::record_push(&token, up, msg, ping);
    HttpResponse::Ok().json(serde_json::json!({"ok": true}))
}

#[get("/metrics")]
async fn metrics_handler(data: web::Data<Arc<Mutex<Vec<TargetStatus>>>>) -> impl Responder {
    let statuses = data.lock().await; // Changed to .await for tokio::sync::Mutex

    let mut custom_metrics_output = String::new();

    // General Metrics (apply to all types)
    custom_metrics_output.push_str(HELP_MONITOR_STATUS);
    custom_metrics_output.push('\n');
    custom_metrics_output.push_str(TYPE_MONITOR_STATUS);
    custom_metrics_output.push('\n');

    let mut status_metrics_buffer = String::new();

    custom_metrics_output.push_str(HELP_MONITOR_CONSECUTIVE_FAILURES);
    custom_metrics_output.push('\n');
    custom_metrics_output.push_str(TYPE_MONITOR_CONSECUTIVE_FAILURES);
    custom_metrics_output.push('\n');
    let mut consecutive_failures_buffer = String::new();

    // HTTP Specific Metrics
    let mut http_metrics_buffer = String::new();
    let mut has_http_metrics = false; // To know if we need to print HTTP specific HELP/TYPE
    let mut info_buffer = String::new();
    let mut domain_buffer = String::new();
    let mut group_totals: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    // Iterate once and build up metric strings for custom metrics
    for status in statuses.iter() {
        let monitor_name = escape_label_value(&status.target_alias);
        let monitor_url = escape_label_value(&status.monitor_url);
        let monitor_hostname = escape_label_value(&status.monitor_hostname);
        let monitor_port = escape_label_value(&status.monitor_port.to_string());

        let check_type = status
            .last_result
            .as_ref()
            .map(|r| r.type_label())
            .unwrap_or("unknown");

        let labels = format!(
            "monitor_name=\"{}\",monitor_type=\"{}\",monitor_url=\"{}\",monitor_hostname=\"{}\",monitor_port=\"{}\"",
            monitor_name, check_type, monitor_url, monitor_hostname, monitor_port
        );

        // monitor_status
        let health_value = if status.is_healthy { 1 } else { 0 };
        let _ = writeln!(
            status_metrics_buffer,
            "monitor_status{{{}}} {}",
            labels, health_value
        );

        // monitor_consecutive_failures
        let _ = writeln!(
            consecutive_failures_buffer,
            "monitor_consecutive_failures{{{}}} {}",
            labels, status.consecutive_failures
        );

        // HTTP specific metrics
        if let Some(CheckResult::Http(http_details)) = &status.last_result {
            if !has_http_metrics {
                has_http_metrics = true; // Mark that we have HTTP metrics to print HELP/TYPE lines later
            }

            // monitor_response_time
            let _ = writeln!(
                http_metrics_buffer,
                "monitor_response_time{{{}}} {}",
                labels, http_details.response_time_ms
            );

            // monitor_cert_days_remaining
            if let Some(days_remaining) = status.cert_days_remaining {
                let _ = writeln!(
                    http_metrics_buffer,
                    "monitor_cert_days_remaining{{{}}} {}",
                    labels, days_remaining
                );
            }

            // monitor_cert_is_valid
            let cert_valid_value = if status.cert_is_valid.unwrap_or(false) {
                1
            } else {
                0
            };
            let _ = writeln!(
                http_metrics_buffer,
                "monitor_cert_is_valid{{{}}} {}",
                labels, cert_valid_value
            );
        }
        if let Some(CheckResult::Protocol(_, r)) = &status.last_result {
            has_http_metrics = true;
            let _ = writeln!(
                http_metrics_buffer,
                "monitor_response_time{{{}}} {}",
                labels, r.response_time_ms
            );
        }

        // Groups / tags (info-style series; labels of existing series are unchanged)
        if status.group.is_some() || !status.tags.is_empty() {
            let _ = writeln!(
                info_buffer,
                "monitor_info{{{},monitor_group=\"{}\",monitor_tags=\"{}\"}} 1",
                labels,
                escape_label_value(status.group.as_deref().unwrap_or("")),
                escape_label_value(&status.tags.join(","))
            );
        }
        if let Some(g) = &status.group {
            let e = group_totals.entry(g.clone()).or_insert((0, 0));
            e.1 += 1;
            if status.is_healthy {
                e.0 += 1;
            }
        }
        if let Some(days) = status.domain_expiry_days {
            let _ = writeln!(
                domain_buffer,
                "monitor_domain_days_remaining{{{}}} {}",
                labels, days
            );
        }

        if let Some(CheckResult::Tcp(tcp_details)) = &status.last_result {
            // monitor_response_time
            let _ = writeln!(
                http_metrics_buffer,
                "monitor_response_time{{{}}} {}",
                labels, tcp_details.response_time_ms
            );
        }
    }

    // Append buffered custom metrics to the main custom output
    custom_metrics_output.push_str(&status_metrics_buffer);
    custom_metrics_output.push_str(&consecutive_failures_buffer);

    if !info_buffer.is_empty() {
        custom_metrics_output
            .push_str("# HELP monitor_info Group and tags of the monitor (value is always 1).\n");
        custom_metrics_output.push_str("# TYPE monitor_info gauge\n");
        custom_metrics_output.push_str(&info_buffer);
    }
    if !group_totals.is_empty() {
        custom_metrics_output.push_str("# HELP monitor_group_status Aggregate group health (1 = every member UP, 0 = any member DOWN).\n");
        custom_metrics_output.push_str("# TYPE monitor_group_status gauge\n");
        for (g, (up, total)) in &group_totals {
            let _ = writeln!(
                custom_metrics_output,
                "monitor_group_status{{monitor_group=\"{}\"}} {}",
                escape_label_value(g),
                if up == total { 1 } else { 0 }
            );
        }
    }
    if !domain_buffer.is_empty() {
        custom_metrics_output.push_str("# HELP monitor_domain_days_remaining Days until the registered domain expires (RDAP).\n");
        custom_metrics_output.push_str("# TYPE monitor_domain_days_remaining gauge\n");
        custom_metrics_output.push_str(&domain_buffer);
    }

    if has_http_metrics {
        custom_metrics_output.push_str(HELP_MONITOR_RESPONSE_TIME);
        custom_metrics_output.push('\n');
        custom_metrics_output.push_str(TYPE_MONITOR_RESPONSE_TIME);
        custom_metrics_output.push('\n');

        custom_metrics_output.push_str(HELP_MONITOR_CERT_DAYS_REMAINING);
        custom_metrics_output.push('\n');
        custom_metrics_output.push_str(TYPE_MONITOR_CERT_DAYS_REMAINING);
        custom_metrics_output.push('\n');

        custom_metrics_output.push_str(HELP_MONITOR_CERT_IS_VALID);
        custom_metrics_output.push('\n');
        custom_metrics_output.push_str(TYPE_MONITOR_CERT_IS_VALID);
        custom_metrics_output.push('\n');

        custom_metrics_output.push_str(&http_metrics_buffer);
    }

    // Process Metrics
    let registry = Registry::new();
    let process_collector = ProcessCollector::for_self();
    registry
        .register(Box::new(process_collector))
        .expect("ProcessCollector registration failed");

    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    let process_metric_families = registry.gather();
    encoder
        .encode(&process_metric_families, &mut buffer)
        .expect("Failed to encode process metrics");
    let process_metrics_string = String::from_utf8(buffer).unwrap_or_default();

    // Combine custom metrics and process metrics
    // Adding a newline if custom_metrics_output is not empty and process_metrics_string is not empty
    let final_output = if !custom_metrics_output.is_empty() && !process_metrics_string.is_empty() {
        format!("{}\n{}", custom_metrics_output, process_metrics_string)
    } else {
        format!("{}{}", custom_metrics_output, process_metrics_string)
    };

    HttpResponse::Ok()
        .content_type(CONTENT_TYPE_PROMETHEUS)
        .body(final_output)
}

pub async fn start_web_server(
    address: String,
    shared_statuses: Arc<Mutex<Vec<TargetStatus>>>, // This type now correctly refers to tokio::sync::Mutex due to the import change
) -> std::io::Result<()> {
    info!("Starting HTTP server at http://{}", address);
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(shared_statuses.clone()))
            .service(metrics_handler)
            .service(badge_handler)
            .service(simple_badge_handler)
            .service(badges_list_handler)
            .service(monitors_api_handler)
            .service(push_handler)
    })
    .bind(&address)? // Borrow address
    .run()
    .await
    .map_err(|e| {
        // Log error if server fails to run
        error!("HTTP server run failed: {}", e);
        e
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitoring::{CheckStatus, HttpCheckResultDetails, TcpCheckResult};
    use actix_web::body::to_bytes;
    use actix_web::test as actix_test; // Renamed to avoid conflict

    // Helper to create a TargetStatus for testing
    fn create_test_target_status(
        alias: &str,
        is_healthy: bool,
        consecutive_failures: u32,
        monitor_url: &str,
        monitor_hostname: &str,
        monitor_port: u16,
        check_result: Option<CheckResult>,
        cert_days: Option<i64>,
        cert_valid: Option<bool>,
    ) -> TargetStatus {
        TargetStatus {
            target_alias: alias.to_string(),
            last_check_time: None, // Not directly relevant for metrics output structure
            last_result: check_result,
            consecutive_failures,
            is_healthy,
            uptime_percentage_24h: 0.0,        // Placeholder
            average_response_time_24h_ms: 0.0, // Placeholder
            monitor_url: monitor_url.to_string(),
            monitor_hostname: monitor_hostname.to_string(),
            monitor_port,
            cert_days_remaining: cert_days,
            cert_is_valid: cert_valid,
            check_history: Vec::new(),
            group: None,
            tags: Vec::new(),
            domain_expiry_days: None,
        }
    }

    #[actix_web::test]
    async fn test_metrics_handler_empty_status_list() {
        let statuses = Arc::new(Mutex::new(Vec::<TargetStatus>::new())); // Tokio Mutex needs to be used here
        let data = web::Data::new(statuses);

        let app =
            actix_test::init_service(App::new().app_data(data.clone()).service(metrics_handler))
                .await;
        let req = actix_test::TestRequest::get().uri("/metrics").to_request();
        let resp = actix_test::call_service(&app, req).await;

        let body_bytes = to_bytes(resp.into_body()).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        // Check for process metrics (any common one)
        assert!(body_str.contains("process_cpu_seconds_total"));

        // Check for custom metric HELP/TYPE lines
        assert!(body_str.contains(HELP_MONITOR_STATUS));
        assert!(body_str.contains(TYPE_MONITOR_STATUS));
        assert!(body_str.contains(HELP_MONITOR_CONSECUTIVE_FAILURES));
        assert!(body_str.contains(TYPE_MONITOR_CONSECUTIVE_FAILURES));

        // Check that HTTP specific HELP/TYPE lines are NOT present if no HTTP metrics
        assert!(!body_str.contains(HELP_MONITOR_RESPONSE_TIME));
        assert!(!body_str.contains(TYPE_MONITOR_RESPONSE_TIME));
        assert!(!body_str.contains(HELP_MONITOR_CERT_DAYS_REMAINING));
        assert!(!body_str.contains(TYPE_MONITOR_CERT_DAYS_REMAINING));
        assert!(!body_str.contains(HELP_MONITOR_CERT_IS_VALID));
        assert!(!body_str.contains(TYPE_MONITOR_CERT_IS_VALID));

        // Check that no actual custom metric data lines are present
        assert!(!body_str.contains("monitor_status{"));
        assert!(!body_str.contains("monitor_consecutive_failures{"));
    }

    #[actix_web::test]
    async fn test_metrics_handler_mixed_targets() {
        let statuses_vec = vec![
            create_test_target_status(
                "Healthy HTTP Cert OK",
                true,
                0,
                "https://healthy.example.com",
                "healthy.example.com",
                443,
                Some(CheckResult::Http(HttpCheckResultDetails {
                    status: CheckStatus::Healthy,
                    response_time_ms: 120,
                    cert_days_remaining: Some(30),
                    cert_is_valid: Some(true),
                })),
                Some(30),
                Some(true),
            ),
            create_test_target_status(
                "Unhealthy HTTP No Cert",
                false,
                3,
                "http://unhealthy.example.com",
                "unhealthy.example.com",
                80,
                Some(CheckResult::Http(HttpCheckResultDetails {
                    status: CheckStatus::Unhealthy("Timeout".to_string()),
                    response_time_ms: 5000,
                    cert_days_remaining: None,
                    cert_is_valid: None,
                })),
                None,
                None,
            ),
            create_test_target_status(
                "Healthy TCP",
                true,
                0,
                "tcp://healthy.tcp.example.com:1234",
                "healthy.tcp.example.com",
                1234,
                Some(CheckResult::Tcp(TcpCheckResult {
                    status: CheckStatus::Healthy,
                    response_time_ms: 0,
                })),
                None,
                None,
            ),
            create_test_target_status(
                "Unhealthy TCP",
                false,
                5,
                "tcp://unhealthy.tcp.example.com:5678",
                "unhealthy.tcp.example.com",
                5678,
                Some(CheckResult::Tcp(TcpCheckResult {
                    status: CheckStatus::Unhealthy("Connection refused".to_string()),
                    response_time_ms: 0,
                })),
                None,
                None,
            ),
        ];
        let statuses = Arc::new(Mutex::new(statuses_vec)); // Tokio Mutex needs to be used here
        let data = web::Data::new(statuses.clone()); // Clone Arc for app_data

        let app =
            actix_test::init_service(App::new().app_data(data.clone()).service(metrics_handler))
                .await;
        let req = actix_test::TestRequest::get().uri("/metrics").to_request();
        let resp = actix_test::call_service(&app, req).await;

        let body_bytes = to_bytes(resp.into_body()).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        // Check for process metrics
        assert!(body_str.contains("process_cpu_seconds_total"));

        // --- Check HELP/TYPE lines ---
        // General
        assert_eq!(body_str.matches(HELP_MONITOR_STATUS).count(), 1);
        assert_eq!(body_str.matches(TYPE_MONITOR_STATUS).count(), 1);
        assert_eq!(
            body_str.matches(HELP_MONITOR_CONSECUTIVE_FAILURES).count(),
            1
        );
        assert_eq!(
            body_str.matches(TYPE_MONITOR_CONSECUTIVE_FAILURES).count(),
            1
        );
        // HTTP Specific
        assert_eq!(body_str.matches(HELP_MONITOR_RESPONSE_TIME).count(), 1);
        assert_eq!(body_str.matches(TYPE_MONITOR_RESPONSE_TIME).count(), 1);
        assert_eq!(
            body_str.matches(HELP_MONITOR_CERT_DAYS_REMAINING).count(),
            1
        );
        assert_eq!(
            body_str.matches(TYPE_MONITOR_CERT_DAYS_REMAINING).count(),
            1
        );
        assert_eq!(body_str.matches(HELP_MONITOR_CERT_IS_VALID).count(), 1);
        assert_eq!(body_str.matches(TYPE_MONITOR_CERT_IS_VALID).count(), 1);

        // --- Check Metric Data Lines ---
        // Healthy HTTP Cert OK
        let healthy_http_labels = "monitor_name=\"Healthy HTTP Cert OK\",monitor_type=\"http\",monitor_url=\"https://healthy.example.com\",monitor_hostname=\"healthy.example.com\",monitor_port=\"443\"";
        assert!(body_str.contains(&format!("monitor_status{{{}}} 1", healthy_http_labels)));
        assert!(body_str.contains(&format!(
            "monitor_consecutive_failures{{{}}} 0",
            healthy_http_labels
        )));
        assert!(body_str.contains(&format!(
            "monitor_response_time{{{}}} 120",
            healthy_http_labels
        )));
        assert!(body_str.contains(&format!(
            "monitor_cert_days_remaining{{{}}} 30",
            healthy_http_labels
        )));
        assert!(body_str.contains(&format!(
            "monitor_cert_is_valid{{{}}} 1",
            healthy_http_labels
        )));

        // Unhealthy HTTP No Cert
        let unhealthy_http_labels = "monitor_name=\"Unhealthy HTTP No Cert\",monitor_type=\"http\",monitor_url=\"http://unhealthy.example.com\",monitor_hostname=\"unhealthy.example.com\",monitor_port=\"80\"";
        assert!(body_str.contains(&format!("monitor_status{{{}}} 0", unhealthy_http_labels)));
        assert!(body_str.contains(&format!(
            "monitor_consecutive_failures{{{}}} 3",
            unhealthy_http_labels
        )));
        assert!(body_str.contains(&format!(
            "monitor_response_time{{{}}} 5000",
            unhealthy_http_labels
        )));
        // Cert metrics should NOT be present for this target
        assert!(!body_str.contains(&format!(
            "monitor_cert_days_remaining{{{}}}",
            unhealthy_http_labels
        )));
        assert!(body_str.contains(&format!(
            "monitor_cert_is_valid{{{}}} 0",
            unhealthy_http_labels
        ))); // Should be 0 as None is treated as false

        // Healthy TCP
        let healthy_tcp_labels = "monitor_name=\"Healthy TCP\",monitor_type=\"tcp\",monitor_url=\"tcp://healthy.tcp.example.com:1234\",monitor_hostname=\"healthy.tcp.example.com\",monitor_port=\"1234\"";
        assert!(body_str.contains(&format!("monitor_status{{{}}} 1", healthy_tcp_labels)));
        assert!(body_str.contains(&format!(
            "monitor_consecutive_failures{{{}}} 0",
            healthy_tcp_labels
        )));
        // TCP checks report their connect time; HTTP-only cert metrics must be absent
        assert!(body_str.contains(&format!(
            "monitor_response_time{{{}}} 0",
            healthy_tcp_labels
        )));
        assert!(!body_str.contains(&format!(
            "monitor_cert_days_remaining{{{}}}",
            healthy_tcp_labels
        )));
        assert!(!body_str.contains(&format!("monitor_cert_is_valid{{{}}}", healthy_tcp_labels)));

        // Unhealthy TCP
        let unhealthy_tcp_labels = "monitor_name=\"Unhealthy TCP\",monitor_type=\"tcp\",monitor_url=\"tcp://unhealthy.tcp.example.com:5678\",monitor_hostname=\"unhealthy.tcp.example.com\",monitor_port=\"5678\"";
        assert!(body_str.contains(&format!("monitor_status{{{}}} 0", unhealthy_tcp_labels)));
        assert!(body_str.contains(&format!(
            "monitor_consecutive_failures{{{}}} 5",
            unhealthy_tcp_labels
        )));
    }

    #[actix_web::test]
    async fn test_metrics_handler_http_no_cert_info() {
        let statuses_vec = vec![create_test_target_status(
            "HTTP No Cert Info",
            true,
            0,
            "http://no.cert.info",
            "no.cert.info",
            80,
            Some(CheckResult::Http(HttpCheckResultDetails {
                status: CheckStatus::Healthy,
                response_time_ms: 75,
                cert_days_remaining: None, // Explicitly None
                cert_is_valid: None,       // Explicitly None
            })),
            None,
            None, // TargetStatus also has None for cert fields
        )];
        let statuses = Arc::new(Mutex::new(statuses_vec)); // Tokio Mutex needs to be used here
        let data = web::Data::new(statuses.clone()); // Clone Arc for app_data

        let app =
            actix_test::init_service(App::new().app_data(data.clone()).service(metrics_handler))
                .await;
        let req = actix_test::TestRequest::get().uri("/metrics").to_request();
        let resp = actix_test::call_service(&app, req).await;

        let body_bytes = to_bytes(resp.into_body()).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        let labels = "monitor_name=\"HTTP No Cert Info\",monitor_type=\"http\",monitor_url=\"http://no.cert.info\",monitor_hostname=\"no.cert.info\",monitor_port=\"80\"";
        assert!(body_str.contains(&format!("monitor_status{{{}}} 1", labels)));
        assert!(body_str.contains(&format!("monitor_response_time{{{}}} 75", labels)));

        // Cert metrics should NOT be present if days_remaining is None
        assert!(!body_str.contains(&format!("monitor_cert_days_remaining{{{}}}", labels)));
        // cert_is_valid should be 0 if None
        assert!(body_str.contains(&format!("monitor_cert_is_valid{{{}}} 0", labels)));
    }
    #[actix_web::test]
    async fn test_escape_label_value_in_metrics() {
        let statuses_vec = vec![create_test_target_status(
            "name with \"quotes\" and \\backslash and \nnewline",
            true,
            0,
            "http://label.test/path",
            "label.test",
            80,
            Some(CheckResult::Http(HttpCheckResultDetails {
                status: CheckStatus::Healthy,
                response_time_ms: 50,
                cert_days_remaining: None,
                cert_is_valid: None,
            })),
            None,
            None,
        )];
        let statuses = Arc::new(Mutex::new(statuses_vec)); // Tokio Mutex needs to be used here
        let data = web::Data::new(statuses.clone()); // Clone Arc for app_data

        let app =
            actix_test::init_service(App::new().app_data(data.clone()).service(metrics_handler))
                .await;
        let req = actix_test::TestRequest::get().uri("/metrics").to_request();
        let resp = actix_test::call_service(&app, req).await;

        let body_bytes = to_bytes(resp.into_body()).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        // Verify that the special characters in the alias are escaped in the output
        let expected_escaped_name =
            "monitor_name=\"name with \\\"quotes\\\" and \\\\backslash and \\nnewline\"";
        assert!(body_str.contains(expected_escaped_name));
        assert!(body_str.contains(&format!("monitor_status{{{},monitor_type=\"http\",monitor_url=\"http://label.test/path\",monitor_hostname=\"label.test\",monitor_port=\"80\"}} 1", expected_escaped_name)));
    }

    // Badge tests
    fn create_test_target_with_alias(alias: &str, is_healthy: bool) -> TargetStatus {
        let mut status = TargetStatus::new(
            alias.to_string(),
            "http://example.com".to_string(),
            "example.com".to_string(),
            80,
        );
        status.is_healthy = is_healthy;
        status.uptime_percentage_24h = if is_healthy { 99.5 } else { 45.2 };

        if is_healthy {
            status.last_result = Some(CheckResult::Http(HttpCheckResultDetails {
                status: CheckStatus::Healthy,
                response_time_ms: 150,
                cert_days_remaining: Some(30),
                cert_is_valid: Some(true),
            }));
        } else {
            status.last_result = Some(CheckResult::Http(HttpCheckResultDetails {
                status: CheckStatus::Unhealthy("Connection failed".to_string()),
                response_time_ms: 5000,
                cert_days_remaining: None,
                cert_is_valid: Some(false),
            }));
        }

        status
    }

    #[actix_web::test]
    async fn test_badge_handler_healthy_target() {
        let statuses = vec![create_test_target_with_alias("Test Website", true)];
        let shared_statuses = Arc::new(Mutex::new(statuses));
        let data = web::Data::new(shared_statuses);

        let app =
            actix_test::init_service(App::new().app_data(data.clone()).service(badge_handler))
                .await;

        let req = actix_test::TestRequest::get()
            .uri("/badge/Test%20Website")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;

        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "image/svg+xml; charset=utf-8"
        );

        let body_bytes = to_bytes(resp.into_body()).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        // Check SVG content
        assert!(body_str.contains("<svg"));
        assert!(body_str.contains("Test Website"));
        assert!(body_str.contains("UP"));
        assert!(body_str.contains("#4c1")); // Green color for healthy
        assert!(body_str.contains("150ms")); // Response time
        assert!(body_str.contains("99.5% uptime")); // Uptime percentage
    }

    #[actix_web::test]
    async fn test_simple_badge_handler() {
        let statuses = vec![create_test_target_with_alias("Simple Test", true)];
        let shared_statuses = Arc::new(Mutex::new(statuses));
        let data = web::Data::new(shared_statuses);

        let app = actix_test::init_service(
            App::new()
                .app_data(data.clone())
                .service(simple_badge_handler),
        )
        .await;

        let req = actix_test::TestRequest::get()
            .uri("/badge/Simple%20Test/simple")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;

        let body_bytes = to_bytes(resp.into_body()).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        assert!(body_str.contains("Simple Test"));
        assert!(body_str.contains("UP"));
        // Should NOT contain additional info in simple badge
        assert!(!body_str.contains("ms"));
        assert!(!body_str.contains("uptime"));
    }

    #[actix_web::test]
    async fn test_badge_handler_not_found() {
        let statuses: Vec<TargetStatus> = vec![];
        let shared_statuses = Arc::new(Mutex::new(statuses));
        let data = web::Data::new(shared_statuses);

        let app =
            actix_test::init_service(App::new().app_data(data.clone()).service(badge_handler))
                .await;

        let req = actix_test::TestRequest::get()
            .uri("/badge/NonExistent")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;

        assert_eq!(resp.status(), 404);

        let body_bytes = to_bytes(resp.into_body()).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        assert!(body_str.contains("NonExistent"));
        assert!(body_str.contains("NOT FOUND"));
        assert!(body_str.contains("#9f9f9f")); // Gray color for not found
    }

    // ─── Groups, tags, i18n, push ────────────────────────────────────────────

    fn grouped(alias: &str, healthy: bool, group: &str, tags: &[&str]) -> TargetStatus {
        let mut t = create_test_target_with_alias(alias, healthy);
        t.group = Some(group.to_string());
        t.tags = tags.iter().map(|s| s.to_string()).collect();
        t
    }

    #[actix_web::test]
    async fn test_badge_is_localized() {
        let data = web::Data::new(Arc::new(Mutex::new(vec![create_test_target_with_alias(
            "Test Website",
            true,
        )])));
        let app =
            actix_test::init_service(App::new().app_data(data.clone()).service(badge_handler))
                .await;

        // ?lang= wins
        let req = actix_test::TestRequest::get()
            .uri("/badge/Test%20Website?lang=zh-TW")
            .to_request();
        let body = String::from_utf8(
            to_bytes(actix_test::call_service(&app, req).await.into_body())
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(body.contains("正常"));
        assert!(body.contains("可用率"));
        assert!(!body.contains(">UP<"));

        // Accept-Language is honoured when no ?lang=
        let req = actix_test::TestRequest::get()
            .uri("/badge/Test%20Website")
            .insert_header(("Accept-Language", "ja,en;q=0.5"))
            .to_request();
        let body = String::from_utf8(
            to_bytes(actix_test::call_service(&app, req).await.into_body())
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(body.contains("稼働中"));

        // Unknown target → localized NOT FOUND, 404
        let req = actix_test::TestRequest::get()
            .uri("/badge/nope?lang=de")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), 404);
        let body = String::from_utf8(to_bytes(resp.into_body()).await.unwrap().to_vec()).unwrap();
        assert!(body.contains("NICHT GEFUNDEN"));
    }

    #[actix_web::test]
    async fn test_monitors_api_filters_and_group_status() {
        let statuses = vec![
            grouped("db-primary", true, "database", &["prod", "critical"]),
            grouped("db-replica", false, "database", &["prod"]),
            grouped("web", true, "frontend", &["staging"]),
        ];
        let data = web::Data::new(Arc::new(Mutex::new(statuses)));
        let app = actix_test::init_service(
            App::new()
                .app_data(data.clone())
                .service(monitors_api_handler),
        )
        .await;

        let get = |uri: &'static str| {
            let app = &app;
            async move {
                let req = actix_test::TestRequest::get().uri(uri).to_request();
                let resp = actix_test::call_service(app, req).await;
                assert_eq!(resp.status(), 200);
                serde_json::from_slice::<serde_json::Value>(
                    &to_bytes(resp.into_body()).await.unwrap(),
                )
                .unwrap()
            }
        };

        let all = get("/api/monitors").await;
        assert_eq!(all["monitors"].as_array().unwrap().len(), 3);

        let prod = get("/api/monitors?tag=prod").await;
        assert_eq!(prod["monitors"].as_array().unwrap().len(), 2);
        // one member DOWN → whole group DOWN
        let g = &prod["groups"][0];
        assert_eq!(g["name"], "database");
        assert_eq!(g["status"], "down");
        assert_eq!(g["up"], 1);
        assert_eq!(g["total"], 2);

        let fe = get("/api/monitors?group=frontend&lang=zh-TW").await;
        assert_eq!(fe["monitors"].as_array().unwrap().len(), 1);
        assert_eq!(fe["monitors"][0]["status_text"], "正常");
        assert_eq!(fe["language"], "zh-TW");
        assert_eq!(fe["groups"][0]["status"], "up");
    }

    #[actix_web::test]
    async fn test_push_endpoint() {
        crate::monitoring::push::register_token("tok-api-test");
        let app = actix_test::init_service(App::new().service(push_handler)).await;

        let req = actix_test::TestRequest::get()
            .uri("/api/push/tok-api-test?status=down&msg=boom&ping=42")
            .to_request();
        assert_eq!(actix_test::call_service(&app, req).await.status(), 200);
        let rec = crate::monitoring::push::last_push("tok-api-test").unwrap();
        assert!(!rec.up);
        assert_eq!(rec.message, "boom");
        assert_eq!(rec.ping_ms, Some(42));

        let req = actix_test::TestRequest::post()
            .uri("/api/push/tok-api-test")
            .to_request();
        assert_eq!(actix_test::call_service(&app, req).await.status(), 200);
        assert!(
            crate::monitoring::push::last_push("tok-api-test")
                .unwrap()
                .up
        );

        // Unregistered tokens are rejected and never stored
        let req = actix_test::TestRequest::get()
            .uri("/api/push/not-configured")
            .to_request();
        assert_eq!(actix_test::call_service(&app, req).await.status(), 404);
        assert!(crate::monitoring::push::last_push("not-configured").is_none());
    }

    #[actix_web::test]
    async fn test_metrics_group_info_and_domain() {
        let mut a = grouped("db-primary", true, "database", &["prod", "critical"]);
        a.domain_expiry_days = Some(123);
        let b = grouped("db-replica", false, "database", &["prod"]);
        let data = web::Data::new(Arc::new(Mutex::new(vec![a, b])));
        let app =
            actix_test::init_service(App::new().app_data(data.clone()).service(metrics_handler))
                .await;
        let req = actix_test::TestRequest::get().uri("/metrics").to_request();
        let body = String::from_utf8(
            to_bytes(actix_test::call_service(&app, req).await.into_body())
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();

        assert!(body.contains("monitor_group_status{monitor_group=\"database\"} 0"));
        assert!(body.contains("monitor_group=\"database\",monitor_tags=\"prod,critical\"} 1"));
        assert!(body.contains("monitor_domain_days_remaining{"));
        assert!(body.contains("} 123"));
    }

    #[actix_web::test]
    async fn test_metrics_protocol_type_label() {
        use crate::monitoring::{ProtocolKind, ServiceCheckResult};
        let t = create_test_target_status(
            "mq",
            true,
            0,
            "mqtt://broker:1883",
            "broker",
            1883,
            Some(CheckResult::Protocol(
                ProtocolKind::Mqtt,
                ServiceCheckResult {
                    status: CheckStatus::Healthy,
                    response_time_ms: 9,
                    service_info: None,
                },
            )),
            None,
            None,
        );
        let data = web::Data::new(Arc::new(Mutex::new(vec![t])));
        let app =
            actix_test::init_service(App::new().app_data(data.clone()).service(metrics_handler))
                .await;
        let req = actix_test::TestRequest::get().uri("/metrics").to_request();
        let body = String::from_utf8(
            to_bytes(actix_test::call_service(&app, req).await.into_body())
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(body.contains("monitor_type=\"mqtt\""));
        assert!(body.contains("monitor_response_time{monitor_name=\"mq\""));
    }
}
