#!/bin/bash

# Quick benchmark test script - simplified version

set -e

echo "==================================================================="
echo "        QUICK PERFORMANCE TEST: miwidothttp vs nginx"
echo "==================================================================="
echo ""

# Build Docker images first
echo "Building Docker images..."
docker build -t miwidothttp:benchmark .

# Create a simple docker-compose for testing
cat > docker-compose-quick.yml << 'EOF'
version: '3.8'

services:
  miwidothttp:
    image: miwidothttp:benchmark
    container_name: miwidothttp-test
    ports:
      - "8080:8080"
    volumes:
      - ./config-benchmark.toml:/etc/miwidothttp/config.toml:ro
      - ./static-test:/var/www/html:ro
    environment:
      - RUST_LOG=error

  nginx:
    image: nginx:1.25-alpine
    container_name: nginx-test
    ports:
      - "8081:80"
    volumes:
      - ./nginx-benchmark.conf:/etc/nginx/nginx.conf:ro
      - ./static-test:/var/www/html:ro
EOF

# Start containers
echo "Starting containers..."
docker compose -f docker-compose-quick.yml down 2>/dev/null || true
docker compose -f docker-compose-quick.yml up -d

# Wait for services
echo "Waiting for services to start..."
sleep 5

# Check if services are running
echo ""
echo "Checking services..."
for port in 8080 8081; do
    if curl -s -o /dev/null -w "%{http_code}" http://localhost:$port/index.html | grep -q "200"; then
        echo "✓ Service on port $port is ready"
    else
        echo "✗ Service on port $port failed"
    fi
done

echo ""
echo "Running quick benchmark tests..."
echo "================================"

# Function to run a simple test
run_quick_test() {
    local name=$1
    local url=$2
    
    echo ""
    echo "Testing $name..."
    echo "URL: $url"
    
    # Use Apache Bench for quick test
    if command -v ab &> /dev/null; then
        ab -n 1000 -c 10 -k $url 2>&1 | grep -E "Requests per second:|Time per request:|Transfer rate:"
    elif command -v wrk &> /dev/null; then
        wrk -t2 -c10 -d10s $url 2>&1 | grep -E "Requests/sec:|Latency"
    else
        # Fallback to curl
        echo "Using curl for basic test (install ab or wrk for better results)..."
        time for i in {1..100}; do curl -s $url > /dev/null; done
    fi
}

# Run tests
echo ""
echo "Test 1: Small HTML file"
echo "-----------------------"
run_quick_test "miwidothttp" "http://localhost:8080/index.html"
run_quick_test "nginx" "http://localhost:8081/index.html"

echo ""
echo "Test 2: 1KB text file"
echo "--------------------"
run_quick_test "miwidothttp" "http://localhost:8080/1kb.txt"
run_quick_test "nginx" "http://localhost:8081/1kb.txt"

echo ""
echo "Test 3: 100KB binary file"
echo "-------------------------"
run_quick_test "miwidothttp" "http://localhost:8080/100kb.bin"
run_quick_test "nginx" "http://localhost:8081/100kb.bin"

# Cleanup
echo ""
echo "Cleaning up..."
docker compose -f docker-compose-quick.yml down

echo ""
echo "Quick benchmark complete!"
echo ""
echo "For comprehensive benchmarks, run: ./benchmark.sh"