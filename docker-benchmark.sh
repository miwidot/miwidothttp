#!/bin/bash

echo "==================================="
echo "DOCKER CONTAINER BENCHMARK TEST"
echo "==================================="
echo ""

# Cleanup any existing containers
echo "Cleaning up existing containers..."
docker stop miwidothttp-bench nginx-bench 2>/dev/null
docker rm miwidothttp-bench nginx-bench 2>/dev/null
pkill -f miwidothttp 2>/dev/null

# Build miwidothttp Docker image
echo "Building miwidothttp Docker image..."
docker build -f Dockerfile.alpine -t miwidothttp:benchmark . || {
    echo "Failed to build miwidothttp image"
    exit 1
}

# Start nginx container
echo "Starting nginx container..."
docker run -d --name nginx-bench \
    -p 8080:80 \
    -v $(pwd)/static:/usr/share/nginx/html:ro \
    nginx:1.25-alpine

# Start miwidothttp container
echo "Starting miwidothttp container..."
docker run -d --name miwidothttp-bench \
    -p 9001:9001 \
    -v $(pwd)/static:/app/static:ro \
    miwidothttp:benchmark

# Wait for containers to be ready
echo "Waiting for containers to start..."
sleep 5

# Verify both are running
docker ps | grep -E "nginx-bench|miwidothttp-bench"

echo ""
echo "==================================="
echo "RUNNING PERFORMANCE TESTS"
echo "==================================="

echo ""
echo "TEST 1: NGINX (Docker Container)"
echo "---------------------------------"
wrk -t4 -c100 -d10s --latency http://localhost:8080/index.html

echo ""
echo "TEST 2: MIWIDOTHTTP (Docker Container)"
echo "---------------------------------------"
wrk -t4 -c100 -d10s --latency http://localhost:9001/index.html

echo ""
echo "==================================="
echo "CONTAINER RESOURCE USAGE"
echo "==================================="
echo ""
echo "Resource usage during benchmark:"
docker stats --no-stream nginx-bench miwidothttp-bench

# Cleanup
echo ""
echo "Cleaning up..."
docker stop miwidothttp-bench nginx-bench
docker rm miwidothttp-bench nginx-bench

echo ""
echo "==================================="
echo "BENCHMARK COMPLETE!"
echo "====================================="