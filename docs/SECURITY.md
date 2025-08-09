# Security Architecture & Hardening

## Table of Contents
- [TLS/SSL Implementation](#tlsssl-implementation)
- [Request Security](#request-security)
- [Process Isolation](#process-isolation)
- [Cluster Security](#cluster-security)
- [Security Headers](#security-headers)
- [Attack Mitigation](#attack-mitigation)

## TLS/SSL Implementation

### Current Support

#### Cloudflare Origin CA
```toml
[ssl]
provider = "cloudflare"
api_token = "..." # Stored in environment variable
zone_id = "..."
auto_renew = true
renewal_days_before = 30
```

#### ACME/Let's Encrypt Support
```toml
[ssl]
provider = "acme"
acme_server = "https://acme-v02.api.letsencrypt.org/directory"
email = "admin@example.com"
agree_tos = true

[ssl.challenges]
# HTTP-01 Challenge
http_01_port = 80
http_01_path = "/.well-known/acme-challenge"

# DNS-01 Challenge (for wildcards)
dns_01_provider = "cloudflare" # or route53, gcloud, etc.
dns_01_credentials = "${DNS_API_KEY}"

# TLS-ALPN-01 Challenge
tls_alpn_01_port = 443
```

#### Manual Certificates
```toml
[ssl.domains."example.com"]
cert_file = "/etc/ssl/certs/example.com.crt"
key_file = "/etc/ssl/private/example.com.key"
chain_file = "/etc/ssl/certs/example.com.chain"
```

### HTTP/3 & QUIC Roadmap

**Current Status**: Not yet implemented
**Target**: Q2 2026

```toml
# Future configuration
[server.http3]
enabled = true
port = 443
congestion_control = "bbr"
max_idle_timeout = 30000
initial_max_data = 10485760
initial_max_stream_data = 1048576
```

**Implementation Plan**:
1. Integrate `quinn` crate for QUIC
2. Implement HTTP/3 using `h3` crate
3. Add 0-RTT resumption support
4. Implement connection migration

## Request Security

### Header Normalization & Limits

```rust
// src/security/headers.rs
pub struct HeaderSecurity {
    max_header_size: usize,        // Default: 8KB
    max_headers: usize,             // Default: 100
    max_header_name_len: usize,     // Default: 256
    max_header_value_len: usize,    // Default: 4KB
}

impl HeaderSecurity {
    pub fn normalize_headers(&self, headers: &mut HeaderMap) -> Result<()> {
        // Remove hop-by-hop headers
        headers.remove("connection");
        headers.remove("keep-alive");
        headers.remove("proxy-authenticate");
        headers.remove("proxy-authorization");
        headers.remove("te");
        headers.remove("trailers");
        headers.remove("transfer-encoding");
        headers.remove("upgrade");
        
        // Normalize header names (lowercase)
        // Validate header values (no control characters)
        // Deduplicate headers where appropriate
        
        // Enforce limits
        if headers.len() > self.max_headers {
            return Err(Error::TooManyHeaders);
        }
        
        for (name, value) in headers.iter() {
            if name.as_str().len() > self.max_header_name_len {
                return Err(Error::HeaderNameTooLong);
            }
            if value.len() > self.max_header_value_len {
                return Err(Error::HeaderValueTooLong);
            }
            // Check for header smuggling attempts
            if value.to_str()?.contains(['\r', '\n']) {
                return Err(Error::HeaderSmuggling);
            }
        }
        
        Ok(())
    }
}
```

### Body Size Limits

```toml
[security.limits]
max_body_size = 104857600  # 100MB default
max_multipart_size = 524288000  # 500MB for file uploads
max_json_depth = 64  # Prevent deep JSON recursion
max_form_fields = 1000
max_url_length = 8192
```

### Timeout Configuration

```toml
[security.timeouts]
# Slowloris protection
header_read_timeout = 10  # seconds to read headers
body_read_timeout = 30    # seconds to read body
write_timeout = 30         # seconds to write response
idle_timeout = 60          # keep-alive timeout
request_timeout = 120      # total request timeout

# Connection limits
max_connections_per_ip = 100
connection_rate_per_ip = 10  # per second
```

### Request Smuggling Defense

```rust
// src/security/smuggling.rs
pub struct AntiSmuggling {
    strict_content_length: bool,
    reject_ambiguous_requests: bool,
}

impl AntiSmuggling {
    pub fn validate_request(&self, req: &Request) -> Result<()> {
        // Check for conflicting headers
        if req.headers().contains_key("content-length") &&
           req.headers().contains_key("transfer-encoding") {
            if self.reject_ambiguous_requests {
                return Err(Error::AmbiguousRequest);
            }
            // Prefer Transfer-Encoding per RFC 7230
        }
        
        // Validate Content-Length
        if let Some(cl) = req.headers().get("content-length") {
            let cl_str = cl.to_str()?;
            // Check for multiple values
            if cl_str.contains(',') {
                return Err(Error::MultipleContentLength);
            }
            // Check for invalid characters
            if !cl_str.chars().all(|c| c.is_ascii_digit()) {
                return Err(Error::InvalidContentLength);
            }
        }
        
        // Validate Transfer-Encoding
        if let Some(te) = req.headers().get("transfer-encoding") {
            let te_str = te.to_str()?;
            // Check for obfuscation attempts
            if te_str.contains("chunk") && !te_str.contains("chunked") {
                return Err(Error::ObfuscatedTransferEncoding);
            }
        }
        
        Ok(())
    }
}
```

### PROXY Protocol Support

```rust
// src/security/proxy_protocol.rs
pub struct ProxyProtocol {
    enabled: bool,
    required: bool,
    trusted_proxies: Vec<IpAddr>,
}

impl ProxyProtocol {
    pub async fn parse_header(&self, stream: &mut TcpStream) -> Result<ClientInfo> {
        if !self.enabled {
            return Ok(ClientInfo::from_stream(stream));
        }
        
        // Read PROXY protocol header
        let mut buf = [0u8; 108]; // Max v1 header size
        stream.peek(&mut buf).await?;
        
        if &buf[0..6] == b"PROXY " {
            // Version 1
            self.parse_v1_header(&buf)
        } else if &buf[0..12] == b"\x0D\x0A\x0D\x0A\x00\x0D\x0A\x51\x55\x49\x54\x0A" {
            // Version 2
            self.parse_v2_header(&buf)
        } else if self.required {
            Err(Error::ProxyProtocolRequired)
        } else {
            Ok(ClientInfo::from_stream(stream))
        }
    }
}
```

### X-Forwarded-For Handling

```toml
[security.forwarded]
# Trust strategy
trust_proxy = true
trusted_proxies = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"]
# How many proxy hops to trust
trusted_hops = 2

# Header priority (first match wins)
client_ip_headers = [
    "CF-Connecting-IP",      # Cloudflare
    "X-Real-IP",             # Common proxy header
    "X-Forwarded-For",       # Standard
    "X-Client-IP",           # Some proxies
    "Forwarded",             # RFC 7239
]

# Security options
reject_private_ip = false  # Reject if client IP is private
require_proxy_headers = false  # Require proxy headers from trusted IPs
```

## Process Isolation

### Current Implementation

```rust
// src/process/isolation.rs
use nix::unistd::{setuid, setgid, chroot, Uid, Gid};
use nix::sys::signal;
use seccomp::{Context, Action, Syscall};

pub struct ProcessIsolation {
    config: IsolationConfig,
}

pub struct IsolationConfig {
    // User/Group dropping
    run_as_user: Option<String>,
    run_as_group: Option<String>,
    
    // Filesystem isolation
    chroot_dir: Option<PathBuf>,
    working_dir: PathBuf,
    
    // Resource limits
    max_memory: Option<usize>,
    max_cpu_percent: Option<f32>,
    max_open_files: Option<usize>,
    
    // Seccomp filtering
    enable_seccomp: bool,
    allowed_syscalls: Vec<String>,
}

impl ProcessIsolation {
    pub fn apply(&self) -> Result<()> {
        // Drop privileges
        if let Some(user) = &self.config.run_as_user {
            let uid = Uid::from_raw(get_uid_from_name(user)?);
            let gid = Gid::from_raw(get_gid_from_name(user)?);
            
            // Set supplementary groups
            initgroups(user, gid)?;
            
            // Change group first (while we still have privileges)
            setgid(gid)?;
            
            // Then change user
            setuid(uid)?;
        }
        
        // Chroot jail
        if let Some(root) = &self.config.chroot_dir {
            chroot(root)?;
            std::env::set_current_dir("/")?;
        }
        
        // Apply resource limits
        self.apply_resource_limits()?;
        
        // Apply seccomp filter
        if self.config.enable_seccomp {
            self.apply_seccomp_filter()?;
        }
        
        Ok(())
    }
    
    fn apply_seccomp_filter(&self) -> Result<()> {
        let mut ctx = Context::new(Action::Kill)?;
        
        // Allow basic syscalls needed for operation
        let allowed = vec![
            Syscall::read,
            Syscall::write,
            Syscall::open,
            Syscall::close,
            Syscall::stat,
            Syscall::fstat,
            Syscall::lstat,
            Syscall::poll,
            Syscall::brk,
            Syscall::mmap,
            Syscall::mprotect,
            Syscall::munmap,
            Syscall::rt_sigaction,
            Syscall::rt_sigprocmask,
            Syscall::ioctl,
            Syscall::nanosleep,
            Syscall::select,
            Syscall::sched_yield,
            Syscall::gettimeofday,
            Syscall::getpid,
            Syscall::socket,
            Syscall::connect,
            Syscall::accept,
            Syscall::sendto,
            Syscall::recvfrom,
            Syscall::bind,
            Syscall::listen,
            Syscall::clone,
            Syscall::execve,
            Syscall::exit,
            Syscall::exit_group,
            Syscall::futex,
            Syscall::epoll_create,
            Syscall::epoll_ctl,
            Syscall::epoll_wait,
        ];
        
        for syscall in allowed {
            ctx.allow_syscall(syscall)?;
        }
        
        ctx.load()?;
        Ok(())
    }
}
```

### Cgroups Integration

```rust
// src/process/cgroups.rs
use cgroups_rs::{cgroup_builder::CgroupBuilder, CgroupPid};

pub struct CgroupIsolation {
    cgroup_name: String,
    memory_limit: Option<i64>,
    cpu_quota: Option<i64>,
    io_weight: Option<u64>,
}

impl CgroupIsolation {
    pub fn create_cgroup(&self, pid: i32) -> Result<()> {
        let cgroup = CgroupBuilder::new(&self.cgroup_name)
            .memory()
                .memory_hard_limit(self.memory_limit.unwrap_or(512 * 1024 * 1024))
                .done()
            .cpu()
                .quota(self.cpu_quota.unwrap_or(100000))
                .period(100000)
                .done()
            .blkio()
                .weight(self.io_weight.unwrap_or(500))
                .done()
            .build()?;
        
        cgroup.add_task(CgroupPid::from(pid as u64))?;
        
        Ok(())
    }
}
```

### Process Configuration

```toml
[process.isolation]
# User/Group
run_as_user = "www-data"
run_as_group = "www-data"

# Filesystem
chroot = "/var/www/app"
working_dir = "/app"

# Resource limits
max_memory = "512M"
max_cpu_percent = 50
max_open_files = 10000

# Security
enable_seccomp = true
enable_apparmor = true
apparmor_profile = "miwidothttp-app"

# Cgroups
enable_cgroups = true
cgroup_parent = "miwidothttp.slice"
```

## Cluster Security

### Split-Brain Prevention

```rust
// src/cluster/consensus.rs
use raft::{Config, Raft, Storage};

pub struct ConsensusManager {
    raft: Raft<MemStorage>,
    quorum_size: usize,
    split_brain_detector: SplitBrainDetector,
}

pub struct SplitBrainDetector {
    last_leader_seen: Instant,
    leader_timeout: Duration,
    min_cluster_size: usize,
}

impl ConsensusManager {
    pub async fn handle_split_brain(&mut self) -> Result<()> {
        let active_nodes = self.get_active_nodes().await?;
        
        // Check if we have quorum
        if active_nodes.len() < self.quorum_size {
            warn!("Lost quorum: {} nodes active, {} required", 
                  active_nodes.len(), self.quorum_size);
            
            // Enter read-only mode
            self.enter_readonly_mode().await?;
            
            // Try to rejoin majority partition
            self.attempt_rejoin().await?;
        }
        
        // Detect multiple leaders (split-brain)
        let leaders = self.detect_leaders(&active_nodes).await?;
        if leaders.len() > 1 {
            error!("Split-brain detected: {} leaders found", leaders.len());
            
            // Step down if we're not in the majority partition
            if !self.in_majority_partition(&active_nodes) {
                self.raft.step_down()?;
            }
        }
        
        Ok(())
    }
    
    fn in_majority_partition(&self, nodes: &[NodeId]) -> bool {
        nodes.len() > self.quorum_size / 2
    }
}
```

### Membership Changes

```rust
// src/cluster/membership.rs
pub struct MembershipManager {
    config: ClusterConfig,
    state: Arc<RwLock<ClusterState>>,
}

impl MembershipManager {
    pub async fn add_node(&mut self, node: NodeInfo) -> Result<()> {
        // Verify node identity
        self.verify_node_identity(&node).await?;
        
        // Check cluster capacity
        if self.state.read().await.nodes.len() >= self.config.max_nodes {
            return Err(Error::ClusterFull);
        }
        
        // Use joint consensus for safe membership change
        self.start_joint_consensus(ConfigChange::AddNode(node.clone())).await?;
        
        // Wait for new node to catch up
        self.wait_for_node_sync(&node.id).await?;
        
        // Commit membership change
        self.commit_config_change().await?;
        
        Ok(())
    }
    
    async fn verify_node_identity(&self, node: &NodeInfo) -> Result<()> {
        // Verify TLS certificate
        let cert = node.get_certificate()?;
        cert.verify_hostname(&node.address)?;
        
        // Verify cluster token
        let token = node.auth_token.as_ref()
            .ok_or(Error::MissingAuthToken)?;
        self.verify_cluster_token(token)?;
        
        Ok(())
    }
}
```

### Configuration Quorum

```toml
[cluster.consensus]
# Raft configuration
election_timeout_min = 150  # ms
election_timeout_max = 300  # ms
heartbeat_interval = 50     # ms
max_append_entries = 64

# Quorum settings
min_cluster_size = 3
quorum_size = 2  # (n/2) + 1

# Split-brain handling
auto_rejoin = true
rejoin_delay = 30  # seconds
readonly_on_split = true

# Membership changes
max_nodes = 7
allow_auto_join = false
require_tls = true
cluster_token = "${CLUSTER_TOKEN}"
```

## Security Headers

### Default Security Headers

```rust
// src/security/headers.rs
pub fn apply_security_headers(response: &mut Response) {
    let headers = response.headers_mut();
    
    // HSTS
    headers.insert(
        "Strict-Transport-Security",
        "max-age=31536000; includeSubDomains; preload".parse().unwrap()
    );
    
    // Frame Options
    headers.insert(
        "X-Frame-Options",
        "DENY".parse().unwrap()
    );
    
    // Content Type Options
    headers.insert(
        "X-Content-Type-Options",
        "nosniff".parse().unwrap()
    );
    
    // XSS Protection
    headers.insert(
        "X-XSS-Protection",
        "1; mode=block".parse().unwrap()
    );
    
    // Referrer Policy
    headers.insert(
        "Referrer-Policy",
        "strict-origin-when-cross-origin".parse().unwrap()
    );
    
    // CSP
    headers.insert(
        "Content-Security-Policy",
        "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'".parse().unwrap()
    );
    
    // Permissions Policy
    headers.insert(
        "Permissions-Policy",
        "geolocation=(), microphone=(), camera=()".parse().unwrap()
    );
}
```

### Configurable Headers

```toml
[security.headers]
# Standard security headers
hsts_enabled = true
hsts_max_age = 31536000
hsts_include_subdomains = true
hsts_preload = true

frame_options = "DENY"  # or "SAMEORIGIN"
content_type_options = "nosniff"
xss_protection = "1; mode=block"
referrer_policy = "strict-origin-when-cross-origin"

# Content Security Policy
csp_enabled = true
csp_report_only = false
csp_directives = """
    default-src 'self';
    script-src 'self' 'unsafe-inline' https://cdn.example.com;
    style-src 'self' 'unsafe-inline';
    img-src 'self' data: https:;
    font-src 'self' data:;
    connect-src 'self' wss://example.com;
    frame-ancestors 'none';
    base-uri 'self';
    form-action 'self';
    upgrade-insecure-requests;
"""

# Custom headers
[security.headers.custom]
"X-Custom-Header" = "value"
"X-Powered-By" = ""  # Remove header
```

## Attack Mitigation

### DDoS Protection

```rust
// src/security/ddos.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DDoSProtection {
    connection_limiter: Arc<RwLock<ConnectionLimiter>>,
    syn_cookie: SynCookieValidator,
    rate_limiter: Arc<RwLock<RateLimiter>>,
}

pub struct ConnectionLimiter {
    max_connections_per_ip: usize,
    connections: HashMap<IpAddr, usize>,
    blacklist: HashSet<IpAddr>,
    whitelist: HashSet<IpAddr>,
}

impl DDoSProtection {
    pub async fn check_connection(&self, addr: IpAddr) -> Result<()> {
        // Check whitelist
        if self.is_whitelisted(addr).await {
            return Ok(());
        }
        
        // Check blacklist
        if self.is_blacklisted(addr).await {
            return Err(Error::Blacklisted);
        }
        
        // Check connection limit
        let mut limiter = self.connection_limiter.write().await;
        let count = limiter.connections.entry(addr).or_insert(0);
        
        if *count >= limiter.max_connections_per_ip {
            // Add to temporary blacklist
            limiter.blacklist.insert(addr);
            return Err(Error::TooManyConnections);
        }
        
        *count += 1;
        
        // Check rate limit
        if !self.rate_limiter.write().await.check_rate(addr) {
            return Err(Error::RateLimited);
        }
        
        Ok(())
    }
}
```

### Slowloris Protection

```rust
// src/security/slowloris.rs
pub struct SlowlorisProtection {
    header_timeout: Duration,
    min_data_rate: usize,  // bytes per second
    incomplete_requests: Arc<RwLock<HashMap<SocketAddr, RequestState>>>,
}

pub struct RequestState {
    start_time: Instant,
    bytes_received: usize,
    headers_complete: bool,
}

impl SlowlorisProtection {
    pub async fn check_request_progress(&self, addr: SocketAddr) -> Result<()> {
        let mut requests = self.incomplete_requests.write().await;
        
        if let Some(state) = requests.get_mut(&addr) {
            let elapsed = state.start_time.elapsed();
            
            // Check header timeout
            if !state.headers_complete && elapsed > self.header_timeout {
                requests.remove(&addr);
                return Err(Error::HeaderTimeout);
            }
            
            // Check minimum data rate
            let rate = state.bytes_received as f64 / elapsed.as_secs_f64();
            if rate < self.min_data_rate as f64 {
                requests.remove(&addr);
                return Err(Error::SlowClient);
            }
        }
        
        Ok(())
    }
}
```

### Request Smuggling Prevention

```toml
[security.anti_smuggling]
# Strict parsing
strict_http_parsing = true
reject_invalid_headers = true
normalize_headers = true

# Ambiguous request handling
reject_ambiguous_requests = true  # CL + TE headers
prefer_transfer_encoding = true   # If both present

# Connection handling
max_request_per_connection = 1000
force_connection_close = false

# Validation
validate_content_length = true
max_chunk_size = 8192
reject_invalid_characters = true
```

## Security Monitoring

### Audit Logging

```toml
[security.audit]
enabled = true
log_level = "info"
log_file = "/var/log/miwidothttp/audit.log"

# What to log
log_authentication = true
log_authorization = true
log_configuration_changes = true
log_security_events = true
log_admin_actions = true

# Log format
format = "json"  # or "text"
include_request_body = false  # PII concern
include_response_body = false
include_headers = ["User-Agent", "Referer"]
exclude_headers = ["Authorization", "Cookie"]
```

### Intrusion Detection

```rust
// src/security/ids.rs
pub struct IntrusionDetection {
    rules: Vec<DetectionRule>,
    alert_threshold: u32,
    block_threshold: u32,
}

pub struct DetectionRule {
    name: String,
    pattern: Regex,
    severity: Severity,
    action: Action,
}

impl IntrusionDetection {
    pub fn scan_request(&self, req: &Request) -> Vec<Alert> {
        let mut alerts = Vec::new();
        
        // Check URI
        if let Some(alert) = self.check_uri(req.uri()) {
            alerts.push(alert);
        }
        
        // Check headers
        for (name, value) in req.headers() {
            if let Some(alert) = self.check_header(name, value) {
                alerts.push(alert);
            }
        }
        
        // Check common attack patterns
        alerts.extend(self.check_sql_injection(req));
        alerts.extend(self.check_xss(req));
        alerts.extend(self.check_path_traversal(req));
        alerts.extend(self.check_command_injection(req));
        
        alerts
    }
}
```

## Security Checklist

### Deployment Security Checklist

- [ ] TLS/SSL properly configured with strong ciphers
- [ ] Security headers enabled and configured
- [ ] Rate limiting enabled
- [ ] DDoS protection configured
- [ ] Process isolation enabled
- [ ] Audit logging enabled
- [ ] Intrusion detection rules configured
- [ ] Regular security updates scheduled
- [ ] Backup and recovery procedures tested
- [ ] Incident response plan documented
- [ ] Security monitoring dashboard setup
- [ ] Penetration testing completed

### Configuration Security

```toml
# Recommended production security configuration
[security]
enforce_https = true
min_tls_version = "1.2"
ciphers = [
    "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
    "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
    "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256",
    "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
]

[security.limits]
max_connections = 100000
max_connections_per_ip = 100
max_body_size = 10485760  # 10MB
max_header_size = 8192
max_headers = 100

[security.timeouts]
header_timeout = 10
body_timeout = 30
idle_timeout = 60
keepalive_timeout = 30

[security.rate_limiting]
enabled = true
requests_per_second = 100
burst_size = 200
```