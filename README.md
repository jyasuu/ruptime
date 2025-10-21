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

- `uptime_status_health` - Service health status (1=UP, 0=DOWN)
- `uptime_response_time_seconds` - Response time in seconds
- `uptime_consecutive_failures_total` - Count of consecutive failures
- `uptime_cert_expiry_seconds` - SSL certificate expiry time (HTTPS only)
- `uptime_cert_is_valid` - Certificate validity status (HTTPS only)

All metrics include labels for `target_alias`, `target_host`, and `check_type` (HTTP, TCP, Postgres, Redis, MySQL, MongoDB, RabbitMQ, Kafka, Elasticsearch).

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

