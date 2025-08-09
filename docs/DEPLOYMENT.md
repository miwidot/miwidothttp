# Deployment Guide

## Table of Contents
- [Deployment Strategies](#deployment-strategies)
- [Single Server Deployment](#single-server-deployment)
- [Multi-Server Cluster](#multi-server-cluster)
- [Cloud Deployments](#cloud-deployments)
- [Container Orchestration](#container-orchestration)
- [Load Balancing](#load-balancing)
- [SSL/TLS Setup](#ssltls-setup)
- [Monitoring Setup](#monitoring-setup)
- [Backup & Recovery](#backup--recovery)
- [Security Hardening](#security-hardening)
- [Performance Optimization](#performance-optimization)

## Deployment Strategies

### Deployment Models

| Model | Use Case | Pros | Cons |
|-------|----------|------|------|
| **Single Server** | Development, small sites | Simple, low cost | No redundancy |
| **Active-Passive** | Medium traffic, HA required | Simple failover | Resource waste |
| **Active-Active** | High traffic, load distribution | Full utilization | Complex setup |
| **Geographic** | Global audience | Low latency | Complex sync |
| **Edge** | CDN, static content | Fast delivery | Limited compute |

## Single Server Deployment

### 1. Basic Setup

```bash
# Install miwidothttp
curl -L https://github.com/miwidot/miwidothttp/releases/latest/download/miwidothttp-linux-amd64.tar.gz | tar -xz
sudo mv miwidothttp /usr/local/bin/

# Create user and directories
sudo useradd -r -s /bin/false miwidothttp
sudo mkdir -p /etc/miwidothttp /var/log/miwidothttp /var/lib/miwidothttp
sudo chown -R miwidothttp:miwidothttp /etc/miwidothttp /var/log/miwidothttp /var/lib/miwidothttp

# Create configuration
sudo cat > /etc/miwidothttp/config.toml << 'EOF'
[server]
http_port = 80
https_port = 443
workers = 0  # Auto-detect

[ssl]
provider = "cloudflare"
auto_renew = true

[ssl.cloudflare]
api_token = "${CLOUDFLARE_API_TOKEN}"
zone_id = "${CLOUDFLARE_ZONE_ID}"

[[vhosts]]
domains = ["example.com", "www.example.com"]
root = "/var/www/html"
EOF

# Setup systemd service
sudo cat > /etc/systemd/system/miwidothttp.service << 'EOF'
[Unit]
Description=miwidothttp High-Performance HTTP Server
After=network.target

[Service]
Type=simple
User=miwidothttp
ExecStart=/usr/local/bin/miwidothttp --config /etc/miwidothttp/config.toml
Restart=always
RestartSec=10
StandardOutput=append:/var/log/miwidothttp/server.log
StandardError=append:/var/log/miwidothttp/error.log

[Install]
WantedBy=multi-user.target
EOF

# Start service
sudo systemctl daemon-reload
sudo systemctl enable miwidothttp
sudo systemctl start miwidothttp
```

### 2. Firewall Configuration

```bash
# UFW (Ubuntu/Debian)
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw allow 7946/tcp  # Cluster gossip
sudo ufw allow 7947/tcp  # Cluster gRPC
sudo ufw reload

# firewalld (RHEL/CentOS)
sudo firewall-cmd --permanent --add-service=http
sudo firewall-cmd --permanent --add-service=https
sudo firewall-cmd --permanent --add-port=7946/tcp
sudo firewall-cmd --permanent --add-port=7947/tcp
sudo firewall-cmd --reload

# iptables
sudo iptables -A INPUT -p tcp --dport 80 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 443 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 7946 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 7947 -j ACCEPT
sudo iptables-save > /etc/iptables/rules.v4
```

## Multi-Server Cluster

### 1. Three-Node Cluster Setup

#### Node 1 (Leader)
```bash
# /etc/miwidothttp/config.toml
[cluster]
enabled = true
node_id = "node-1"
cluster_name = "production"
bind_addr = "0.0.0.0:7946"
advertise_addr = "10.0.1.10:7946"
grpc_port = 7947

# First node doesn't need seed nodes
seed_nodes = []

[cluster.consensus]
enable_leader_election = true
replication_factor = 3
quorum_size = 2
```

#### Node 2 & 3 (Followers)
```bash
# /etc/miwidothttp/config.toml
[cluster]
enabled = true
node_id = "node-2"  # or "node-3"
cluster_name = "production"
bind_addr = "0.0.0.0:7946"
advertise_addr = "10.0.1.11:7946"  # or .12
grpc_port = 7947

# Point to first node
seed_nodes = ["10.0.1.10:7946"]

[cluster.consensus]
enable_leader_election = true
replication_factor = 3
quorum_size = 2
```

### 2. Cluster Initialization

```bash
# Start nodes in order
# Node 1
ssh node1 'sudo systemctl start miwidothttp'

# Wait for node 1 to be ready
sleep 10

# Node 2
ssh node2 'sudo systemctl start miwidothttp'

# Node 3
ssh node3 'sudo systemctl start miwidothttp'

# Verify cluster status
curl http://node1:8080/api/v1/cluster/status
```

### 3. Shared Configuration via etcd

```bash
# Install etcd cluster
sudo apt-get install etcd

# Configure etcd cluster
cat > /etc/etcd/etcd.conf.yml << EOF
name: 'etcd-1'
data-dir: '/var/lib/etcd'
listen-client-urls: 'http://0.0.0.0:2379'
advertise-client-urls: 'http://10.0.1.10:2379'
listen-peer-urls: 'http://0.0.0.0:2380'
initial-advertise-peer-urls: 'http://10.0.1.10:2380'
initial-cluster: 'etcd-1=http://10.0.1.10:2380,etcd-2=http://10.0.1.11:2380,etcd-3=http://10.0.1.12:2380'
initial-cluster-token: 'etcd-cluster-token'
initial-cluster-state: 'new'
EOF

# Configure miwidothttp to use etcd
[cluster.discovery]
method = "etcd"
etcd_endpoints = [
  "http://10.0.1.10:2379",
  "http://10.0.1.11:2379",
  "http://10.0.1.12:2379"
]
```

## Cloud Deployments

### AWS Deployment

#### 1. EC2 with Auto Scaling

```yaml
# cloudformation.yaml
AWSTemplateFormatVersion: '2010-09-09'
Description: 'miwidothttp Auto Scaling Group'

Resources:
  LaunchTemplate:
    Type: AWS::EC2::LaunchTemplate
    Properties:
      LaunchTemplateName: miwidothttp-template
      LaunchTemplateData:
        ImageId: ami-0c55b159cbfafe1f0  # Ubuntu 22.04
        InstanceType: t3.medium
        IamInstanceProfile:
          Arn: !GetAtt InstanceProfile.Arn
        SecurityGroupIds:
          - !Ref SecurityGroup
        UserData:
          Fn::Base64: !Sub |
            #!/bin/bash
            curl -L https://github.com/miwidot/miwidothttp/releases/latest/download/miwidothttp-linux-amd64.tar.gz | tar -xz
            mv miwidothttp /usr/local/bin/
            
            cat > /etc/miwidothttp/config.toml << EOF
            [server]
            http_port = 80
            https_port = 443
            
            [cluster]
            enabled = true
            node_id = "$(ec2-metadata --instance-id)"
            
            [cluster.discovery]
            method = "aws"
            aws_region = "${AWS::Region}"
            aws_tag_key = "Environment"
            aws_tag_value = "production"
            EOF
            
            systemctl start miwidothttp

  AutoScalingGroup:
    Type: AWS::AutoScaling::AutoScalingGroup
    Properties:
      LaunchTemplate:
        LaunchTemplateId: !Ref LaunchTemplate
        Version: !GetAtt LaunchTemplate.LatestVersionNumber
      MinSize: 3
      MaxSize: 10
      DesiredCapacity: 3
      TargetGroupARNs:
        - !Ref TargetGroup
      HealthCheckType: ELB
      HealthCheckGracePeriod: 300

  ApplicationLoadBalancer:
    Type: AWS::ElasticLoadBalancingV2::LoadBalancer
    Properties:
      Type: application
      Subnets:
        - !Ref SubnetA
        - !Ref SubnetB
      SecurityGroups:
        - !Ref SecurityGroup

  TargetGroup:
    Type: AWS::ElasticLoadBalancingV2::TargetGroup
    Properties:
      Port: 80
      Protocol: HTTP
      VpcId: !Ref VPC
      HealthCheckPath: /health
      HealthCheckIntervalSeconds: 30
```

#### 2. ECS Fargate Deployment

```json
{
  "family": "miwidothttp",
  "networkMode": "awsvpc",
  "requiresCompatibilities": ["FARGATE"],
  "cpu": "1024",
  "memory": "2048",
  "containerDefinitions": [
    {
      "name": "miwidothttp",
      "image": "miwidot/miwidothttp:latest",
      "portMappings": [
        {
          "containerPort": 8080,
          "protocol": "tcp"
        },
        {
          "containerPort": 8443,
          "protocol": "tcp"
        }
      ],
      "environment": [
        {
          "name": "CLUSTER_ENABLED",
          "value": "true"
        },
        {
          "name": "REDIS_URL",
          "value": "redis://redis.internal:6379"
        }
      ],
      "logConfiguration": {
        "logDriver": "awslogs",
        "options": {
          "awslogs-group": "/ecs/miwidothttp",
          "awslogs-region": "us-east-1",
          "awslogs-stream-prefix": "ecs"
        }
      }
    }
  ]
}
```

### Google Cloud Platform

#### 1. GKE Deployment

```yaml
# gke-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: miwidothttp
  namespace: production
spec:
  replicas: 3
  selector:
    matchLabels:
      app: miwidothttp
  template:
    metadata:
      labels:
        app: miwidothttp
    spec:
      containers:
      - name: miwidothttp
        image: gcr.io/project-id/miwidothttp:latest
        ports:
        - containerPort: 8080
          name: http
        - containerPort: 8443
          name: https
        - containerPort: 7946
          name: gossip
        resources:
          requests:
            memory: "1Gi"
            cpu: "500m"
          limits:
            memory: "2Gi"
            cpu: "1000m"
        env:
        - name: CLUSTER_ENABLED
          value: "true"
        - name: NODE_ID
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5
---
apiVersion: v1
kind: Service
metadata:
  name: miwidothttp
  namespace: production
spec:
  type: LoadBalancer
  ports:
  - port: 80
    targetPort: 8080
    name: http
  - port: 443
    targetPort: 8443
    name: https
  selector:
    app: miwidothttp
---
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: miwidothttp-hpa
  namespace: production
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: miwidothttp
  minReplicas: 3
  maxReplicas: 20
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
  - type: Resource
    resource:
      name: memory
      target:
        type: Utilization
        averageUtilization: 80
```

### Azure Deployment

#### Container Instances
```bash
# Deploy to Azure Container Instances
az container create \
  --resource-group myResourceGroup \
  --name miwidothttp \
  --image miwidot/miwidothttp:latest \
  --cpu 2 \
  --memory 4 \
  --ports 80 443 \
  --dns-name-label miwidothttp \
  --environment-variables \
    CLUSTER_ENABLED=true \
    AZURE_STORAGE_CONNECTION_STRING=$AZURE_STORAGE
```

## Container Orchestration

### Docker Swarm

```bash
# Initialize swarm
docker swarm init --advertise-addr 10.0.1.10

# Join workers
docker swarm join-token worker

# Deploy stack
cat > docker-stack.yml << EOF
version: '3.8'

services:
  miwidothttp:
    image: miwidot/miwidothttp:latest
    deploy:
      replicas: 3
      update_config:
        parallelism: 1
        delay: 10s
      restart_policy:
        condition: on-failure
    ports:
      - "80:8080"
      - "443:8443"
    networks:
      - miwidothttp-net
    environment:
      CLUSTER_ENABLED: "true"
      REDIS_URL: "redis://redis:6379"

  redis:
    image: redis:7-alpine
    deploy:
      replicas: 1
    networks:
      - miwidothttp-net

networks:
  miwidothttp-net:
    driver: overlay
EOF

docker stack deploy -c docker-stack.yml miwidothttp
```

### Kubernetes with Helm

```bash
# Create Helm chart
helm create miwidothttp

# values.yaml
replicaCount: 3

image:
  repository: miwidot/miwidothttp
  pullPolicy: IfNotPresent
  tag: "latest"

service:
  type: LoadBalancer
  port: 80
  targetPort: 8080

ingress:
  enabled: true
  className: "nginx"
  annotations:
    cert-manager.io/cluster-issuer: "letsencrypt-prod"
  hosts:
    - host: example.com
      paths:
        - path: /
          pathType: ImplementationSpecific
  tls:
    - secretName: example-com-tls
      hosts:
        - example.com

resources:
  limits:
    cpu: 1000m
    memory: 2Gi
  requests:
    cpu: 500m
    memory: 1Gi

autoscaling:
  enabled: true
  minReplicas: 3
  maxReplicas: 20
  targetCPUUtilizationPercentage: 70

cluster:
  enabled: true
  replicationFactor: 3

# Deploy
helm install miwidothttp ./miwidothttp -n production
```

## Load Balancing

### HAProxy Configuration

```
# /etc/haproxy/haproxy.cfg
global
    maxconn 100000
    log /dev/log local0
    chroot /var/lib/haproxy
    stats socket /run/haproxy/admin.sock mode 660 level admin
    stats timeout 30s
    user haproxy
    group haproxy
    daemon

defaults
    mode http
    log global
    option httplog
    option dontlognull
    option http-server-close
    option forwardfor except 127.0.0.0/8
    option redispatch
    retries 3
    timeout http-request 10s
    timeout queue 1m
    timeout connect 10s
    timeout client 1m
    timeout server 1m
    timeout http-keep-alive 10s
    timeout check 10s
    maxconn 100000

frontend http_front
    bind *:80
    bind *:443 ssl crt /etc/ssl/certs/haproxy.pem
    redirect scheme https if !{ ssl_fc }
    
    # ACLs
    acl is_websocket hdr(Upgrade) -i WebSocket
    acl is_api path_beg /api
    
    # Use backends
    use_backend websocket_back if is_websocket
    use_backend api_back if is_api
    default_backend web_back

backend web_back
    balance roundrobin
    option httpchk GET /health
    server node1 10.0.1.10:8080 check
    server node2 10.0.1.11:8080 check
    server node3 10.0.1.12:8080 check

backend api_back
    balance leastconn
    option httpchk GET /health
    server node1 10.0.1.10:8080 check weight 100
    server node2 10.0.1.11:8080 check weight 100
    server node3 10.0.1.12:8080 check weight 150

backend websocket_back
    balance source
    option http-server-close
    option forceclose
    server node1 10.0.1.10:8080 check
    server node2 10.0.1.11:8080 check
    server node3 10.0.1.12:8080 check

stats enable
stats uri /haproxy?stats
stats refresh 30s
```

### NGINX Load Balancer

```nginx
# /etc/nginx/nginx.conf
upstream miwidothttp {
    least_conn;
    server 10.0.1.10:8080 weight=100 max_fails=3 fail_timeout=30s;
    server 10.0.1.11:8080 weight=100 max_fails=3 fail_timeout=30s;
    server 10.0.1.12:8080 weight=150 max_fails=3 fail_timeout=30s;
    keepalive 32;
}

upstream miwidothttp_ws {
    ip_hash;
    server 10.0.1.10:8080;
    server 10.0.1.11:8080;
    server 10.0.1.12:8080;
}

server {
    listen 80;
    listen 443 ssl http2;
    server_name example.com;

    ssl_certificate /etc/nginx/ssl/cert.pem;
    ssl_certificate_key /etc/nginx/ssl/key.pem;

    location / {
        proxy_pass http://miwidothttp;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;
    }

    location /ws {
        proxy_pass http://miwidothttp_ws;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }
}
```

## SSL/TLS Setup

### Cloudflare Origin CA

```bash
# Configure Cloudflare API credentials
export CLOUDFLARE_API_TOKEN="your-api-token"
export CLOUDFLARE_ZONE_ID="your-zone-id"

# Configuration
cat > /etc/miwidothttp/ssl.toml << EOF
[ssl]
provider = "cloudflare"
auto_renew = true
renewal_days_before_expiry = 30

[ssl.cloudflare]
api_token = "${CLOUDFLARE_API_TOKEN}"
zone_id = "${CLOUDFLARE_ZONE_ID}"
cert_validity_days = 90

[ssl.domains."example.com"]
auto_generate = true
force_https = true
hsts_enabled = true
hsts_max_age = 31536000
EOF
```

### Let's Encrypt with Certbot

```bash
# Install certbot
sudo apt-get install certbot

# Generate certificate
sudo certbot certonly --standalone -d example.com -d www.example.com

# Configure miwidothttp
[ssl.domains."example.com"]
cert_file = "/etc/letsencrypt/live/example.com/fullchain.pem"
key_file = "/etc/letsencrypt/live/example.com/privkey.pem"

# Auto-renewal cron
echo "0 2 * * * root certbot renew --quiet --post-hook 'systemctl reload miwidothttp'" > /etc/cron.d/certbot
```

## Monitoring Setup

### Prometheus Configuration

```yaml
# prometheus.yml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'miwidothttp'
    static_configs:
      - targets:
        - '10.0.1.10:8080'
        - '10.0.1.11:8080'
        - '10.0.1.12:8080'
    metrics_path: '/metrics'

  - job_name: 'node_exporter'
    static_configs:
      - targets:
        - '10.0.1.10:9100'
        - '10.0.1.11:9100'
        - '10.0.1.12:9100'

alerting:
  alertmanagers:
    - static_configs:
        - targets:
          - 'alertmanager:9093'

rule_files:
  - 'alerts.yml'
```

### Grafana Dashboard

```json
{
  "dashboard": {
    "title": "miwidothttp Monitoring",
    "panels": [
      {
        "title": "Request Rate",
        "targets": [
          {
            "expr": "rate(http_requests_total[5m])"
          }
        ]
      },
      {
        "title": "Response Time",
        "targets": [
          {
            "expr": "histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m]))"
          }
        ]
      },
      {
        "title": "Active Connections",
        "targets": [
          {
            "expr": "http_connections_active"
          }
        ]
      },
      {
        "title": "Error Rate",
        "targets": [
          {
            "expr": "rate(http_requests_total{status=~\"5..\"}[5m])"
          }
        ]
      }
    ]
  }
}
```

## Backup & Recovery

### Backup Strategy

```bash
#!/bin/bash
# backup.sh

BACKUP_DIR="/backup/miwidothttp"
DATE=$(date +%Y%m%d-%H%M%S)

# Backup configuration
tar -czf ${BACKUP_DIR}/config-${DATE}.tar.gz /etc/miwidothttp/

# Backup SSL certificates
tar -czf ${BACKUP_DIR}/ssl-${DATE}.tar.gz /etc/miwidothttp/certs/

# Backup session data
redis-cli --rdb ${BACKUP_DIR}/redis-${DATE}.rdb

# Backup logs
tar -czf ${BACKUP_DIR}/logs-${DATE}.tar.gz /var/log/miwidothttp/

# Upload to S3
aws s3 cp ${BACKUP_DIR}/ s3://backup-bucket/miwidothttp/${DATE}/ --recursive

# Clean old backups (keep 30 days)
find ${BACKUP_DIR} -type f -mtime +30 -delete
```

### Disaster Recovery

```bash
#!/bin/bash
# restore.sh

RESTORE_DATE=$1
BACKUP_SOURCE="s3://backup-bucket/miwidothttp/${RESTORE_DATE}"

# Download backups
aws s3 sync ${BACKUP_SOURCE}/ /tmp/restore/

# Stop service
systemctl stop miwidothttp

# Restore configuration
tar -xzf /tmp/restore/config-${RESTORE_DATE}.tar.gz -C /

# Restore SSL certificates
tar -xzf /tmp/restore/ssl-${RESTORE_DATE}.tar.gz -C /

# Restore Redis data
redis-cli --rdb /tmp/restore/redis-${RESTORE_DATE}.rdb

# Start service
systemctl start miwidothttp

# Verify
curl http://localhost:8080/health
```

## Security Hardening

### System Security

```bash
# Kernel parameters
cat >> /etc/sysctl.conf << EOF
# Network security
net.ipv4.tcp_syncookies = 1
net.ipv4.conf.all.rp_filter = 1
net.ipv4.conf.default.rp_filter = 1
net.ipv4.conf.all.accept_source_route = 0
net.ipv4.conf.default.accept_source_route = 0
net.ipv4.icmp_echo_ignore_broadcasts = 1
net.ipv4.icmp_ignore_bogus_error_responses = 1
net.ipv4.conf.all.log_martians = 1

# DDoS protection
net.ipv4.tcp_max_syn_backlog = 4096
net.ipv4.tcp_synack_retries = 2
net.ipv4.tcp_syn_retries = 3

# File descriptors
fs.file-max = 2000000
fs.nr_open = 2000000
EOF

sysctl -p

# AppArmor profile
cat > /etc/apparmor.d/usr.local.bin.miwidothttp << EOF
#include <tunables/global>

/usr/local/bin/miwidothttp {
  #include <abstractions/base>
  #include <abstractions/nameservice>
  
  capability net_bind_service,
  capability setuid,
  capability setgid,
  
  /usr/local/bin/miwidothttp mr,
  /etc/miwidothttp/** r,
  /var/log/miwidothttp/** w,
  /var/lib/miwidothttp/** rw,
  /proc/sys/net/core/somaxconn r,
  
  network tcp,
  network udp,
}
EOF

apparmor_parser -r /etc/apparmor.d/usr.local.bin.miwidothttp
```

### Application Security

```toml
# /etc/miwidothttp/security.toml
[security]
# CORS
[security.cors]
enabled = true
allowed_origins = ["https://example.com"]
allowed_methods = ["GET", "POST", "PUT", "DELETE"]
allow_credentials = true

# Headers
[security.headers]
"X-Frame-Options" = "DENY"
"X-Content-Type-Options" = "nosniff"
"X-XSS-Protection" = "1; mode=block"
"Referrer-Policy" = "strict-origin-when-cross-origin"
"Content-Security-Policy" = "default-src 'self'"
"Strict-Transport-Security" = "max-age=31536000; includeSubDomains; preload"

# Rate limiting
[security.rate_limiting]
enabled = true
requests_per_second = 100
requests_per_minute = 5000
burst_size = 200

# IP filtering
[security.ip_filter]
enabled = true
whitelist = ["10.0.0.0/8", "192.168.0.0/16"]
blacklist = []

# DDoS protection
[security.ddos]
enabled = true
threshold = 10000
block_duration = 300
```

## Performance Optimization

### System Tuning

```bash
# CPU governor
echo performance | tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor

# Network optimization
ethtool -K eth0 gro on
ethtool -K eth0 gso on
ethtool -K eth0 tso on

# Transparent Huge Pages
echo never > /sys/kernel/mm/transparent_hugepage/enabled
echo never > /sys/kernel/mm/transparent_hugepage/defrag

# NUMA optimization
numactl --cpunodebind=0 --membind=0 /usr/local/bin/miwidothttp
```

### Application Tuning

```toml
# /etc/miwidothttp/performance.toml
[performance]
worker_threads = 16  # 2x CPU cores
async_threads = 64

[performance.buffers]
read_buffer_size = 65536
write_buffer_size = 65536
max_header_buffer_size = 16384

[performance.http2]
initial_stream_window_size = 2097152
initial_connection_window_size = 5242880
max_concurrent_streams = 1000

[performance.cache]
enabled = true
max_size_mb = 4096
ttl_seconds = 3600

[performance.connection_pool]
max_idle_per_host = 100
idle_timeout_seconds = 60
max_lifetime_seconds = 300
```

## Deployment Checklist

### Pre-Deployment
- [ ] System requirements verified
- [ ] Network connectivity tested
- [ ] DNS records configured
- [ ] SSL certificates ready
- [ ] Firewall rules configured
- [ ] Monitoring setup complete
- [ ] Backup strategy defined

### Deployment
- [ ] Application installed
- [ ] Configuration validated
- [ ] Service started
- [ ] Health checks passing
- [ ] SSL/TLS working
- [ ] Cluster joined (if applicable)
- [ ] Load balancer configured

### Post-Deployment
- [ ] Performance baseline established
- [ ] Monitoring dashboards working
- [ ] Alerts configured
- [ ] Documentation updated
- [ ] Team trained
- [ ] Runbooks created
- [ ] Disaster recovery tested

## Rollback Procedures

```bash
#!/bin/bash
# rollback.sh

PREVIOUS_VERSION=$1

# Stop current version
systemctl stop miwidothttp

# Backup current state
tar -czf /backup/rollback-$(date +%Y%m%d-%H%M%S).tar.gz /etc/miwidothttp/

# Restore previous version
cp /backup/versions/miwidothttp-${PREVIOUS_VERSION} /usr/local/bin/miwidothttp
tar -xzf /backup/config-${PREVIOUS_VERSION}.tar.gz -C /

# Start service
systemctl start miwidothttp

# Verify
sleep 5
if ! curl -f http://localhost:8080/health; then
    echo "Rollback failed, restoring from backup"
    tar -xzf /backup/rollback-*.tar.gz -C /
    systemctl restart miwidothttp
fi
```