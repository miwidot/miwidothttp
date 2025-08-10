#!/bin/bash

echo "======================================="
echo "DOCKER vs DOCKER PERFORMANCE TEST"
echo "======================================="
echo ""

# Cleanup
docker stop nginx-bench miwidothttp-test 2>/dev/null
docker rm nginx-bench miwidothttp-test 2>/dev/null
pkill -f miwidothttp 2>/dev/null

# Start nginx
echo "Starting nginx (Docker)..."
docker run -d --name nginx-bench -p 8080:80 -v $(pwd)/static:/usr/share/nginx/html:ro nginx:1.25-alpine

# Start miwidothttp
echo "Starting miwidothttp (Docker)..."
docker run -d --name miwidothttp-test -p 9001:9001 -v $(pwd)/static:/app/static:ro miwidothttp:latest

# Wait for containers
sleep 5

echo ""
echo "Container Status:"
docker ps | grep -E "nginx-bench|miwidothttp-test"

echo ""
echo "======================================="
echo "BENCHMARKS (Both in Docker)"
echo "======================================="

echo ""
echo "1. nginx (Docker):"
echo "-------------------"
wrk -t4 -c100 -d10s --latency http://localhost:8080/index.html

echo ""
echo "2. miwidothttp (Docker):"
echo "------------------------"
wrk -t4 -c100 -d10s --latency http://localhost:9001/index.html

echo ""
echo "======================================="
echo "RESOURCE USAGE"
echo "======================================="
docker stats --no-stream nginx-bench miwidothttp-test

# Cleanup
echo ""
echo "Cleaning up..."
docker stop nginx-bench miwidothttp-test
docker rm nginx-bench miwidothttp-test

echo ""
echo "Complete!"