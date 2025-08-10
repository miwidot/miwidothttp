#!/bin/bash

echo "=========================================="
echo "COMPREHENSIVE WEB SERVER BENCHMARK"
echo "=========================================="
echo ""
echo "Test Date: $(date)"
echo "Test File: static/index.html (346 bytes)"
echo "Test Tool: wrk (4 threads, 100 connections, 10 seconds)"
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Results array
declare -A results

# Function to run benchmark
run_benchmark() {
    local name=$1
    local port=$2
    local url="http://localhost:${port}/index.html"
    
    echo -e "${BLUE}Testing ${name}...${NC}"
    
    # Check if server is responding
    if ! curl -s -o /dev/null -w "%{http_code}" "$url" | grep -q "200"; then
        echo -e "${RED}${name} is not responding on port ${port}${NC}"
        results[$name]="Not Available"
        return
    fi
    
    # Run benchmark
    result=$(wrk -t4 -c100 -d10s --latency "$url" 2>/dev/null | grep "Requests/sec" | awk '{print $2}')
    
    if [ -z "$result" ]; then
        results[$name]="Failed"
    else
        results[$name]=$result
        echo -e "${GREEN}${name}: ${result} RPS${NC}"
    fi
    
    sleep 2
}

# Clean up existing containers
echo "Cleaning up existing containers..."
docker-compose -f docker-compose.benchmark.yml down 2>/dev/null
docker stop bench-miwidothttp bench-nginx bench-apache bench-frankenphp bench-caddy bench-litespeed bench-node bench-bun 2>/dev/null
docker rm bench-miwidothttp bench-nginx bench-apache bench-frankenphp bench-caddy bench-litespeed bench-node bench-bun 2>/dev/null

# Build Node.js dependencies
echo "Preparing Node.js server..."
cd node-server && npm install --silent && cd ..

# Start all containers
echo ""
echo "Starting all containers..."
docker-compose -f docker-compose.benchmark.yml up -d

# Wait for containers to be ready
echo "Waiting for containers to start..."
sleep 10

# Check running containers
echo ""
echo "Running containers:"
docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}" | grep bench-

echo ""
echo "=========================================="
echo "RUNNING BENCHMARKS"
echo "=========================================="
echo ""

# Run benchmarks
run_benchmark "miwidothttp" 9002
run_benchmark "nginx" 8001
run_benchmark "Apache2" 8002
run_benchmark "FrankenPHP" 8003
run_benchmark "Caddy" 8004
run_benchmark "LiteSpeed" 8005
run_benchmark "Node.js Express" 8006
run_benchmark "Bun" 8007

# Sort and display results
echo ""
echo "=========================================="
echo "BENCHMARK RESULTS (Sorted by Performance)"
echo "=========================================="
echo ""

# Create temp file for sorting
temp_file=$(mktemp)

# Write results to temp file
for server in "${!results[@]}"; do
    if [[ ${results[$server]} != "Failed" ]] && [[ ${results[$server]} != "Not Available" ]]; then
        echo "${results[$server]} $server" >> $temp_file
    fi
done

# Sort and display
echo "| Rank | Server | Requests/Second |"
echo "|------|--------|-----------------|"
rank=1
while IFS=' ' read -r rps server; do
    printf "| %d | %-15s | %s |\n" $rank "$server" "$rps"
    rank=$((rank + 1))
done < <(sort -rn $temp_file)

# Show failed servers
echo ""
echo "Failed or unavailable servers:"
for server in "${!results[@]}"; do
    if [[ ${results[$server]} == "Failed" ]] || [[ ${results[$server]} == "Not Available" ]]; then
        echo "- $server: ${results[$server]}"
    fi
done

# Clean up temp file
rm -f $temp_file

# Container stats
echo ""
echo "=========================================="
echo "RESOURCE USAGE"
echo "=========================================="
docker stats --no-stream bench-miwidothttp bench-nginx bench-apache bench-frankenphp bench-caddy bench-litespeed bench-node bench-bun 2>/dev/null | grep -v "CONTAINER ID"

# Clean up
echo ""
echo "Cleaning up..."
docker-compose -f docker-compose.benchmark.yml down

echo ""
echo "=========================================="
echo "BENCHMARK COMPLETE!"
echo "=========================================="