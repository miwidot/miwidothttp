# Troubleshooting Guide

## Table of Contents
- [Common Issues](#common-issues)
- [Startup Problems](#startup-problems)
- [Performance Issues](#performance-issues)
- [SSL/TLS Problems](#ssltls-problems)
- [Cluster Issues](#cluster-issues)
- [Memory & Resource Issues](#memory--resource-issues)
- [Network Problems](#network-problems)
- [Application Errors](#application-errors)
- [Debugging Tools](#debugging-tools)
- [Log Analysis](#log-analysis)
- [Emergency Procedures](#emergency-procedures)

## Common Issues

### Server Won't Start

#### Symptom
```bash
$ systemctl status miwidothttp
● miwidothttp.service - miwidothttp HTTP Server
     Loaded: loaded
     Active: failed (Result: exit-code)
```

#### Diagnosis
```bash
# Check logs
journalctl -u miwidothttp -n 50

# Check for port conflicts
sudo lsof -i :8080
sudo netstat -tulpn | grep :8080

# Validate configuration
miwidothttp --validate-config /etc/miwidothttp/config.toml

# Check permissions
ls -la /etc/miwidothttp/
ls -la /var/log/miwidothttp/
```

#### Solutions

**Port Already in Use:**
```bash
# Find process using port
sudo lsof -i :8080
# Kill the process
sudo kill -9 <PID>
# Or change port in config
sed -i 's/http_port = 8080/http_port = 8081/' /etc/miwidothttp/config.toml
```

**Permission Denied:**
```bash
# Fix ownership
sudo chown -R miwidothttp:miwidothttp /etc/miwidothttp
sudo chown -R miwidothttp:miwidothttp /var/log/miwidothttp
sudo chown -R miwidothttp:miwidothttp /var/lib/miwidothttp

# Fix permissions
sudo chmod 755 /usr/local/bin/miwidothttp
sudo chmod 644 /etc/miwidothttp/config.toml
```

**Configuration Error:**
```bash
# Validate syntax
miwidothttp --validate-config /etc/miwidothttp/config.toml

# Common fixes
# Missing quotes around strings
# Invalid TOML syntax
# Incorrect paths
# Invalid regex patterns
```

### High CPU Usage

#### Symptom
```bash
$ top
PID   USER      %CPU  COMMAND
12345 miwidot   95.5  miwidothttp
```

#### Diagnosis
```bash
# Profile CPU usage
perf record -p $(pidof miwidothttp) -g -- sleep 30
perf report

# Check goroutine count (if applicable)
curl http://localhost:8080/debug/pprof/goroutine?debug=1

# Monitor system calls
strace -p $(pidof miwidothttp) -c

# Check thread count
ps -T -p $(pidof miwidothttp)
```

#### Solutions

**Too Many Workers:**
```toml
# Reduce worker count
[server]
workers = 4  # Instead of auto-detect
```

**Inefficient Regex:**
```toml
# Optimize regex patterns
[[rewrites]]
# Bad: Complex backtracking
pattern = "^(.*)(\\.php)?(.*)$"
# Good: Specific pattern
pattern = "^/api/([^/]+)/([^/]+)$"
```

**Disable Unnecessary Features:**
```toml
[monitoring]
enabled = false  # If not needed

[logging]
level = "warn"  # Reduce logging overhead
```

### Memory Leaks

#### Symptom
```bash
$ free -h
              total        used        free
Mem:           15Gi        14Gi       1.0Gi
```

#### Diagnosis
```bash
# Memory profiling
curl http://localhost:8080/debug/pprof/heap > heap.prof
go tool pprof heap.prof

# Check for goroutine leaks
curl http://localhost:8080/debug/pprof/goroutine

# Monitor over time
while true; do
  ps aux | grep miwidothttp | grep -v grep
  sleep 60
done
```

#### Solutions

**Limit Cache Size:**
```toml
[cache]
max_size_mb = 512  # Reduce from default
ttl_seconds = 1800  # Shorter TTL
```

**Session Cleanup:**
```toml
[sessions]
cleanup_interval = 300  # 5 minutes
max_sessions = 10000  # Limit total sessions
```

**Connection Limits:**
```toml
[server]
max_connections = 10000  # Set reasonable limit

[proxy]
max_idle_per_host = 10  # Reduce idle connections
```

## Startup Problems

### Configuration Not Loading

#### Check File Path
```bash
# Verify config exists
ls -la /etc/miwidothttp/config.toml

# Check for typos in systemd service
grep ExecStart /etc/systemd/system/miwidothttp.service
```

#### Environment Variables
```bash
# Check if env vars are set
env | grep MIWIDOTHTTP

# Set in systemd service
[Service]
Environment="MIWIDOTHTTP_CONFIG=/etc/miwidothttp/config.toml"
```

### SSL Certificate Issues

#### Certificate Not Found
```bash
# Check paths
ls -la /etc/miwidothttp/certs/

# Verify in config
grep -A5 "\[ssl\]" /etc/miwidothttp/config.toml

# Test certificate
openssl x509 -in /path/to/cert.pem -text -noout
```

#### Cloudflare API Errors
```bash
# Test API token
curl -X GET "https://api.cloudflare.com/client/v4/user/tokens/verify" \
     -H "Authorization: Bearer YOUR_API_TOKEN"

# Check permissions
# Token needs Zone:SSL:Edit permission
```

## Performance Issues

### Slow Response Times

#### Diagnosis
```bash
# Test with curl
curl -w "@curl-format.txt" -o /dev/null -s http://localhost:8080/

# curl-format.txt:
time_namelookup:  %{time_namelookup}s\n
time_connect:  %{time_connect}s\n
time_appconnect:  %{time_appconnect}s\n
time_pretransfer:  %{time_pretransfer}s\n
time_redirect:  %{time_redirect}s\n
time_starttransfer:  %{time_starttransfer}s\n
time_total:  %{time_total}s\n

# Check backend health
curl http://localhost:8080/api/v1/backends
```

#### Solutions

**Enable Caching:**
```toml
[cache]
enabled = true
backend = "memory"
ttl_seconds = 3600
```

**Optimize Database Queries:**
```toml
[database]
connection_pool_size = 50
query_timeout = 5
```

**Enable Compression:**
```toml
[compression]
enabled = true
level = 6
min_size = 1024
```

### Connection Refused

#### Check Listening Ports
```bash
# Verify server is listening
netstat -tlnp | grep miwidothttp
ss -tlnp | grep 8080

# Check firewall
sudo iptables -L -n | grep 8080
sudo ufw status
```

#### Solutions
```bash
# Open firewall ports
sudo ufw allow 8080/tcp
sudo ufw allow 8443/tcp

# Check binding address
# Change from 127.0.0.1 to 0.0.0.0
[server]
bind_addr = "0.0.0.0:8080"
```

## SSL/TLS Problems

### Certificate Validation Failed

#### Check Certificate Chain
```bash
# Verify certificate
openssl s_client -connect localhost:8443 -showcerts

# Check expiration
openssl x509 -in cert.pem -noout -dates

# Verify chain
openssl verify -CAfile ca.pem cert.pem
```

#### Mixed Content Warnings
```toml
# Force HTTPS
[vhosts.ssl]
force_https = true
hsts_enabled = true

# Set secure headers
[vhosts.headers]
"Content-Security-Policy" = "upgrade-insecure-requests"
```

### Automatic Renewal Not Working

#### Check Renewal Service
```bash
# Check cron/timer
systemctl status miwidothttp-cert-renewal.timer
crontab -l | grep certbot

# Test renewal
miwidothttp --renew-certs --dry-run
```

#### Manual Renewal
```bash
# Cloudflare
miwidothttp --renew-cert --domain example.com

# Let's Encrypt
certbot renew --force-renewal
systemctl reload miwidothttp
```

## Cluster Issues

### Node Not Joining Cluster

#### Check Network Connectivity
```bash
# Test gossip port
nc -zv node2 7946

# Test gRPC port
nc -zv node2 7947

# Ping test
ping -c 4 node2
```

#### Verify Configuration
```toml
[cluster]
# Ensure matching cluster name
cluster_name = "production"

# Correct seed nodes
seed_nodes = ["10.0.1.10:7946"]

# Matching ports
gossip_port = 7946
grpc_port = 7947
```

#### Debug Gossip Protocol
```bash
# Enable debug logging
[logging]
level = "debug"

# Check gossip messages
grep "gossip" /var/log/miwidothttp/server.log

# Monitor cluster events
curl http://localhost:8080/api/v1/cluster/events
```

### Split Brain Scenario

#### Identify Split Brain
```bash
# Check each node's view
for node in node1 node2 node3; do
  echo "=== $node ==="
  ssh $node "curl -s localhost:8080/api/v1/cluster/status | jq .leader"
done
```

#### Resolution
```bash
# Stop minority partition
ssh node3 "systemctl stop miwidothttp"

# Clear state
ssh node3 "rm -rf /var/lib/miwidothttp/cluster/*"

# Rejoin cluster
ssh node3 "systemctl start miwidothttp"
```

### Replication Lag

#### Monitor Lag
```bash
# Check replication status
curl http://localhost:8080/api/v1/cluster/replication

# Monitor metrics
curl http://localhost:8080/metrics | grep replication_lag
```

#### Solutions
```toml
# Increase replication workers
[cluster.replication]
workers = 4
batch_size = 100

# Reduce sync interval
[cluster]
data_sync_interval_ms = 5000  # From 10000
```

## Memory & Resource Issues

### Out of Memory Errors

#### Emergency Response
```bash
# Free memory immediately
sync && echo 3 > /proc/sys/vm/drop_caches

# Restart with memory limit
systemctl stop miwidothttp
ulimit -v 4194304  # 4GB limit
systemctl start miwidothttp
```

#### Long-term Solutions
```toml
# Limit memory usage
[performance.memory]
max_heap_size = 4096  # MB
gc_interval = 60  # seconds

# Reduce caches
[cache]
max_size_mb = 256

[sessions]
max_sessions = 5000
```

### File Descriptor Limits

#### Check Current Limits
```bash
# System limits
cat /proc/sys/fs/file-max

# Process limits
cat /proc/$(pidof miwidothttp)/limits | grep "open files"

# Current usage
ls /proc/$(pidof miwidothttp)/fd | wc -l
```

#### Increase Limits
```bash
# System-wide
echo "fs.file-max = 2000000" >> /etc/sysctl.conf
sysctl -p

# Per-process
# In systemd service
[Service]
LimitNOFILE=1000000

# Or in limits.conf
echo "miwidothttp soft nofile 1000000" >> /etc/security/limits.conf
echo "miwidothttp hard nofile 1000000" >> /etc/security/limits.conf
```

## Network Problems

### DNS Resolution Issues

#### Test DNS
```bash
# Check resolver
cat /etc/resolv.conf

# Test resolution
dig example.com
nslookup example.com
host example.com

# Bypass DNS
echo "192.168.1.10 backend.local" >> /etc/hosts
```

#### Solutions
```toml
# Use IP addresses
[backends."app"]
target = "http://192.168.1.100:3000"  # Instead of hostname

# Configure DNS
[network]
dns_servers = ["8.8.8.8", "8.8.4.4"]
dns_timeout = 5
```

### Connection Timeouts

#### Diagnose
```bash
# Trace route
traceroute backend.example.com

# Test connectivity
curl -v --connect-timeout 5 http://backend:3000

# Check MTU
ping -M do -s 1472 backend
```

#### Adjust Timeouts
```toml
[proxy.timeout]
connect_timeout_seconds = 30  # Increase
read_timeout_seconds = 60
write_timeout_seconds = 60

[server]
request_timeout = 120
```

## Application Errors

### 502 Bad Gateway

#### Check Backend Health
```bash
# Direct backend test
curl http://localhost:3000/health

# Check process
ps aux | grep node
ps aux | grep python
ps aux | grep java

# Restart backend
systemctl restart app-backend
```

#### Fix Configuration
```toml
[backends."app"]
# Ensure correct target
target = "http://localhost:3000"

# Add health check
health_check = "/health"
health_check_interval = 30
```

### 503 Service Unavailable

#### Check Server Status
```bash
# Server health
curl http://localhost:8080/health

# Resource usage
free -h
df -h
top -n 1
```

#### Circuit Breaker Tripped
```toml
# Reset circuit breaker
[circuit_breaker]
failure_threshold = 10  # Increase
recovery_timeout = 30  # Decrease
```

## Debugging Tools

### Enable Debug Mode

```toml
[debug]
enabled = true
pprof_enabled = true
trace_enabled = true

[logging]
level = "trace"
include_caller = true
include_stack_trace = true
```

### Performance Profiling

```bash
# CPU profile
curl http://localhost:8080/debug/pprof/profile?seconds=30 > cpu.prof

# Memory profile
curl http://localhost:8080/debug/pprof/heap > heap.prof

# Goroutine dump
curl http://localhost:8080/debug/pprof/goroutine?debug=2

# Trace
curl http://localhost:8080/debug/pprof/trace?seconds=5 > trace.out
```

### Request Tracing

```bash
# Enable tracing
curl -X POST http://localhost:8080/api/v1/tracing/enable

# Trace specific request
curl -H "X-Trace: true" http://localhost:8080/

# View trace
curl http://localhost:8080/api/v1/traces/latest
```

## Log Analysis

### Parse Error Patterns

```bash
# Count errors by type
grep ERROR /var/log/miwidothttp/server.log | \
  awk '{print $5}' | sort | uniq -c | sort -rn

# Find slow requests
grep "duration" /var/log/miwidothttp/access.log | \
  awk '$NF > 1000' | tail -20

# Track 5xx errors
tail -f /var/log/miwidothttp/access.log | \
  grep "\" 5[0-9][0-9] "
```

### Log Aggregation

```bash
# Send to syslog
[logging.outputs]
type = "syslog"
address = "localhost:514"
facility = "local0"

# JSON for parsing
[logging]
format = "json"

# Parse with jq
tail -f /var/log/miwidothttp/server.log | \
  jq 'select(.level == "error")'
```

## Emergency Procedures

### Server Unresponsive

```bash
#!/bin/bash
# emergency-restart.sh

# Try graceful restart
systemctl restart miwidothttp

# Wait 10 seconds
sleep 10

# Check if responsive
if ! curl -f http://localhost:8080/health; then
  echo "Graceful restart failed, forcing..."
  
  # Kill all processes
  pkill -9 miwidothttp
  
  # Clear locks
  rm -f /var/lib/miwidothttp/*.lock
  
  # Start fresh
  systemctl start miwidothttp
fi
```

### Rollback Deployment

```bash
#!/bin/bash
# rollback.sh

# Stop current version
systemctl stop miwidothttp

# Restore previous binary
cp /backup/miwidothttp.prev /usr/local/bin/miwidothttp

# Restore configuration
cp /backup/config.toml.prev /etc/miwidothttp/config.toml

# Start service
systemctl start miwidothttp

# Verify
curl http://localhost:8080/health || {
  echo "Rollback failed!"
  exit 1
}
```

### Data Corruption

```bash
# Stop service
systemctl stop miwidothttp

# Backup corrupted data
mv /var/lib/miwidothttp /var/lib/miwidothttp.corrupted

# Restore from backup
tar -xzf /backup/data-latest.tar.gz -C /var/lib/

# Clear caches
redis-cli FLUSHALL

# Start service
systemctl start miwidothttp

# Verify data integrity
miwidothttp --verify-data
```

## Diagnostic Commands

### Quick Health Check
```bash
#!/bin/bash
# health-check.sh

echo "=== System Resources ==="
free -h
df -h
uptime

echo "=== Process Status ==="
systemctl status miwidothttp --no-pager

echo "=== Port Listening ==="
netstat -tlnp | grep miwidothttp

echo "=== Recent Errors ==="
journalctl -u miwidothttp -p err -n 10 --no-pager

echo "=== HTTP Health ==="
curl -s http://localhost:8080/health | jq .

echo "=== Cluster Status ==="
curl -s http://localhost:8080/api/v1/cluster/status | jq .
```

### Performance Check
```bash
#!/bin/bash
# perf-check.sh

echo "=== Response Time ==="
for i in {1..10}; do
  curl -w "%{time_total}\n" -o /dev/null -s http://localhost:8080/
done | awk '{sum+=$1} END {print "Average:", sum/NR "s"}'

echo "=== Throughput Test ==="
ab -n 1000 -c 10 http://localhost:8080/

echo "=== Resource Usage ==="
ps aux | grep miwidothttp | grep -v grep

echo "=== Connection Count ==="
netstat -an | grep :8080 | wc -l
```

## Common Error Messages

| Error | Cause | Solution |
|-------|-------|----------|
| `bind: address already in use` | Port conflict | Change port or kill conflicting process |
| `too many open files` | FD limit reached | Increase ulimit |
| `cannot allocate memory` | OOM | Add RAM or reduce memory usage |
| `connection refused` | Service not running | Start service |
| `permission denied` | Wrong permissions | Fix ownership/permissions |
| `certificate has expired` | SSL cert expired | Renew certificate |
| `no route to host` | Network issue | Check network configuration |
| `context deadline exceeded` | Timeout | Increase timeout values |
| `transport endpoint is not connected` | Connection dropped | Check network stability |
| `cluster not ready` | Cluster forming | Wait for quorum |