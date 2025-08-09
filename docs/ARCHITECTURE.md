# Architecture Guide

## System Overview

miwidothttp is a high-performance HTTP server built with a modular, event-driven architecture optimized for concurrent request handling and horizontal scalability.

## Core Components

### 1. Network Layer

```
┌─────────────────────────────────────────┐
│           Network Layer                  │
├─────────────────────────────────────────┤
│  TCP Listener (Tokio)                   │
│  ├── HTTP/1.1 Handler                   │
│  ├── HTTP/2 Handler (h2)                │
│  └── WebSocket Handler (tungstenite)    │
└─────────────────────────────────────────┘
```

**Key Technologies:**
- **Tokio**: Async I/O runtime providing event loop and task scheduling
- **Axum**: Web framework built on Tower middleware system
- **Hyper**: Low-level HTTP implementation
- **Rustls**: TLS implementation in pure Rust

**Connection Flow:**
1. TCP connection accepted by Tokio listener
2. TLS handshake if HTTPS (with SNI routing)
3. HTTP protocol negotiation (ALPN)
4. Request parsed and routed through middleware pipeline

### 2. Request Processing Pipeline

```rust
Request → Middleware Stack → Handler → Response

Middleware Stack:
1. Logging/Tracing
2. Authentication
3. Rate Limiting
4. URL Rewriting
5. Caching
6. Compression
7. Error Handling
```

**Middleware Details:**

#### Logging Middleware
- Structured logging with `tracing`
- Configurable formats (JSON, Common, Combined)
- Automatic rotation and compression
- Request ID correlation

#### Authentication Middleware
- Session-based auth
- JWT token validation
- Basic/Bearer auth support
- CSRF protection

#### Rate Limiting
- Token bucket algorithm
- Distributed rate limiting via Redis
- Per-IP, per-user, per-endpoint limits
- Sliding window counters

#### URL Rewriting
- Regex-based pattern matching
- Capture groups and backreferences
- Conditional rewrites
- Flag support (L, R, P, F, G)

#### Caching
- Multi-tier cache (memory → Redis → disk)
- Content-based cache keys
- Vary header support
- Cache invalidation strategies

### 3. Virtual Host System

```rust
pub struct VirtualHost {
    pub domains: Vec<String>,        // Including wildcards
    pub priority: u32,               // Resolution order
    pub root: Option<PathBuf>,       // Static file root
    pub backend: Option<Backend>,    // Proxy target
    pub ssl: Option<SslConfig>,      // Per-host SSL
    pub rewrites: Vec<RewriteRule>,  // URL rewrites
    pub error_pages: HashMap<u16, String>,
}
```

**Domain Matching Algorithm:**
1. Exact match (highest priority)
2. Wildcard suffix match (`*.example.com`)
3. Wildcard prefix match (`api.*`)
4. Default host (lowest priority)

### 4. Backend Management

```rust
pub enum Backend {
    Proxy(ProxyBackend),      // Reverse proxy
    Process(ProcessBackend),   // Managed process
    Static(StaticBackend),     // File serving
    Redirect(RedirectBackend), // URL redirect
}
```

#### Process Management
- Spawns and monitors child processes
- Automatic restart on failure
- Health checking with configurable probes
- Resource limits (CPU, memory, file descriptors)
- Log aggregation from stdout/stderr

#### Proxy Engine
- Connection pooling with keep-alive
- Load balancing strategies:
  - Round-robin
  - Least connections
  - IP hash (sticky sessions)
  - Weighted distribution
- Circuit breaker pattern
- Retry with exponential backoff

### 5. SSL/TLS Management

```rust
pub struct SslManager {
    certificates: HashMap<String, Certificate>,
    cloudflare: CloudflareClient,
    renewal_queue: PriorityQueue<RenewalTask>,
}
```

**Certificate Lifecycle:**
1. **Generation**: Request from Cloudflare Origin CA
2. **Storage**: Save to disk and memory cache
3. **Loading**: Hot-reload without restart
4. **Renewal**: Automatic before expiration
5. **Rotation**: Zero-downtime certificate updates

**SNI Routing:**
- Extract hostname from TLS ClientHello
- Match against certificate store
- Fallback to default certificate
- Support for wildcard certificates

### 6. Session Management

```rust
pub trait SessionStore: Send + Sync {
    async fn load(&self, id: &str) -> Result<Session>;
    async fn save(&self, session: &Session) -> Result<()>;
    async fn delete(&self, id: &str) -> Result<()>;
    async fn cleanup(&self) -> Result<usize>;
}
```

**Implementations:**
- **MemoryStore**: Fast, single-node only
- **RedisStore**: Distributed, persistent
- **FileStore**: Simple, disk-based

**Security Features:**
- Secure session ID generation (256-bit)
- HttpOnly, Secure, SameSite cookies
- Session fixation protection
- CSRF token validation
- Automatic session cleanup

### 7. Cluster Architecture

```
┌──────────────────────────────────────────────┐
│              Cluster Node                    │
├──────────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌──────────┐ │
│  │ Gossip   │  │   Raft   │  │   gRPC   │ │
│  │ Protocol │  │Consensus │  │   RPC    │ │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘ │
│       │              │              │        │
│  ┌────▼──────────────▼──────────────▼────┐ │
│  │        Cluster Manager                 │ │
│  │  - Node Discovery                      │ │
│  │  - Leader Election                     │ │
│  │  - State Replication                   │ │
│  │  - Failure Detection                   │ │
│  └────────────────────────────────────────┘ │
└──────────────────────────────────────────────┘
```

#### Gossip Protocol (Chitchat)
- SWIM-based failure detection
- Eventual consistency for membership
- Low bandwidth overhead
- Scalable to thousands of nodes

#### Consensus (Raft)
- Leader election for coordination
- Log replication for state machine
- Linearizable consistency
- Split-brain prevention

#### Distribution Strategy
- Consistent hashing with virtual nodes
- Configurable replication factor
- Affinity rules for data locality
- Automatic rebalancing on topology changes

### 8. Error Handling

```rust
pub struct ErrorHandler {
    mode: ErrorMode,              // Development/Production
    templates: TemplateEngine,     // Custom error pages
    notifier: ErrorNotifier,       // Alerts/webhooks
    tracker: ErrorTracker,         // Metrics/logging
}
```

**Error Flow:**
1. Error captured in middleware
2. Classify error type and severity
3. Generate appropriate response
4. Log with context and stack trace
5. Trigger notifications if threshold exceeded
6. Update error metrics

### 9. Monitoring & Observability

```rust
pub struct Metrics {
    requests: Counter,
    latency: Histogram,
    connections: Gauge,
    errors: CounterVec,
    backend_health: GaugeVec,
}
```

**Metrics Collection:**
- Prometheus-compatible metrics
- Custom business metrics
- Distributed tracing (OpenTelemetry)
- Structured logging with correlation IDs

**Health Checks:**
- Liveness probe: Is the server running?
- Readiness probe: Can it handle requests?
- Backend health: Are dependencies available?

## Data Flow

### Request Lifecycle

```
1. Client → TCP Connection
2. TLS Handshake (if HTTPS)
3. HTTP Parser → Request Object
4. Virtual Host Matching
5. Middleware Pipeline
   a. Authentication Check
   b. Rate Limit Check
   c. URL Rewrite
   d. Cache Lookup
6. Route to Handler
   a. Static File
   b. Proxy Backend
   c. Process Backend
7. Generate Response
8. Response Middleware
   a. Compression
   b. Headers
   c. Logging
9. Send to Client
10. Connection Close/Keep-Alive
```

### Cluster Communication

```
Node A                Node B                Node C
  │                     │                     │
  ├──── Gossip ────────►├──── Gossip ────────►│
  │◄─── Heartbeat ──────┤◄─── Heartbeat ──────┤
  │                     │                     │
  ├──── Raft Vote ─────►│                     │
  │◄─── Vote Grant ─────┤                     │
  │                     │                     │
  ├──── Log Entry ─────►├──── Log Entry ─────►│
  │◄─── Ack ────────────┤◄─── Ack ────────────┤
```

## Performance Optimizations

### Memory Management
- Object pooling for connections
- Arena allocation for request processing
- Zero-copy I/O where possible
- Careful use of Arc/Rc for shared state

### CPU Optimization
- Work-stealing task scheduler
- CPU affinity for worker threads
- SIMD operations for parsing
- Branch prediction hints

### Network Optimization
- TCP_NODELAY for low latency
- SO_REUSEPORT for load distribution
- Kernel bypass with io_uring (Linux)
- Batched system calls

### Caching Strategy
- L1: Thread-local cache (no locks)
- L2: Shared memory cache (RwLock)
- L3: Redis cache (network)
- L4: Disk cache (SSD)

## Security Architecture

### Defense in Depth
1. **Network Level**: Rate limiting, DDoS protection
2. **Protocol Level**: TLS 1.2+, secure ciphers
3. **Application Level**: Input validation, CSRF
4. **Session Level**: Secure cookies, rotation
5. **Data Level**: Encryption at rest

### Threat Mitigation
- **SQL Injection**: Parameterized queries
- **XSS**: Content-Security-Policy headers
- **CSRF**: Token validation
- **Clickjacking**: X-Frame-Options
- **Man-in-the-Middle**: Certificate pinning

## Scalability Patterns

### Vertical Scaling
- Increase worker threads
- Tune buffer sizes
- Optimize kernel parameters
- Add more CPU/RAM

### Horizontal Scaling
- Add cluster nodes
- Distribute via consistent hashing
- Replicate for redundancy
- Geographic distribution

### Load Distribution
- DNS round-robin
- Hardware load balancer
- Software load balancer (HAProxy)
- Client-side load balancing

## Deployment Patterns

### Single Node
- Development/testing
- Small applications
- Edge locations

### Active-Passive
- Primary handles all traffic
- Secondary on standby
- Automatic failover

### Active-Active
- All nodes handle traffic
- Load distributed evenly
- No single point of failure

### Geographic Distribution
- Nodes in multiple regions
- Geo-DNS routing
- Cross-region replication
- Edge caching

## Extension Points

### Custom Middleware
```rust
pub trait Middleware {
    async fn process(&self, req: Request, next: Next) -> Response;
}
```

### Custom Backends
```rust
pub trait Backend {
    async fn handle(&self, req: Request) -> Result<Response>;
}
```

### Custom Session Stores
```rust
pub trait SessionStore {
    async fn load(&self, id: &str) -> Result<Session>;
    async fn save(&self, session: &Session) -> Result<()>;
}
```

### Custom Metrics
```rust
pub trait MetricsCollector {
    fn record(&self, metric: &Metric);
    fn export(&self) -> Vec<DataPoint>;
}
```

## Future Enhancements

### Planned Features
- HTTP/3 (QUIC) support
- GraphQL subscription handling
- gRPC proxying
- WebAssembly plugins
- Dynamic configuration via API
- Machine learning for anomaly detection

### Performance Goals
- 500k+ requests/second per node
- Sub-millisecond P99 latency
- 2M+ concurrent connections
- 100GB/s throughput

### Scalability Goals
- 1000+ node clusters
- Cross-region federation
- Edge computing support
- Serverless integration