# Uptime Monitor

A comprehensive Rust-based uptime monitoring tool that checks the availability of HTTP, TCP, and database/service infrastructure and exposes metrics in Prometheus format. Built as an alternative to Uptime Kuma with advanced assertion testing capabilities and extensive service health checking.

## 🚀 Features

### Core Monitoring
- **HTTP Monitoring** - GET, POST, PUT, DELETE, HEAD, OPTIONS support
- **TCP Monitoring** - Port connectivity testing
- **Service Health Checks** - Database and infrastructure service monitoring
- **SSL/TLS Certificate Validation** - Certificate expiry and validity checks
- **Prometheus Metrics** - Industry-standard metrics format
- **SVG Status Badges** - Embeddable status badges for dashboards and documentation
- **Configurable Intervals** - Flexible monitoring frequency

### Service Health Monitoring
- **PostgreSQL** - Database connectivity, version detection, SSL support
- **Redis** - Connection testing, authentication, database selection
- **MySQL** - Database health checks with SSL and authentication
- **MongoDB** - Connection validation and cluster status
- **RabbitMQ** - AMQP connectivity and queue operations
- **Kafka** - Broker connectivity and metadata retrieval
- **Elasticsearch** - Cluster health monitoring via HTTP API

### Advanced HTTP Testing
- **Multiple Authentication Methods**:
  - Basic Authentication (username/password)
  - Bearer Token authentication  
  - OAuth2 support (client credentials flow)
- **Custom Headers** - Add any HTTP headers to requests
- **Request Body Support** - POST/PUT with custom payloads
- **Timeout Configuration** - Per-check timeout settings
- **Redirect Handling** - Follow or ignore redirects

### Powerful Assertion Engine
- **JSON Path Assertions** - Validate JSON response structure and values
- **Header Assertions** - Check response headers and values
- **Status Code Validation** - Expected vs actual status codes
- **Body Content Assertions** - Regex pattern matching
- **Response Time Validation** - Performance thresholds
- **Data Type Validation** - UUID, IP address, date format validation
- **Certificate Field Assertions** - SSL certificate property validation

### Assertion Predicates
- **Equality**: `Equals`, `NotEquals`
- **Comparison**: `GreaterThan`, `LessThan`, `GreaterThanOrEqual`, `LessThanOrEqual`
- **String Operations**: `StartsWith`, `EndsWith`, `Contains`, `NotContains`
- **Pattern Matching**: `Matches` (regex), `NotMatches`
- **Existence**: `Exists`, `NotExists`
- **Type Validation**: `IsBoolean`, `IsNumber`, `IsString`, `IsCollection`, `IsEmpty`
- **Format Validation**: `IsIsoDate`, `IsIpv4`, `IsIpv6`, `IsUuid`

## 📊 Metrics

The `/metrics` endpoint provides Prometheus-compatible metrics:

- `monitor_status` - Service health (1=UP, 0=DOWN)
- `monitor_response_time` - Last response time in **milliseconds**
- `monitor_consecutive_failures` - Consecutive failed checks (resets to 0 on recovery)
- `monitor_cert_days_remaining` - Days until the TLS certificate expires (HTTPS only)
- `monitor_cert_is_valid` - Certificate validity (1=valid, 0=invalid; HTTPS only)
- `monitor_domain_days_remaining` - Days until domain registration expires (needs `check_domain_expiry = true`)
- `monitor_group_status` - Group health (1 only if every member is UP)
- `monitor_info` - Group and tags of each monitor (value always 1)

Per-monitor metrics carry the labels `monitor_name`, `monitor_type`, `monitor_url`, `monitor_hostname` and `monitor_port`.

A ready-made Grafana dashboard is provisioned from `grafana/provisioning/dashboards/ruptime-overview.json`
(availability, latency, status history, TLS/domain expiry, groups).

## 🐳 Full stack with Docker Compose

Runs ruptime, Prometheus, Alertmanager and Grafana (with the provisioned **Ruptime / Uptime Overview** dashboard):

```bash
cp .env.example .env        # then set GRAFANA_ADMIN_PASSWORD (required; no default is shipped)
docker compose up --build
```

Grafana is at http://localhost:3000. The admin password is only read when Grafana first creates its
database, so to change it later use the Grafana UI (or delete `grafana/data` to start over).

## 🔒 Securing `/metrics` and `/api/monitors`

Set `RUPTIME_API_TOKEN` to require `Authorization: Bearer <token>` on `/metrics` and `/api/monitors`:

```bash
RUPTIME_API_TOKEN=$(openssl rand -hex 32) ./uptime_monitor
curl -H "Authorization: Bearer $RUPTIME_API_TOKEN" http://localhost:8080/metrics
```

- Unset or empty: endpoints stay open, as before (a warning is logged at startup).
- Badges and the push endpoint are **not** covered (push monitors have their own tokens).
- Plain HTTP sends the token in clear text; put it behind TLS if it crosses an untrusted network.

Prometheus scrape config:

```yaml
  - job_name: 'uptime'
    authorization:
      type: Bearer
      credentials_file: /etc/prometheus/ruptime_token   # file containing only the token
    static_configs:
      - targets: ['rust-app:8080']
```

## 🏷️ SVG Status Badges

The uptime monitor provides SVG badges that can be embedded in websites, documentation, or dashboards to display real-time status information.

### Badge Endpoints

- **`/badges`** - HTML page listing all available badges with previews
- **`/badge/{target_alias}`** - Detailed badge with response time and uptime percentage
- **`/badge/{target_alias}/simple`** - Simple badge showing only UP/DOWN status

### Badge Features

- **Real-time Status** - Shows current UP/DOWN status with appropriate colors
- **Response Time** - Displays latest response time for HTTP checks (detailed badge only)
- **Uptime Percentage** - Shows 24-hour uptime percentage (detailed badge only)
- **URL Encoding** - Target aliases with spaces or special characters are automatically handled
- **Colors**: 
  - 🟢 Green (`#4c1`) for healthy targets
  - 🔴 Red (`#e05d44`) for unhealthy targets  
  - ⚪ Gray (`#9f9f9f`) for unknown/not found targets

### Usage Examples

#### Markdown
```markdown
![Uptime Status](http://your-monitor:8080/badge/My%20Website)
![Simple Status](http://your-monitor:8080/badge/My%20Website/simple)
```

#### HTML
```html
<img src="http://your-monitor:8080/badge/My%20Website" alt="Website Status">
<img src="http://your-monitor:8080/badge/API%20Server/simple" alt="API Status">
```

#### RestructuredText
```rst
.. image:: http://your-monitor:8080/badge/Database%20Server
   :alt: Database Status
```

## 🛠️ Quick Start

### 1. Installation
```bash
git clone <repository>
cd uptime-monitor
cargo build --release
```

### 2. Configuration
```bash
cp config-example.toml config.toml
# Edit config.toml with your monitoring targets
```

### 3. Run
```bash
cargo run
# Or run the release binary
./target/release/uptime_monitor
```

### 4. Access Metrics & Badges
```bash
# Prometheus metrics
curl http://localhost:8080/metrics

# View all available badges
curl http://localhost:8080/badges

# Get SVG badge for a specific target
curl http://localhost:8080/badge/Example%20Website

# Get simple SVG badge (no additional info)
curl http://localhost:8080/badge/Example%20Website/simple
```

## ⚙️ Configuration

### Basic Configuration
```toml
# Monitoring interval in seconds
monitoring_interval_seconds = 60

# Memory cleanup interval in minutes  
memory_cleanup_interval_minutes = 60

# Keep history for hours
keep_history_hours = 24

[[hosts]]
address = "example.com"
alias = "Example Website"

  [[hosts.checks]]
  type = "Http"
  name = "Example HTTPS Check"  # Optional: Custom name for this specific check
  port = 443
  path = "/"
  protocol = "Https"
  method = "Get"
  timeout_seconds = 10
  expected_status_code = 200
  check_ssl_certificate = true
```

### Check Naming
Each check can have an optional `name` field that will be used as the target alias in metrics and badges:

- **With `name`**: The check will appear in metrics/badges with the specified name
- **Without `name`**: The check will appear as "HostAlias (CheckType:Port)" (e.g., "Web Server (HTTP:80)")

This is especially useful when monitoring multiple services on the same host:

```toml
[[hosts]]
address = "web.example.com"
alias = "Web Server"

  [[hosts.checks]]
  type = "Http"
  name = "Website Frontend"  # Shows as "Website Frontend" in metrics
  port = 80
  path = "/"
  protocol = "Http"
  method = "Get"
  
  [[hosts.checks]]
  type = "Http"
  name = "Admin Panel"  # Shows as "Admin Panel" in metrics
  port = 443
  path = "/admin"
  protocol = "Https"
  method = "Get"
  
  [[hosts.checks]]
  type = "Tcp"
  name = "SSH Access"  # Shows as "SSH Access" in metrics
  port = 22
```

### Advanced HTTP Configuration
```toml
[[hosts]]
address = "httpbin.org"
alias = "HTTPBin API Test"

  [[hosts.checks]]
  type = "Http"
  port = 443
  path = "/get?param=value"
  protocol = "Https"
  method = "Get"
  timeout_seconds = 30
  expected_status_code = 200
  check_ssl_certificate = true
  
  # Authentication
  [hosts.checks.auth]
  Basic = { username = "user", password = "pass" }
  # OR Bearer = { token = "your-token" }
  # OR OAuth2 = { client_id = "id", client_secret = "secret", token_url = "url" }
  
  # Custom Headers
  [hosts.checks.headers]
  "User-Agent" = "UptimeMonitor/1.0"
  "Accept" = "application/json"
  
  # Assertions
  [[hosts.checks.assertions]]
  query = "Status"
  predicate = "Equals"
  value = { Integer = 200 }
  
  [[hosts.checks.assertions]]
  query = { JsonPath = { path = "$.url" } }
  predicate = "Contains"
  value = { String = "httpbin.org" }
  
  [[hosts.checks.assertions]]
  query = { Header = { name = "Content-Type" } }
  predicate = "Contains"
  value = { String = "application/json" }
```

### TCP Configuration
```toml
[[hosts]]
address = "1.1.1.1"
alias = "Cloudflare DNS"

  [[hosts.checks]]
  type = "Tcp"
  port = 53
  timeout_seconds = 5
```

### Service Health Check Configuration

#### PostgreSQL Database Monitoring
```toml
[[hosts]]
address = "postgres.example.com"
alias = "Production PostgreSQL"

  [[hosts.checks]]
  type = "Postgres"
  port = 5432
  username = "monitoring_user"
  password = "secure_password"
  database = "production_db"
  ssl_mode = "Require"
  timeout_seconds = 10
```

#### Redis Cache Monitoring
```toml
[[hosts]]
address = "redis.example.com"
alias = "Redis Cache"

  [[hosts.checks]]
  type = "Redis"
  port = 6379
  password = "redis_password"
  database = 0
  timeout_seconds = 5
```

#### MySQL Database Monitoring
```toml
[[hosts]]
address = "mysql.example.com"
alias = "MySQL Database"

  [[hosts.checks]]
  type = "MySQL"
  port = 3306
  username = "monitor"
  password = "mysql_password"
  database = "health_check"
  use_ssl = true
  timeout_seconds = 10
```

#### MongoDB Monitoring
```toml
[[hosts]]
address = "mongo.example.com"
alias = "MongoDB Cluster"

  [[hosts.checks]]
  type = "MongoDB"
  port = 27017
  username = "monitoring"
  password = "mongo_password"
  database = "admin"
  use_ssl = false
  timeout_seconds = 15
```

#### RabbitMQ Message Queue Monitoring
```toml
[[hosts]]
address = "rabbitmq.example.com"
alias = "RabbitMQ Server"

  [[hosts.checks]]
  type = "RabbitMQ"
  port = 5672
  username = "monitor"
  password = "rabbit_password"
  vhost = "/"
  use_ssl = false
  timeout_seconds = 10
```

#### Kafka Cluster Monitoring
```toml
[[hosts]]
address = "kafka.example.com"
alias = "Kafka Cluster"

  [[hosts.checks]]
  type = "Kafka"
  port = 9092
  topic = "health-check"
  use_ssl = false
  timeout_seconds = 15
```

#### Elasticsearch Monitoring
```toml
[[hosts]]
address = "elasticsearch.example.com"
alias = "Elasticsearch Cluster"

  [[hosts.checks]]
  type = "Elasticsearch"
  port = 9200
  username = "elastic"
  password = "elastic_password"
  use_ssl = true
  timeout_seconds = 10
```

## 🧪 Testing

### Unit Tests
```bash
# Run all unit tests
cargo test

# Run specific test suite
cargo test --test assertion_tests
cargo test --test httpbin_simplified_tests
```

### Integration Tests with Real HTTP Requests
```bash
# Run tests that make real HTTP requests to httpbin.org
cargo test --test httpbin_real_tests -- --ignored

# Run specific real HTTP test
cargo test --test httpbin_real_tests test_httpbin_get_real_request -- --ignored
```

### Test Coverage
- **Unit Tests**: 53+ tests covering core functionality
- **Assertion Tests**: 12 tests for assertion logic validation
- **Configuration Tests**: 29 tests for configuration validation
- **Real HTTP Tests**: 16 tests making actual HTTP requests to httpbin.org
- **Service Health Tests**: Comprehensive testing for all 7 service types
- **Integration Tests**: End-to-end monitoring workflow testing

### HTTPBin Test Suite
The project includes comprehensive tests against httpbin.org covering:
- ✅ All HTTP methods (GET, POST, PUT, DELETE, HEAD, OPTIONS)
- ✅ Authentication (Basic Auth, Bearer Token, OAuth2)
- ✅ Status code testing (200, 201, 400, 404, 500, etc.)
- ✅ Header manipulation and validation
- ✅ JSON/XML/HTML content type handling
- ✅ Response time and delay testing
- ✅ Redirect handling
- ✅ UUID, IP address, and date validation
- ✅ Error handling and edge cases

## 🏗️ Architecture

### Core Components
- **Config Module** (`src/config.rs`) - Configuration parsing and validation
- **Monitoring Module** (`src/monitoring.rs`) - Core monitoring logic, assertion evaluation, and service health checks
- **API Module** (`src/api.rs`) - Prometheus metrics and SVG badge HTTP endpoints
- **Main Module** (`src/main.rs`) - Application entry point and coordination

### Service Health Check Architecture
- **Async Connection Testing** - Non-blocking database and service connectivity
- **Service-Specific Validation** - Each service type has tailored health checks
- **Version Detection** - Automatic service version identification where possible
- **SSL/TLS Support** - Secure connections for supported services
- **Connection Pooling Ready** - Efficient resource management for high-frequency checks

### Key Features
- **Async/Await** - Built on Tokio for high-performance async I/O
- **Type Safety** - Leverages Rust's type system for reliable monitoring
- **Memory Efficient** - Automatic cleanup of historical data
- **Extensible** - Easy to add new assertion types and monitoring protocols
- **Production Ready** - Comprehensive service health monitoring for modern infrastructure
- **Single Binary** - No external dependencies, easy deployment

## 📚 Examples

### Monitor a REST API with JSON Assertions
```toml
[[hosts]]
address = "api.example.com"
alias = "Example API"

  [[hosts.checks]]
  type = "Http"
  port = 443
  path = "/api/v1/health"
  protocol = "Https"
  method = "Get"
  timeout_seconds = 15
  
  [hosts.checks.headers]
  "Authorization" = "Bearer your-api-token"
  "Accept" = "application/json"
  
  [[hosts.checks.assertions]]
  query = "Status"
  predicate = "Equals"
  value = { Integer = 200 }
  
  [[hosts.checks.assertions]]
  query = { JsonPath = { path = "$.status" } }
  predicate = "Equals"
  value = { String = "healthy" }
  
  [[hosts.checks.assertions]]
  query = { JsonPath = { path = "$.response_time_ms" } }
  predicate = "LessThan"
  value = { Integer = 1000 }
```

### Monitor Database with Service Health Checks
```toml
[[hosts]]
address = "db.example.com"
alias = "Production Database"

  # PostgreSQL health check with actual database connectivity
  [[hosts.checks]]
  type = "Postgres"
  port = 5432
  username = "health_check_user"
  password = "secure_password"
  database = "production"
  ssl_mode = "Require"
  timeout_seconds = 10

  # Fallback TCP check for basic connectivity
  [[hosts.checks]]
  type = "Tcp"
  port = 5432
  timeout_seconds = 5
```

### SSL Certificate Monitoring
```toml
[[hosts]]
address = "secure.example.com"
alias = "Secure Website"

  [[hosts.checks]]
  type = "Http"
  port = 443
  path = "/"
  protocol = "Https"
  method = "Get"
  check_ssl_certificate = true
  
  [[hosts.checks.assertions]]
  query = { Certificate = { field = "NotAfter" } }
  predicate = "IsIsoDate"
  value = { String = "" }
```

### Complete Infrastructure Monitoring Setup
```toml
# Comprehensive monitoring configuration for a typical web application stack
monitoring_interval_seconds = 60
memory_cleanup_interval_minutes = 60
keep_history_hours = 24

# Web Application Frontend
[[hosts]]
address = "app.example.com"
alias = "Frontend Application"
  [[hosts.checks]]
  type = "Http"
  port = 443
  path = "/health"
  protocol = "Https"
  method = "Get"
  timeout_seconds = 10
  expected_status_code = 200
  check_ssl_certificate = true

# API Backend
[[hosts]]
address = "api.example.com"
alias = "Backend API"
  [[hosts.checks]]
  type = "Http"
  port = 443
  path = "/api/health"
  protocol = "Https"
  method = "Get"
  timeout_seconds = 15
  [hosts.checks.headers]
  "Accept" = "application/json"
  [[hosts.checks.assertions]]
  query = { JsonPath = { path = "$.status" } }
  predicate = "Equals"
  value = { String = "healthy" }

# PostgreSQL Primary Database
[[hosts]]
address = "db-primary.example.com"
alias = "Primary Database"
  [[hosts.checks]]
  type = "Postgres"
  port = 5432
  username = "monitor"
  password = "monitoring_password"
  database = "production"
  ssl_mode = "Require"
  timeout_seconds = 10

# Redis Cache
[[hosts]]
address = "cache.example.com"
alias = "Redis Cache"
  [[hosts.checks]]
  type = "Redis"
  port = 6379
  password = "redis_password"
  database = 0
  timeout_seconds = 5

# Message Queue
[[hosts]]
address = "queue.example.com"
alias = "RabbitMQ Queue"
  [[hosts.checks]]
  type = "RabbitMQ"
  port = 5672
  username = "monitor"
  password = "queue_password"
  vhost = "production"
  use_ssl = false
  timeout_seconds = 10

# Search Engine
[[hosts]]
address = "search.example.com"
alias = "Elasticsearch"
  [[hosts.checks]]
  type = "Elasticsearch"
  port = 9200
  username = "elastic"
  password = "elastic_password"
  use_ssl = true
  timeout_seconds = 15
```

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Add comprehensive tests
4. Ensure all tests pass: `cargo test`
5. Submit a pull request

## 📄 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🔗 Related Projects

- [Uptime Kuma](https://github.com/louislam/uptime-kuma) - The inspiration for this project
- [Prometheus](https://prometheus.io/) - Metrics collection and alerting
- [HTTPBin](https://httpbin.org/) - HTTP testing service used in our test suite


---

## Protocols, HTTP options, groups/tags, i18n and domain expiry

A full annotated example is in [`config.new-features.example.toml`](config.new-features.example.toml).

**New check types:** `Ping` (system `ping`), `Dns` (A/AAAA/CNAME/MX/NS/TXT/SRV/CAA/PTR, optional resolver and expected value),
`Push` (passive heartbeat via `GET|POST /api/push/{token}`), `WebSocket`, `Smtp`, `Ntp`, `Mqtt`,
`Grpc` (standard `grpc.health.v1` health check, h2c or TLS),
`Snmp` (v2c GET of one OID, optional expected value), `Docker` (container running + healthcheck state via the Engine API;
mount `/var/run/docker.sock` or use `endpoint = "tcp://host:2375"`).

**HTTP options:** request `body`, `keyword` / `invert_keyword`, `accepted_status_codes` (ranges), redirects
(`follow_redirects`, `max_redirects`), `proxy` (HTTP/SOCKS5), `ca_cert_path`, `client_identity_path` (mTLS),
`ignore_tls_errors`, `upside_down`, `user_agent`.

> **Behaviour changes in the HTTP check:** it now runs on `reqwest` (rustls). TLS certificates are **verified by default**
> (previously never verified) — set `ignore_tls_errors = true` for self-signed targets. Redirects are followed by default.
> Chunked / compressed responses are now decoded. The OpenSSL probe still supplies `monitor_cert_days_remaining`.

**Groups & tags:** set `group` and `tags` on a host. Exposed via `GET /api/monitors?tag=…&group=…` and the metrics
`monitor_info`, `monitor_group_status`.

**i18n:** badges, `/badges` and `/api/monitors` text are localised (en, zh-TW, zh-CN, ja, ko, es, de, fr) using
`?lang=` → `Accept-Language` → `default_language`.

**Domain expiry:** `check_domain_expiry = true` on a host queries RDAP and exports `monitor_domain_days_remaining`
(and the `domain_expiry_days` field in the API). Some ccTLDs do not publish RDAP; those hosts simply report nothing.

> **Metric fix:** `monitor_consecutive_failures` previously counted every failed check twice. It now counts each failed
> check once, so existing alert thresholds on this metric effectively double in sensitivity-to-time (e.g. `>= 6` used to fire
> after 3 failed checks, now after 6). The `Target … recovered after N failures` log line, which never fired before, now works.
