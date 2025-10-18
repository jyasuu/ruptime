# help me implement uptime kuma alternative solution tool

## focus on prometheus metrics features

## support basic and oauth2 authencation

## support maintain time configuration

## support ssl expire day

## metrics data write disk durable is not require not like uptime kuma. just keep it in memory and clean for a while

## ui dashboard is not require .


## try compitable with grafana dashboard @uptime-kuma-dashboard.json or refactor it


## reference uptime kuma metrics
```
# HELP monitor_cert_days_remaining The number of days remaining until the certificate expires
# TYPE monitor_cert_days_remaining gauge
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/auth/realms/business/account/",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/auth/realms/pcg/account/",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/EinvoiceWeb/",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="https://example.com/services/mapc/info",monitor_type="http",monitor_url="https://example.com/services/mapc/info",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="https://example.com/services/vapc/info",monitor_type="http",monitor_url="https://example.com/services/vapc/info",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/gscmpurd/",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="http://example.com/purd/",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/gscm",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com",monitor_hostname="null",monitor_port="null"} 324
monitor_cert_days_remaining{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com",monitor_hostname="null",monitor_port="null"} 324

# HELP monitor_cert_is_valid Is the certificate still valid? (1 = Yes, 0= No)
# TYPE monitor_cert_is_valid gauge
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/auth/realms/business/account/",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/auth/realms/pcg/account/",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/EinvoiceWeb/",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="https://example.com/services/mapc/info",monitor_type="http",monitor_url="https://example.com/services/mapc/info",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="https://example.com/services/vapc/info",monitor_type="http",monitor_url="https://example.com/services/vapc/info",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/gscmpurd/",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="http://example.com/purd/",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/gscm",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com",monitor_hostname="null",monitor_port="null"} 1
monitor_cert_is_valid{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com",monitor_hostname="null",monitor_port="null"} 1

# HELP monitor_response_time Monitor Response Time (ms)
# TYPE monitor_response_time gauge
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} -1
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 27
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 29
monitor_response_time{monitor_name="example.com",monitor_type="port",monitor_url="https://",monitor_hostname="example.com",monitor_port="2010"} 1
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/auth/realms/business/account/",monitor_hostname="null",monitor_port="null"} 314
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/auth/realms/pcg/account/",monitor_hostname="null",monitor_port="null"} 397
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com",monitor_hostname="null",monitor_port="null"} 27
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/",monitor_hostname="null",monitor_port="null"} 17
monitor_response_time{monitor_name="example.com",monitor_type="port",monitor_url="https://",monitor_hostname="example.com",monitor_port="21"} 1
monitor_response_time{monitor_name="https://example.com/services/mapc/info",monitor_type="http",monitor_url="https://example.com/services/mapc/info",monitor_hostname="null",monitor_port="null"} -1
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/EinvoiceWeb/",monitor_hostname="null",monitor_port="null"} 24
monitor_response_time{monitor_name="https://example.com/services/vapc/info",monitor_type="http",monitor_url="https://example.com/services/vapc/info",monitor_hostname="null",monitor_port="null"} -1
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="http://example.com/purd/",monitor_hostname="null",monitor_port="null"} 211
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/gscmpurd/",monitor_hostname="null",monitor_port="null"} 203
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/gscm",monitor_hostname="null",monitor_port="null"} 218
monitor_response_time{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com",monitor_hostname="null",monitor_port="null"} 601

# HELP monitor_status Monitor Status (1 = UP, 0= DOWN, 2= PENDING, 3= MAINTENANCE)
# TYPE monitor_status gauge
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 0
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 1
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/scm_ocp_app",monitor_hostname="null",monitor_port="null"} 1
monitor_status{monitor_name="example.com",monitor_type="port",monitor_url="https://",monitor_hostname="example.com",monitor_port="2010"} 1
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/auth/realms/business/account/",monitor_hostname="null",monitor_port="null"} 1
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/auth/realms/pcg/account/",monitor_hostname="null",monitor_port="null"} 1
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com",monitor_hostname="null",monitor_port="null"} 1
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/",monitor_hostname="null",monitor_port="null"} 1
monitor_status{monitor_name="example.com",monitor_type="port",monitor_url="https://",monitor_hostname="example.com",monitor_port="21"} 1
monitor_status{monitor_name="https://example.com/services/mapc/info",monitor_type="http",monitor_url="https://example.com/services/mapc/info",monitor_hostname="null",monitor_port="null"} 0
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/EinvoiceWeb/",monitor_hostname="null",monitor_port="null"} 1
monitor_status{monitor_name="https://example.com/services/vapc/info",monitor_type="http",monitor_url="https://example.com/services/vapc/info",monitor_hostname="null",monitor_port="null"} 0
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="http://example.com/purd/",monitor_hostname="null",monitor_port="null"} 1
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/gscmpurd/",monitor_hostname="null",monitor_port="null"} 1
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com/gscm",monitor_hostname="null",monitor_port="null"} 1
monitor_status{monitor_name="example.com",monitor_type="http",monitor_url="https://example.com",monitor_hostname="null",monitor_port="null"} 1

# HELP process_cpu_user_seconds_total Total user CPU time spent in seconds.
# TYPE process_cpu_user_seconds_total counter
process_cpu_user_seconds_total 34427.87895300006

# HELP process_cpu_system_seconds_total Total system CPU time spent in seconds.
# TYPE process_cpu_system_seconds_total counter
process_cpu_system_seconds_total 5352.061869000055

# HELP process_cpu_seconds_total Total user and system CPU time spent in seconds.
# TYPE process_cpu_seconds_total counter
process_cpu_seconds_total 39779.94082199961

# HELP process_start_time_seconds Start time of the process since unix epoch in seconds.
# TYPE process_start_time_seconds gauge
process_start_time_seconds 1742096327

# HELP process_resident_memory_bytes Resident memory size in bytes.
# TYPE process_resident_memory_bytes gauge
process_resident_memory_bytes 117944320

# HELP process_virtual_memory_bytes Virtual memory size in bytes.
# TYPE process_virtual_memory_bytes gauge
process_virtual_memory_bytes 1017204736

# HELP process_heap_bytes Process heap size in bytes.
# TYPE process_heap_bytes gauge
process_heap_bytes 244449280

# HELP process_open_fds Number of open file descriptors.
# TYPE process_open_fds gauge
process_open_fds 25

# HELP process_max_fds Maximum number of open file descriptors.
# TYPE process_max_fds gauge
process_max_fds 1048576

# HELP app_version The service version by package.json
# TYPE app_version gauge
app_version{version="1.23.13",major="1",minor="23",patch="13"} 1

# HELP http_request_duration_seconds Duration of HTTP requests in seconds
# TYPE http_request_duration_seconds histogram

# HELP http_request_size_bytes Size of HTTP requests in bytes
# TYPE http_request_size_bytes histogram

# HELP http_response_size_bytes Size of HTTP response in bytes
# TYPE http_response_size_bytes histogram

# HELP expressjs_number_of_open_connections Number of open connections to the Express.js server
# TYPE expressjs_number_of_open_connections gauge
expressjs_number_of_open_connections 0
```