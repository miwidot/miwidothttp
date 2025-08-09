#!/bin/bash
# Performance benchmark setup script for miwidothttp vs nginx

set -e

echo "Setting up benchmark environment..."

# Install dependencies
if ! command -v wrk &> /dev/null; then
    echo "Installing wrk (HTTP benchmarking tool)..."
    if [[ "$OSTYPE" == "darwin"* ]]; then
        brew install wrk
    else
        sudo apt-get update && sudo apt-get install -y wrk
    fi
fi

if ! command -v nginx &> /dev/null; then
    echo "Installing nginx..."
    if [[ "$OSTYPE" == "darwin"* ]]; then
        brew install nginx
    else
        sudo apt-get install -y nginx
    fi
fi

# Create test directories
mkdir -p benchmark/results
mkdir -p benchmark/static
mkdir -p benchmark/configs

# Create test HTML file
cat > benchmark/static/index.html << 'EOF'
<!DOCTYPE html>
<html>
<head><title>Benchmark Test</title></head>
<body>
<h1>Performance Test Page</h1>
<p>Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.</p>
</body>
</html>
EOF

# Create 1KB, 10KB, 100KB, 1MB test files
dd if=/dev/zero of=benchmark/static/1kb.bin bs=1024 count=1 2>/dev/null
dd if=/dev/zero of=benchmark/static/10kb.bin bs=1024 count=10 2>/dev/null
dd if=/dev/zero of=benchmark/static/100kb.bin bs=1024 count=100 2>/dev/null
dd if=/dev/zero of=benchmark/static/1mb.bin bs=1024 count=1024 2>/dev/null

# Create nginx config for benchmarking
cat > benchmark/configs/nginx-bench.conf << 'EOF'
worker_processes auto;
worker_rlimit_nofile 100000;

events {
    worker_connections 4000;
    use epoll;
    multi_accept on;
}

http {
    open_file_cache max=200000 inactive=20s;
    open_file_cache_valid 30s;
    open_file_cache_min_uses 2;
    open_file_cache_errors on;

    access_log off;
    sendfile on;
    tcp_nopush on;
    tcp_nodelay on;

    gzip on;
    gzip_comp_level 2;
    gzip_min_length 1000;
    gzip_types text/plain application/json text/css text/javascript;

    keepalive_timeout 65;
    keepalive_requests 100000;

    server {
        listen 9080;
        server_name localhost;
        
        location / {
            root /tmp/benchmark-static;
            index index.html;
        }
    }
}
EOF

# Copy static files to nginx serving directory
sudo mkdir -p /tmp/benchmark-static
sudo cp benchmark/static/* /tmp/benchmark-static/

echo "Benchmark environment setup complete!"