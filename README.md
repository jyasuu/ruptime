# Rust Uptime Monitoring Solution

This project is a lightweight uptime monitoring solution implemented in Rust. It provides a metrics API and supports configuration for monitoring hosts, ports, HTTP methods, protocols (HTTP, TCP, etc.), and SSL checks. UI and database are not included.

## Features
- Load configuration for monitoring targets (host, port, protocol, etc.).
- Supports HTTP and TCP protocols.
- SSL certificate validity checks.
- Exposes metrics via an API for integration with monitoring tools like Prometheus.

## Getting Started

### Prerequisites
- Rust (latest stable version)
- Linux OS

### Installation
1. Clone the repository:
   ```bash
   git clone <repository-url>
   cd ruptime
   ```
2. Build the project:
   ```bash
   cargo build --release
   ```

### Configuration
Create a `config.yaml` file in the root directory with the following structure:
```yaml
monitors:
  - name: "Example HTTP Monitor"
    host: "example.com"
    port: 443
    protocol: "http"
    method: "GET"
    ssl_check: true
  - name: "Example TCP Monitor"
    host: "example.com"
    port: 80
    protocol: "tcp"
    ssl_check: false
```

### Running the Application
Run the application with:
```bash
cargo run --release
```

### Metrics API
The application exposes metrics at `http://<host>:<port>/metrics` in Prometheus format.

## License
This project is licensed under the MIT License.


## prompts

```
help me learning iceberg and doris and how to design use them on application and show me a full practice example
```