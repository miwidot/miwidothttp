# API Documentation

## Table of Contents
- [Management API](#management-api)
- [Health & Metrics](#health--metrics)
- [Configuration API](#configuration-api)
- [Cluster API](#cluster-api)
- [Session API](#session-api)
- [WebSocket API](#websocket-api)
- [Authentication](#authentication)
- [Error Responses](#error-responses)

## Management API

### Base URL
```
http://localhost:8080/api/v1
```

### Endpoints

#### Server Status
```http
GET /api/v1/status
```

**Response:**
```json
{
  "status": "running",
  "version": "1.0.0",
  "uptime": 3600,
  "start_time": "2025-08-09T10:00:00Z",
  "pid": 12345,
  "workers": 8,
  "connections": {
    "active": 1234,
    "idle": 56,
    "total": 1290
  }
}
```

#### Server Configuration
```http
GET /api/v1/config
```

**Response:**
```json
{
  "server": {
    "http_port": 8080,
    "https_port": 8443,
    "workers": 8
  },
  "ssl": {
    "enabled": true,
    "provider": "cloudflare"
  }
}
```

#### Update Configuration
```http
PUT /api/v1/config
Content-Type: application/json

{
  "logging": {
    "level": "debug"
  }
}
```

**Response:**
```json
{
  "success": true,
  "message": "Configuration updated",
  "restart_required": false
}
```

#### Reload Server
```http
POST /api/v1/reload
```

**Response:**
```json
{
  "success": true,
  "message": "Server reloaded successfully"
}
```

#### Graceful Shutdown
```http
POST /api/v1/shutdown
Content-Type: application/json

{
  "timeout": 30,
  "force": false
}
```

## Health & Metrics

### Health Check
```http
GET /health
```

**Response:**
```json
{
  "status": "healthy",
  "timestamp": "2025-08-09T12:00:00Z",
  "checks": {
    "database": "ok",
    "redis": "ok",
    "disk_space": "ok",
    "memory": "ok"
  }
}
```

### Readiness Check
```http
GET /ready
```

**Response:**
```json
{
  "ready": true,
  "services": {
    "http": true,
    "https": true,
    "cluster": true
  }
}
```

### Prometheus Metrics
```http
GET /metrics
```

**Response:**
```
# HELP http_requests_total Total number of HTTP requests
# TYPE http_requests_total counter
http_requests_total{method="GET",status="200"} 12345

# HELP http_request_duration_seconds HTTP request latency
# TYPE http_request_duration_seconds histogram
http_request_duration_seconds_bucket{le="0.005"} 1234
http_request_duration_seconds_bucket{le="0.01"} 2345
```

### Custom Metrics
```http
GET /api/v1/metrics
```

**Response:**
```json
{
  "requests": {
    "total": 1000000,
    "per_second": 1500,
    "per_minute": 85000
  },
  "latency": {
    "p50": 0.8,
    "p95": 2.1,
    "p99": 3.2,
    "avg": 1.1
  },
  "bandwidth": {
    "in_bytes": 5000000000,
    "out_bytes": 10000000000,
    "in_rate": 100000,
    "out_rate": 200000
  },
  "errors": {
    "4xx": 1234,
    "5xx": 56,
    "rate": 0.001
  }
}
```

## Configuration API

### List Virtual Hosts
```http
GET /api/v1/vhosts
```

**Response:**
```json
{
  "vhosts": [
    {
      "id": "vhost-1",
      "domains": ["example.com", "www.example.com"],
      "priority": 100,
      "backend": {
        "type": "proxy",
        "target": "http://localhost:3000"
      },
      "ssl": {
        "enabled": true,
        "certificate": "valid"
      }
    }
  ]
}
```

### Add Virtual Host
```http
POST /api/v1/vhosts
Content-Type: application/json

{
  "domains": ["new.example.com"],
  "backend": {
    "type": "proxy",
    "target": "http://localhost:4000"
  }
}
```

### Update Virtual Host
```http
PUT /api/v1/vhosts/{id}
Content-Type: application/json

{
  "priority": 200,
  "backend": {
    "target": "http://localhost:5000"
  }
}
```

### Delete Virtual Host
```http
DELETE /api/v1/vhosts/{id}
```

### List Backends
```http
GET /api/v1/backends
```

**Response:**
```json
{
  "backends": [
    {
      "name": "app-1",
      "type": "nodejs",
      "status": "running",
      "health": "healthy",
      "pid": 12345,
      "uptime": 3600,
      "restarts": 0
    }
  ]
}
```

### Backend Health
```http
GET /api/v1/backends/{name}/health
```

**Response:**
```json
{
  "name": "app-1",
  "healthy": true,
  "last_check": "2025-08-09T12:00:00Z",
  "response_time": 15,
  "status_code": 200,
  "checks_passed": 100,
  "checks_failed": 0
}
```

### Restart Backend
```http
POST /api/v1/backends/{name}/restart
```

### Stop Backend
```http
POST /api/v1/backends/{name}/stop
```

### Start Backend
```http
POST /api/v1/backends/{name}/start
```

## Cluster API

### Cluster Status
```http
GET /api/v1/cluster/status
```

**Response:**
```json
{
  "enabled": true,
  "node_id": "node-1",
  "role": "leader",
  "cluster_name": "production",
  "nodes": [
    {
      "id": "node-1",
      "address": "192.168.1.10:7946",
      "state": "active",
      "role": "leader",
      "load": {
        "cpu": 45.2,
        "memory": 62.8,
        "connections": 1234
      }
    },
    {
      "id": "node-2",
      "address": "192.168.1.11:7946",
      "state": "active",
      "role": "follower",
      "load": {
        "cpu": 38.5,
        "memory": 55.3,
        "connections": 987
      }
    }
  ]
}
```

### List Cluster Nodes
```http
GET /api/v1/cluster/nodes
```

### Get Node Info
```http
GET /api/v1/cluster/nodes/{node-id}
```

### Join Cluster
```http
POST /api/v1/cluster/join
Content-Type: application/json

{
  "seed_nodes": ["192.168.1.10:7946"],
  "cluster_name": "production"
}
```

### Leave Cluster
```http
POST /api/v1/cluster/leave
```

### Force Leader Election
```http
POST /api/v1/cluster/election
```

### Rebalance Cluster
```http
POST /api/v1/cluster/rebalance
```

## Session API

### Get Session Info
```http
GET /api/v1/sessions/{session-id}
```

**Response:**
```json
{
  "id": "session-123",
  "created": "2025-08-09T10:00:00Z",
  "last_accessed": "2025-08-09T12:00:00Z",
  "expires": "2025-08-09T13:00:00Z",
  "data": {
    "user_id": "user-456",
    "ip": "192.168.1.100"
  }
}
```

### List Active Sessions
```http
GET /api/v1/sessions?active=true
```

**Response:**
```json
{
  "total": 1234,
  "sessions": [
    {
      "id": "session-123",
      "user_id": "user-456",
      "created": "2025-08-09T10:00:00Z",
      "last_accessed": "2025-08-09T12:00:00Z"
    }
  ]
}
```

### Invalidate Session
```http
DELETE /api/v1/sessions/{session-id}
```

### Clear All Sessions
```http
DELETE /api/v1/sessions
```

## WebSocket API

### WebSocket Endpoint
```
ws://localhost:8080/ws
```

### Connection
```javascript
const ws = new WebSocket('ws://localhost:8080/ws');

ws.onopen = () => {
  console.log('Connected to WebSocket');
  
  // Subscribe to events
  ws.send(JSON.stringify({
    type: 'subscribe',
    channels: ['metrics', 'logs', 'events']
  }));
};

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  console.log('Received:', data);
};

ws.onerror = (error) => {
  console.error('WebSocket error:', error);
};

ws.onclose = () => {
  console.log('WebSocket connection closed');
};
```

### Message Types

#### Subscribe to Channel
```json
{
  "type": "subscribe",
  "channels": ["metrics", "logs", "events"]
}
```

#### Unsubscribe from Channel
```json
{
  "type": "unsubscribe",
  "channels": ["logs"]
}
```

#### Ping/Pong
```json
{
  "type": "ping",
  "timestamp": 1234567890
}
```

Response:
```json
{
  "type": "pong",
  "timestamp": 1234567890
}
```

### Event Messages

#### Metrics Update
```json
{
  "type": "metrics",
  "timestamp": "2025-08-09T12:00:00Z",
  "data": {
    "requests_per_second": 1500,
    "active_connections": 1234,
    "cpu_usage": 45.2,
    "memory_usage": 62.8
  }
}
```

#### Log Entry
```json
{
  "type": "log",
  "timestamp": "2025-08-09T12:00:00Z",
  "level": "info",
  "message": "Request processed",
  "fields": {
    "request_id": "req-123",
    "method": "GET",
    "path": "/api/users",
    "status": 200,
    "duration_ms": 15
  }
}
```

#### Cluster Event
```json
{
  "type": "cluster_event",
  "timestamp": "2025-08-09T12:00:00Z",
  "event": "node_joined",
  "data": {
    "node_id": "node-3",
    "address": "192.168.1.12:7946"
  }
}
```

## Authentication

### API Key Authentication
```http
GET /api/v1/status
Authorization: Bearer your-api-key-here
```

### Basic Authentication
```http
GET /api/v1/status
Authorization: Basic base64(username:password)
```

### Session Cookie
```http
GET /api/v1/status
Cookie: session_id=session-123
```

## Error Responses

### Standard Error Format
```json
{
  "error": {
    "code": "RESOURCE_NOT_FOUND",
    "message": "The requested resource was not found",
    "details": {
      "resource": "vhost",
      "id": "vhost-999"
    },
    "timestamp": "2025-08-09T12:00:00Z",
    "request_id": "req-123"
  }
}
```

### Common Error Codes

| HTTP Status | Error Code | Description |
|-------------|------------|-------------|
| 400 | BAD_REQUEST | Invalid request format or parameters |
| 401 | UNAUTHORIZED | Authentication required |
| 403 | FORBIDDEN | Insufficient permissions |
| 404 | NOT_FOUND | Resource not found |
| 409 | CONFLICT | Resource conflict |
| 422 | VALIDATION_ERROR | Request validation failed |
| 429 | RATE_LIMITED | Too many requests |
| 500 | INTERNAL_ERROR | Internal server error |
| 502 | BAD_GATEWAY | Backend unavailable |
| 503 | SERVICE_UNAVAILABLE | Service temporarily unavailable |

### Validation Error Details
```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Request validation failed",
    "details": {
      "fields": [
        {
          "field": "domains",
          "message": "At least one domain is required"
        },
        {
          "field": "backend.target",
          "message": "Invalid URL format"
        }
      ]
    }
  }
}
```

## Rate Limiting

API endpoints are rate limited. Rate limit information is included in response headers:

```http
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 999
X-RateLimit-Reset: 1628520000
```

When rate limited:
```http
HTTP/1.1 429 Too Many Requests
Retry-After: 60

{
  "error": {
    "code": "RATE_LIMITED",
    "message": "API rate limit exceeded",
    "retry_after": 60
  }
}
```

## Pagination

List endpoints support pagination:

```http
GET /api/v1/sessions?page=2&limit=50
```

Response includes pagination metadata:
```json
{
  "data": [...],
  "pagination": {
    "page": 2,
    "limit": 50,
    "total": 500,
    "pages": 10,
    "has_next": true,
    "has_prev": true
  }
}
```

## Filtering and Sorting

### Filtering
```http
GET /api/v1/backends?status=running&type=nodejs
```

### Sorting
```http
GET /api/v1/sessions?sort=created&order=desc
```

### Combined
```http
GET /api/v1/backends?status=running&sort=cpu&order=desc&limit=10
```

## Webhooks

### Register Webhook
```http
POST /api/v1/webhooks
Content-Type: application/json

{
  "url": "https://your-site.com/webhook",
  "events": ["backend.down", "cluster.node_failed"],
  "secret": "webhook-secret"
}
```

### Webhook Payload
```json
{
  "event": "backend.down",
  "timestamp": "2025-08-09T12:00:00Z",
  "data": {
    "backend": "app-1",
    "error": "Health check failed"
  },
  "signature": "sha256=..."
}
```

## API Versioning

The API uses URL versioning:
- `/api/v1/` - Current stable version
- `/api/v2/` - Next version (when available)

Deprecated endpoints include:
```http
X-Deprecated: true
X-Sunset-Date: 2026-01-01
```