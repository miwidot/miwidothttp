# Configuration Reference

Complete reference for all configuration options in miwidothttp.

## Configuration File Format

Configuration uses TOML format. The main configuration file is typically named `config.toml`.

```toml
# This is a comment
[section]
key = "value"

[[array_section]]
item = 1

[[array_section]]
item = 2
```

## Server Configuration

### Basic Server Settings

```toml
[server]
# HTTP port to listen on
http_port = 8080

# HTTPS port to listen on  
https_port = 8443

# Enable HTTPS server
enable_https = true

# Number of worker threads (0 = auto-detect CPU cores)
workers = 0

# Maximum concurrent connections
max_connections = 1000000

# Connection timeout in seconds
connection_timeout = 30

# Keep-alive timeout in seconds
keep_alive_timeout = 75

# Request timeout in seconds
request_timeout = 60

# Maximum request body size in bytes
body_size_limit = 104857600  # 100MB

# Server identification header
server_header = "miwidothttp/1.0"

# Enable HTTP/2
http2 = true

# Enable WebSocket support
websocket = true
```

### Network Settings

```toml
[server.network]
# TCP_NODELAY - disable Nagle's algorithm
tcp_nodelay = true

# TCP keep-alive
tcp_keepalive = true

# Socket buffer sizes
socket_buffer_size = 65536

# Accept backlog
accept_backlog = 1024

# SO_REUSEPORT - allow multiple sockets on same port
reuse_port = true

# SO_REUSEADDR - allow reuse of local addresses
reuse_address = true

# IP version preference
ip_version = "both"  # "v4", "v6", "both"
```

## SSL/TLS Configuration

### Cloudflare Integration

```toml
[ssl]
# SSL provider
provider = "cloudflare"  # "cloudflare", "letsencrypt", "manual"

# Auto-renew certificates
auto_renew = true

# Days before expiry to renew
renewal_days_before_expiry = 30

# Certificate storage directory
cert_dir = "/etc/miwidothttp/certs"

[ssl.cloudflare]
# Cloudflare API token (requires Zone:SSL:Edit permission)
api_token = "your-cloudflare-api-token"

# Cloudflare Zone ID
zone_id = "your-zone-id"

# Cloudflare Account ID (optional)
account_id = "your-account-id"

# Certificate validity in days
cert_validity_days = 90

# Certificate type
cert_type = "origin"  # "origin" or "edge"
```

### Manual SSL Configuration

```toml
[ssl.domains."example.com"]
# Path to certificate file
cert_file = "/path/to/cert.pem"

# Path to private key file
key_file = "/path/to/key.pem"

# Path to CA bundle (optional)
ca_file = "/path/to/ca.pem"

# Force HTTPS redirect
force_https = true

# HSTS settings
hsts_enabled = true
hsts_max_age = 31536000
hsts_include_subdomains = true
hsts_preload = true
```

### TLS Settings

```toml
[ssl.tls]
# Minimum TLS version
min_version = "1.2"  # "1.0", "1.1", "1.2", "1.3"

# Maximum TLS version
max_version = "1.3"

# Cipher suites (in order of preference)
cipher_suites = [
    "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
    "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
    "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256",
    "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
    "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
    "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256"
]

# Enable OCSP stapling
ocsp_stapling = true

# Session resumption
session_resumption = true
session_cache_size = 10000
session_timeout = 300  # seconds
```

## Virtual Hosts

```toml
[[vhosts]]
# Domain names (supports wildcards)
domains = ["example.com", "www.example.com", "*.example.com"]

# Priority (higher = higher priority)
priority = 100

# Document root for static files
root = "/var/www/example"

# Index files
index_files = ["index.html", "index.htm"]

# Enable directory listing
directory_listing = false

# Custom error pages
[vhosts.error_pages]
404 = "/errors/404.html"
500 = "/errors/500.html"
503 = "/errors/maintenance.html"

# Backend configuration
[vhosts.backend]
# Backend type
type = "proxy"  # "proxy", "static", "process", "redirect"

# Proxy target (for type="proxy")
target = "http://localhost:3000"

# Health check
health_check_path = "/health"
health_check_interval = 30
health_check_timeout = 5
health_check_healthy_threshold = 2
health_check_unhealthy_threshold = 3

# Load balancing (for multiple targets)
[vhosts.backend.load_balancing]
strategy = "round_robin"  # "round_robin", "least_conn", "ip_hash", "weighted"
targets = [
    { url = "http://backend1:8080", weight = 100 },
    { url = "http://backend2:8080", weight = 100 },
    { url = "http://backend3:8080", weight = 150 }
]

# Process backend (for type="process")
[vhosts.backend.process]
type = "nodejs"  # "nodejs", "python", "tomcat", "custom"
command = "node"
args = ["server.js"]
working_dir = "/app"
port = 3000

[vhosts.backend.process.env]
NODE_ENV = "production"
PORT = "3000"

# Redirect backend (for type="redirect")
[vhosts.backend.redirect]
target = "https://new-site.com"
status_code = 301  # 301, 302, 307, 308
preserve_path = true
preserve_query = true
```

## URL Rewriting

```toml
[[vhosts.rewrites]]
# Regex pattern to match
pattern = "^/old/(.*)"

# Replacement string (supports backreferences)
replacement = "/new/$1"

# Rewrite flags
flags = ["L", "R=301"]  # L=last, R=redirect, P=proxy, F=forbidden, G=gone

# Conditions (all must match)
[[vhosts.rewrites.conditions]]
type = "method"  # "method", "header", "query", "cookie", "file"
value = "GET"

[[vhosts.rewrites.conditions]]
type = "header"
name = "User-Agent"
pattern = ".*Mobile.*"
negate = false  # Invert the condition
```

## Session Management

```toml
[sessions]
# Session store type
store = "redis"  # "memory", "file", "redis"

# Session cookie name
cookie_name = "session_id"

# Cookie settings
cookie_http_only = true
cookie_secure = true
cookie_same_site = "Strict"  # "Strict", "Lax", "None"
cookie_domain = ".example.com"
cookie_path = "/"

# Session TTL in seconds
ttl_seconds = 3600

# Cleanup interval in seconds
cleanup_interval = 300

# Session ID length
id_length = 32

# Enable CSRF protection
csrf_protection = true
csrf_token_name = "csrf_token"
csrf_header_name = "X-CSRF-Token"

# Memory store settings (for store="memory")
[sessions.memory]
max_size = 10000
eviction_policy = "lru"  # "lru", "lfu", "random"

# File store settings (for store="file")
[sessions.file]
directory = "/var/lib/miwidothttp/sessions"
prefix = "sess_"

# Redis store settings (for store="redis")
[sessions.redis]
url = "redis://localhost:6379"
db = 0
key_prefix = "session:"
pool_size = 50
password = "optional-password"
```

## Proxy Configuration

```toml
[proxy]
# Proxy mode
mode = "reverse"  # "reverse", "forward", "transparent", "socks4", "socks5"

# Preserve host header
preserve_host = true

# Add forwarded headers
add_forwarded_headers = true
add_real_ip = true
add_proxy_headers = true

# Remove headers
remove_headers = ["Connection", "Upgrade"]

# Add custom headers
[proxy.headers]
"X-Proxy-Server" = "miwidothttp"
"X-Request-ID" = "${request_id}"

# Connection pooling
[proxy.connection_pool]
max_idle_per_host = 32
idle_timeout_seconds = 90
max_lifetime_seconds = 3600
http2 = true
keep_alive = true

# Timeouts
[proxy.timeout]
connect_timeout_seconds = 10
read_timeout_seconds = 30
write_timeout_seconds = 30
idle_timeout_seconds = 90

# Limits
[proxy.limits]
max_request_size = 104857600  # 100MB
max_response_size = 104857600  # 100MB
max_concurrent_connections = 10000
rate_limit_per_ip = 1000
bandwidth_limit_kbps = 10000

# Forward proxy settings
[proxy.forward]
bind_addr = "0.0.0.0:3128"
auth_required = true

[proxy.forward.auth]
type = "basic"  # "basic", "digest", "ntlm"
username = "proxyuser"
password = "proxypass"
realm = "Proxy Authentication"

# SOCKS proxy settings
[proxy.socks]
bind_addr = "0.0.0.0:1080"
version = 5  # 4 or 5
auth_required = true
username = "socksuser"
password = "sockspass"

# Upstream proxy chaining
[proxy.upstream]
url = "http://upstream-proxy:8080"
auth_username = "user"
auth_password = "pass"
use_for_https = true
```

## Cluster Configuration

```toml
[cluster]
# Enable clustering
enabled = true

# Unique node identifier
node_id = "node-1"

# Cluster name
cluster_name = "production"

# Gossip protocol bind address
bind_addr = "0.0.0.0:7946"

# Address advertised to other nodes
advertise_addr = "192.168.1.100:7946"

# gRPC port for inter-node communication
grpc_port = 7947

# Seed nodes for cluster discovery
seed_nodes = [
    "192.168.1.101:7946",
    "192.168.1.102:7946",
    "192.168.1.103:7946"
]

# Timing configuration
gossip_interval_ms = 1000
heartbeat_interval_ms = 5000
election_timeout_ms = 30000
data_sync_interval_ms = 10000

# Replication settings
replication_factor = 3
quorum_size = 2

# Auto-join cluster on startup
enable_auto_join = true

# Enable automatic failover
enable_auto_failover = true

# Enable automatic rebalancing
enable_auto_rebalance = true

# Service discovery
[cluster.discovery]
method = "etcd"  # "etcd", "consul", "dns", "static"

[cluster.discovery.etcd]
endpoints = [
    "http://etcd1:2379",
    "http://etcd2:2379",
    "http://etcd3:2379"
]
prefix = "/miwidothttp"
ttl = 60

# Distribution strategy
[cluster.distribution]
algorithm = "consistent_hash"  # "consistent_hash", "rendezvous", "jump_hash", "maglev"
virtual_nodes = 150
hash_function = "xxhash"  # "xxhash", "murmur3", "siphash"

# Node weights
[cluster.distribution.weights]
"node-1" = 1.0
"node-2" = 1.5
"node-3" = 1.0

# Consensus settings (Raft)
[cluster.consensus]
enable_leader_election = true
election_timeout_min_ms = 150
election_timeout_max_ms = 300
heartbeat_interval_ms = 50
snapshot_interval = 1000
max_log_entries = 10000
```

## Rate Limiting

```toml
[rate_limiting]
# Enable rate limiting
enabled = true

# Algorithm
algorithm = "token_bucket"  # "token_bucket", "sliding_window", "fixed_window"

# Storage backend
storage = "redis"  # "memory", "redis"

# Default limits
[rate_limiting.default]
requests_per_second = 100
requests_per_minute = 5000
requests_per_hour = 200000
burst_size = 200

# Custom rules
[[rate_limiting.rules]]
name = "api_endpoints"
path_pattern = "/api/*"
requests_per_second = 1000
burst_size = 2000
by = ["ip", "user_id"]  # Rate limit by these identifiers

[[rate_limiting.rules]]
name = "login_endpoint"
path_pattern = "/auth/login"
requests_per_minute = 10
burst_size = 5
by = ["ip"]

# Redis backend settings
[rate_limiting.redis]
url = "redis://localhost:6379"
key_prefix = "ratelimit:"
```

## Caching

```toml
[cache]
# Enable caching
enabled = true

# Cache backend
backend = "redis"  # "memory", "redis", "disk"

# Maximum cache size (MB)
max_size_mb = 1024

# Default TTL in seconds
ttl_seconds = 3600

# Cache key generation
include_query_string = true
include_headers = ["Accept", "Accept-Encoding"]
vary_headers = ["Accept-Language", "User-Agent"]

# Memory cache settings
[cache.memory]
eviction_policy = "lru"  # "lru", "lfu", "arc"
shards = 16  # Number of cache shards

# Redis cache settings
[cache.redis]
url = "redis://localhost:6379"
db = 1
key_prefix = "cache:"
pool_size = 50

# Disk cache settings
[cache.disk]
directory = "/var/cache/miwidothttp"
max_size_gb = 10
cleanup_interval = 3600

# Cache rules
[[cache.rules]]
path_pattern = "/static/*"
ttl_seconds = 86400  # 24 hours
cache_methods = ["GET", "HEAD"]

[[cache.rules]]
path_pattern = "/api/*"
ttl_seconds = 300  # 5 minutes
cache_methods = ["GET"]
respect_cache_control = true
```

## Logging

```toml
[logging]
# Log level
level = "info"  # "trace", "debug", "info", "warn", "error"

# Log format
format = "json"  # "json", "pretty", "compact", "common", "combined"

# Log outputs
[[logging.outputs]]
type = "stdout"
format = "pretty"
level = "info"

[[logging.outputs]]
type = "file"
path = "/var/log/miwidothttp/server.log"
format = "json"
level = "debug"

[[logging.outputs]]
type = "syslog"
endpoint = "localhost:514"
facility = "daemon"
level = "warn"

# Log rotation
[logging.rotation]
enabled = true
max_size_mb = 100
max_age_days = 30
max_backups = 10
compress = true

# Access log
[logging.access]
enabled = true
path = "/var/log/miwidothttp/access.log"
format = "combined"  # "common", "combined", "json", "custom"

# Custom format
[logging.access.custom]
format = '${remote_addr} - ${remote_user} [${time_local}] "${request}" ${status} ${body_bytes_sent} "${http_referer}" "${http_user_agent}" rt=${request_time}'

# Error log
[logging.error]
enabled = true
path = "/var/log/miwidothttp/error.log"
include_stack_trace = true
include_request_body = false
max_body_size = 4096
```

## Error Handling

```toml
[errors]
# Error mode
mode = "production"  # "development", "production", "maintenance"

# Show error details in responses
show_details = false

# Log errors
log_errors = true

# Custom error pages directory
templates_dir = "/etc/miwidothttp/error_pages"

# Custom error pages
[errors.pages]
400 = "400.html"
401 = "401.html"
403 = "403.html"
404 = "404.html"
429 = "429.html"
500 = "500.html"
502 = "502.html"
503 = "503.html"

# Error notifications
[errors.notifications]
enabled = true
webhook_url = "https://hooks.slack.com/services/YOUR/WEBHOOK"
email = "admin@example.com"
threshold = 10  # Send notification after N errors
interval = 300  # Wait N seconds between notifications

# Maintenance mode
[errors.maintenance]
enabled = false
message = "We're performing scheduled maintenance"
expected_duration = "2 hours"
allow_ips = ["192.168.1.0/24", "10.0.0.0/8"]
custom_page = "maintenance.html"
```

## Monitoring

```toml
[monitoring]
# Enable monitoring endpoints
enabled = true

# Metrics endpoint
metrics_endpoint = "/metrics"
metrics_format = "prometheus"  # "prometheus", "json"

# Health check endpoint
health_endpoint = "/health"

# Readiness endpoint
readiness_endpoint = "/ready"

# Prometheus settings
[monitoring.prometheus]
enabled = true
port = 9090
path = "/metrics"
namespace = "miwidothttp"
include_go_metrics = false

# Distributed tracing
[monitoring.tracing]
enabled = true
backend = "jaeger"  # "jaeger", "zipkin", "datadog"
sampling_rate = 0.001  # 0.1%
service_name = "miwidothttp"

[monitoring.tracing.jaeger]
agent_endpoint = "localhost:6831"
collector_endpoint = "http://jaeger:14268/api/traces"

# Custom metrics
[[monitoring.custom_metrics]]
name = "business_metric"
type = "counter"  # "counter", "gauge", "histogram"
labels = ["endpoint", "method"]
```

## Security

```toml
[security]
# CORS settings
[security.cors]
enabled = true
allowed_origins = ["*"]
allowed_methods = ["GET", "POST", "PUT", "DELETE", "OPTIONS"]
allowed_headers = ["*"]
exposed_headers = ["X-Request-ID"]
allow_credentials = true
max_age = 3600

# CSRF protection
[security.csrf]
enabled = true
token_length = 32
token_name = "_csrf"
header_name = "X-CSRF-Token"
safe_methods = ["GET", "HEAD", "OPTIONS"]

# Security headers
[security.headers]
"X-Frame-Options" = "DENY"
"X-Content-Type-Options" = "nosniff"
"X-XSS-Protection" = "1; mode=block"
"Referrer-Policy" = "strict-origin-when-cross-origin"
"Content-Security-Policy" = "default-src 'self'"

# IP filtering
[security.ip_filter]
enabled = false
mode = "whitelist"  # "whitelist" or "blacklist"
whitelist = ["192.168.1.0/24", "10.0.0.0/8"]
blacklist = ["192.168.100.0/24"]

# Request validation
[security.validation]
max_header_size = 8192
max_headers = 100
max_uri_length = 8192
block_suspicious_patterns = true
suspicious_patterns = [
    "../../",
    "<script>",
    "SELECT * FROM"
]
```

## Performance Tuning

```toml
[performance]
# Thread pool settings
worker_threads = 16  # 2x CPU cores recommended
async_threads = 64

# Connection pool
connection_pool_size = 1000
connection_idle_timeout = 60

# Buffer sizes
read_buffer_size = 65536
write_buffer_size = 65536
max_header_buffer_size = 16384

# HTTP/2 settings
[performance.http2]
initial_stream_window_size = 2097152  # 2MB
initial_connection_window_size = 5242880  # 5MB
max_concurrent_streams = 1000
max_frame_size = 16384
max_header_list_size = 16384

# Memory settings
[performance.memory]
# Pre-allocate memory pools
preallocate_pools = true
pool_sizes = {
    small = 1024,   # 1KB blocks
    medium = 16384, # 16KB blocks
    large = 65536   # 64KB blocks
}

# Garbage collection tuning
gc_interval = 60  # seconds
gc_threshold_mb = 100

# CPU settings
[performance.cpu]
# CPU affinity for workers
cpu_affinity = true
# Pin workers to CPUs
pin_workers = [
    { worker = 0, cpu = 0 },
    { worker = 1, cpu = 1 },
    { worker = 2, cpu = 2 },
    { worker = 3, cpu = 3 }
]
```

## Environment Variables

All configuration values can be overridden using environment variables:

```bash
# Server settings
MIWIDOTHTTP_SERVER_HTTP_PORT=8080
MIWIDOTHTTP_SERVER_HTTPS_PORT=8443
MIWIDOTHTTP_SERVER_WORKERS=8

# SSL settings
MIWIDOTHTTP_SSL_CLOUDFLARE_API_TOKEN=your-token
MIWIDOTHTTP_SSL_CLOUDFLARE_ZONE_ID=your-zone-id

# Cluster settings
MIWIDOTHTTP_CLUSTER_ENABLED=true
MIWIDOTHTTP_CLUSTER_NODE_ID=node-1
MIWIDOTHTTP_CLUSTER_SEED_NODES=192.168.1.10:7946,192.168.1.11:7946

# Redis settings
MIWIDOTHTTP_REDIS_URL=redis://localhost:6379
MIWIDOTHTTP_REDIS_PASSWORD=secret
```

## Configuration Validation

Validate configuration before starting:

```bash
# Validate configuration file
miwidothttp --validate-config config.toml

# Dry run (validate and exit)
miwidothttp --dry-run --config config.toml

# Show effective configuration
miwidothttp --show-config --config config.toml
```

## Dynamic Configuration

Some settings can be changed at runtime via the management API:

```bash
# Update rate limits
curl -X PUT http://localhost:8080/config/rate_limiting \
  -H "Content-Type: application/json" \
  -d '{"requests_per_second": 200}'

# Add virtual host
curl -X POST http://localhost:8080/config/vhosts \
  -H "Content-Type: application/json" \
  -d '{"domains": ["new.example.com"], "backend": {"target": "http://localhost:4000"}}'

# Update log level
curl -X PUT http://localhost:8080/config/logging/level \
  -H "Content-Type: application/json" \
  -d '{"level": "debug"}'
```