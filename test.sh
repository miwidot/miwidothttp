#!/bin/bash

# Test script for miwidothttp

set -e

echo "🧪 Testing miwidothttp server..."
echo ""

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Start server in background
echo "Starting server..."
./target/release/miwidothttp > test.log 2>&1 &
SERVER_PID=$!

# Wait for server to start
sleep 3

# Function to test endpoint
test_endpoint() {
    local url=$1
    local expected_status=$2
    local description=$3
    
    echo -n "Testing $description... "
    
    status=$(curl -s -o /dev/null -w "%{http_code}" $url)
    
    if [ "$status" = "$expected_status" ]; then
        echo -e "${GREEN}✓${NC} ($status)"
        return 0
    else
        echo -e "${RED}✗${NC} (got $status, expected $expected_status)"
        return 1
    fi
}

# Run tests
echo ""
echo "Running tests:"
echo "--------------"

# Basic endpoints
test_endpoint "http://localhost:8080/health" "200" "Health check"
test_endpoint "http://localhost:8080/api/status" "200" "API status"
test_endpoint "http://localhost:8080/metrics" "200" "Metrics endpoint"
test_endpoint "http://localhost:8080/api/processes" "200" "Process list"
test_endpoint "http://localhost:8080/api/backends" "200" "Backend list"

# Security headers
echo ""
echo "Checking security headers:"
echo "--------------------------"

headers=$(curl -s -I http://localhost:8080/)
if echo "$headers" | grep -q "x-frame-options"; then
    echo -e "${GREEN}✓${NC} X-Frame-Options present"
else
    echo -e "${RED}✗${NC} X-Frame-Options missing"
fi

if echo "$headers" | grep -q "x-content-type-options"; then
    echo -e "${GREEN}✓${NC} X-Content-Type-Options present"
else
    echo -e "${RED}✗${NC} X-Content-Type-Options missing"
fi

# HTTPS test (if configured)
echo ""
echo "Testing HTTPS (if enabled):"
echo "---------------------------"

if curl -k -s https://localhost:8443/health > /dev/null 2>&1; then
    echo -e "${GREEN}✓${NC} HTTPS is working"
else
    echo -e "⚠️  HTTPS not configured or not working"
fi

# Performance test
echo ""
echo "Quick performance test:"
echo "-----------------------"

echo "Sending 100 requests..."
time for i in {1..100}; do
    curl -s http://localhost:8080/health > /dev/null
done

echo ""
echo "Checking metrics after test:"
echo "----------------------------"

metrics=$(curl -s http://localhost:8080/metrics)
requests=$(echo "$metrics" | grep "http_requests_total" | head -1)
if [ ! -z "$requests" ]; then
    echo "✓ Metrics are being collected"
    echo "  $requests"
else
    echo "⚠️  No metrics found"
fi

# Cleanup
echo ""
echo "Cleaning up..."
kill $SERVER_PID 2>/dev/null || true
rm -f test.log

echo ""
echo "✅ Test complete!"