# Installation Guide

## Table of Contents
- [System Requirements](#system-requirements)
- [Installation Methods](#installation-methods)
- [Building from Source](#building-from-source)
- [Binary Installation](#binary-installation)
- [Package Managers](#package-managers)
- [Docker Installation](#docker-installation)
- [Post-Installation](#post-installation)
- [Verification](#verification)
- [Uninstallation](#uninstallation)

## System Requirements

### Minimum Requirements
- **CPU**: 2 cores (x86_64 or ARM64)
- **RAM**: 2GB
- **Disk**: 500MB for binary + logs/cache
- **OS**: Linux kernel 4.9+, macOS 10.15+, Windows 10+

### Recommended Requirements
- **CPU**: 8+ cores
- **RAM**: 8GB+
- **Disk**: 10GB SSD
- **Network**: 1Gbps+

### Operating Systems

#### Linux
- Ubuntu 20.04+ LTS
- Debian 11+
- RHEL/CentOS/Rocky 8+
- Fedora 35+
- Arch Linux (latest)
- Alpine Linux 3.14+

#### macOS
- macOS 10.15 Catalina+
- Apple Silicon (M1/M2) supported

#### Windows
- Windows 10 version 1909+
- Windows Server 2019+
- WSL2 recommended

### Dependencies

#### Runtime Dependencies
```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install -y \
    ca-certificates \
    libssl3 \
    libgcc1 \
    libc6

# RHEL/CentOS/Rocky
sudo yum install -y \
    ca-certificates \
    openssl-libs \
    glibc

# macOS (using Homebrew)
brew install openssl@3

# Alpine
apk add --no-cache \
    ca-certificates \
    libgcc \
    libssl3
```

#### Optional Dependencies (for specific features)
```bash
# PHP-FPM support
sudo apt-get install -y php8.3-fpm

# Redis for sessions/cache
sudo apt-get install -y redis-server

# Node.js for process management
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt-get install -y nodejs

# Python for process management
sudo apt-get install -y python3 python3-pip

# Java/Tomcat support
sudo apt-get install -y openjdk-17-jdk tomcat10
```

## Installation Methods

### Method 1: Pre-built Binaries (Recommended)

#### Linux (x86_64)
```bash
# Download latest release
curl -L https://github.com/miwidothttp/releases/latest/download/miwidothttp-linux-amd64.tar.gz -o miwidothttp.tar.gz

# Extract
tar -xzf miwidothttp.tar.gz

# Install to system
sudo mv miwidothttp /usr/local/bin/
sudo chmod +x /usr/local/bin/miwidothttp

# Create directories
sudo mkdir -p /etc/miwidothttp
sudo mkdir -p /var/log/miwidothttp
sudo mkdir -p /var/cache/miwidothttp
sudo mkdir -p /var/lib/miwidothttp
```

#### Linux (ARM64)
```bash
# Download ARM64 build
curl -L https://github.com/miwidothttp/releases/latest/download/miwidothttp-linux-arm64.tar.gz -o miwidothttp.tar.gz

# Follow same steps as x86_64
```

#### macOS (Intel)
```bash
# Download macOS build
curl -L https://github.com/miwidothttp/releases/latest/download/miwidothttp-darwin-amd64.tar.gz -o miwidothttp.tar.gz

# Extract and install
tar -xzf miwidothttp.tar.gz
sudo mv miwidothttp /usr/local/bin/
sudo chmod +x /usr/local/bin/miwidothttp
```

#### macOS (Apple Silicon)
```bash
# Download Apple Silicon build
curl -L https://github.com/miwidothttp/releases/latest/download/miwidothttp-darwin-arm64.tar.gz -o miwidothttp.tar.gz

# Extract and install
tar -xzf miwidothttp.tar.gz
sudo mv miwidothttp /usr/local/bin/
sudo chmod +x /usr/local/bin/miwidothttp
```

#### Windows
```powershell
# PowerShell as Administrator
# Download Windows build
Invoke-WebRequest -Uri "https://github.com/miwidothttp/releases/latest/download/miwidothttp-windows-amd64.zip" -OutFile "miwidothttp.zip"

# Extract
Expand-Archive -Path "miwidothttp.zip" -DestinationPath "C:\Program Files\miwidothttp"

# Add to PATH
[Environment]::SetEnvironmentVariable("Path", $env:Path + ";C:\Program Files\miwidothttp", [EnvironmentVariableTarget]::Machine)
```

### Method 2: Building from Source

#### Prerequisites
```bash
# Install Rust (all platforms)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Verify installation
rustc --version  # Should be 1.82+
cargo --version
```

#### Build Steps
```bash
# Clone repository
git clone https://github.com/miwidothttp/miwidothttp.git
cd miwidothttp

# Build release version
cargo build --release

# Binary will be at: target/release/miwidothttp
ls -la target/release/miwidothttp

# Install to system
sudo cp target/release/miwidothttp /usr/local/bin/
sudo chmod +x /usr/local/bin/miwidothttp
```

#### Build with Features
```bash
# Build with all features
cargo build --release --all-features

# Build with specific features
cargo build --release --features "cluster,redis,prometheus"

# Build optimized for your CPU
RUSTFLAGS="-C target-cpu=native" cargo build --release

# Build static binary (Linux)
RUSTFLAGS='-C target-feature=+crt-static' cargo build --release --target x86_64-unknown-linux-musl
```

### Method 3: Package Managers

#### Homebrew (macOS/Linux)
```bash
# Add tap
brew tap miwidothttp/tap

# Install
brew install miwidothttp

# Start as service
brew services start miwidothttp
```

#### APT (Ubuntu/Debian)
```bash
# Add repository
curl -fsSL https://packages.miwidothttp.io/gpg | sudo gpg --dearmor -o /usr/share/keyrings/miwidothttp-archive-keyring.gpg
echo "deb [signed-by=/usr/share/keyrings/miwidothttp-archive-keyring.gpg] https://packages.miwidothttp.io/apt stable main" | sudo tee /etc/apt/sources.list.d/miwidothttp.list

# Install
sudo apt-get update
sudo apt-get install miwidothttp
```

#### YUM/DNF (RHEL/CentOS/Fedora)
```bash
# Add repository
sudo cat > /etc/yum.repos.d/miwidothttp.repo << EOF
[miwidothttp]
name=miwidothttp
baseurl=https://packages.miwidothttp.io/rpm
enabled=1
gpgcheck=1
gpgkey=https://packages.miwidothttp.io/gpg
EOF

# Install
sudo yum install miwidothttp
# or
sudo dnf install miwidothttp
```

#### AUR (Arch Linux)
```bash
# Using yay
yay -S miwidothttp

# Using paru
paru -S miwidothttp

# Manual build
git clone https://aur.archlinux.org/miwidothttp.git
cd miwidothttp
makepkg -si
```

#### Snap
```bash
# Install from Snap Store
sudo snap install miwidothttp

# Classic confinement for full system access
sudo snap install miwidothttp --classic
```

#### Cargo (Rust)
```bash
# Install from crates.io
cargo install miwidothttp

# Install specific version
cargo install miwidothttp --version 1.0.0

# Install with features
cargo install miwidothttp --features "cluster,redis"
```

### Method 4: Docker Installation

#### Pull Official Image
```bash
# Latest stable
docker pull miwidothttp/miwidothttp:latest

# Specific version
docker pull miwidothttp/miwidothttp:1.0.0

# Alpine-based (smaller)
docker pull miwidothttp/miwidothttp:alpine
```

#### Run Container
```bash
# Basic run
docker run -d \
  --name miwidothttp \
  -p 8080:8080 \
  -p 8443:8443 \
  -v /path/to/config:/etc/miwidothttp \
  -v /path/to/logs:/var/log/miwidothttp \
  miwidothttp/miwidothttp:latest

# With environment variables
docker run -d \
  --name miwidothttp \
  -p 8080:8080 \
  -p 8443:8443 \
  -e MIWIDOTHTTP_SERVER_HTTP_PORT=8080 \
  -e MIWIDOTHTTP_SSL_CLOUDFLARE_API_TOKEN=your-token \
  miwidothttp/miwidothttp:latest
```

#### Docker Compose
```yaml
version: '3.8'
services:
  miwidothttp:
    image: miwidothttp/miwidothttp:latest
    container_name: miwidothttp
    ports:
      - "80:8080"
      - "443:8443"
    volumes:
      - ./config:/etc/miwidothttp
      - ./logs:/var/log/miwidothttp
      - ./data:/var/lib/miwidothttp
    environment:
      - MIWIDOTHTTP_CLUSTER_ENABLED=true
      - MIWIDOTHTTP_REDIS_URL=redis://redis:6379
    depends_on:
      - redis
    restart: unless-stopped

  redis:
    image: redis:7-alpine
    container_name: miwidothttp-redis
    volumes:
      - redis-data:/data
    restart: unless-stopped

volumes:
  redis-data:
```

## Post-Installation

### 1. Create System User
```bash
# Create dedicated user
sudo useradd -r -s /bin/false -d /var/lib/miwidothttp miwidothttp

# Set ownership
sudo chown -R miwidothttp:miwidothttp /etc/miwidothttp
sudo chown -R miwidothttp:miwidothttp /var/log/miwidothttp
sudo chown -R miwidothttp:miwidothttp /var/cache/miwidothttp
sudo chown -R miwidothttp:miwidothttp /var/lib/miwidothttp
```

### 2. Configure Systemd Service

Create `/etc/systemd/system/miwidothttp.service`:
```ini
[Unit]
Description=miwidothttp High-Performance HTTP Server
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=miwidothttp
Group=miwidothttp
ExecStart=/usr/local/bin/miwidothttp --config /etc/miwidothttp/config.toml
ExecReload=/bin/kill -HUP $MAINPID
Restart=always
RestartSec=10

# Security
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/log/miwidothttp /var/cache/miwidothttp /var/lib/miwidothttp

# Resource limits
LimitNOFILE=1000000
LimitNPROC=64000

# Performance
CPUWeight=100
MemoryMax=4G
TasksMax=infinity

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl daemon-reload
sudo systemctl enable miwidothttp
sudo systemctl start miwidothttp
sudo systemctl status miwidothttp
```

### 3. Create Basic Configuration

Create `/etc/miwidothttp/config.toml`:
```toml
[server]
http_port = 8080
https_port = 8443
enable_https = true
workers = 0  # Auto-detect

[ssl]
provider = "cloudflare"
auto_renew = true

[ssl.cloudflare]
api_token = "YOUR_TOKEN_HERE"
zone_id = "YOUR_ZONE_ID"

[logging]
level = "info"
format = "json"

[[logging.outputs]]
type = "file"
path = "/var/log/miwidothttp/server.log"
```

### 4. Set Up Log Rotation

Create `/etc/logrotate.d/miwidothttp`:
```
/var/log/miwidothttp/*.log {
    daily
    rotate 30
    compress
    delaycompress
    missingok
    notifempty
    create 0644 miwidothttp miwidothttp
    sharedscripts
    postrotate
        systemctl reload miwidothttp > /dev/null 2>&1 || true
    endscript
}
```

### 5. Configure Firewall

#### UFW (Ubuntu/Debian)
```bash
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw allow 8080/tcp
sudo ufw allow 8443/tcp
sudo ufw reload
```

#### firewalld (RHEL/CentOS)
```bash
sudo firewall-cmd --permanent --add-port=80/tcp
sudo firewall-cmd --permanent --add-port=443/tcp
sudo firewall-cmd --permanent --add-port=8080/tcp
sudo firewall-cmd --permanent --add-port=8443/tcp
sudo firewall-cmd --reload
```

### 6. Performance Tuning

Add to `/etc/sysctl.conf`:
```bash
# Network performance
net.core.somaxconn = 65535
net.core.netdev_max_backlog = 65535
net.ipv4.tcp_max_syn_backlog = 65535
net.ipv4.tcp_fin_timeout = 10
net.ipv4.tcp_tw_reuse = 1
net.ipv4.tcp_keepalive_time = 60
net.ipv4.ip_local_port_range = 1024 65535

# File descriptors
fs.file-max = 2000000
fs.nr_open = 2000000
```

Apply:
```bash
sudo sysctl -p
```

## Verification

### Check Installation
```bash
# Check version
miwidothttp --version

# Validate configuration
miwidothttp --validate-config /etc/miwidothttp/config.toml

# Dry run
miwidothttp --dry-run --config /etc/miwidothttp/config.toml

# Check service status
sudo systemctl status miwidothttp

# Check logs
sudo journalctl -u miwidothttp -f
tail -f /var/log/miwidothttp/server.log
```

### Test Server
```bash
# Test HTTP
curl http://localhost:8080/health

# Test HTTPS
curl https://localhost:8443/health

# Check metrics
curl http://localhost:8080/metrics

# Load test
wrk -t12 -c400 -d30s http://localhost:8080/
```

## Uninstallation

### Systemd Service
```bash
# Stop and disable service
sudo systemctl stop miwidothttp
sudo systemctl disable miwidothttp
sudo rm /etc/systemd/system/miwidothttp.service
sudo systemctl daemon-reload
```

### Binary Removal
```bash
# Remove binary
sudo rm /usr/local/bin/miwidothttp

# Remove directories
sudo rm -rf /etc/miwidothttp
sudo rm -rf /var/log/miwidothttp
sudo rm -rf /var/cache/miwidothttp
sudo rm -rf /var/lib/miwidothttp

# Remove user
sudo userdel miwidothttp
```

### Package Manager Uninstall
```bash
# APT
sudo apt-get remove --purge miwidothttp

# YUM/DNF
sudo yum remove miwidothttp
# or
sudo dnf remove miwidothttp

# Homebrew
brew uninstall miwidothttp
brew services stop miwidothttp

# Snap
sudo snap remove miwidothttp

# Cargo
cargo uninstall miwidothttp
```

### Docker Cleanup
```bash
# Stop and remove container
docker stop miwidothttp
docker rm miwidothttp

# Remove image
docker rmi miwidothttp/miwidothttp:latest

# Remove volumes
docker volume rm miwidothttp_data
```

## Troubleshooting Installation

### Common Issues

#### Permission Denied
```bash
# Fix permissions
sudo chown -R miwidothttp:miwidothttp /etc/miwidothttp
sudo chmod 755 /usr/local/bin/miwidothttp
```

#### Port Already in Use
```bash
# Find process using port
sudo lsof -i :8080
sudo netstat -tulpn | grep :8080

# Change port in config
sed -i 's/http_port = 8080/http_port = 8081/' /etc/miwidothttp/config.toml
```

#### SSL Certificate Issues
```bash
# Check SSL configuration
miwidothttp --test-ssl

# Regenerate certificates
miwidothttp --regenerate-certs
```

#### Missing Dependencies
```bash
# Check library dependencies (Linux)
ldd /usr/local/bin/miwidothttp

# Install missing libraries
sudo apt-get install -f
```

## Next Steps

- [Configuration Guide](CONFIGURATION.md)
- [Deployment Guide](DEPLOYMENT.md)
- [Quick Start Tutorial](../README.md#quick-start)
- [API Documentation](API.md)