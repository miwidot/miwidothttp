#!/bin/bash

echo "Starting benchmark comparison..."
echo "================================"

# Kill any existing servers
pkill -f miwidothttp 2>/dev/null
docker stop nginx-bench 2>/dev/null
docker rm nginx-bench 2>/dev/null

# Start nginx
echo "Starting nginx..."
docker run -d --name nginx-bench -p 8080:80 -v $(pwd)/static:/usr/share/nginx/html:ro nginx:1.25-alpine

# Start miwidothttp
echo "Starting miwidothttp..."
./target/release/miwidothttp > /dev/null 2>&1 &
MIWI_PID=$!

# Wait for servers to start
sleep 3

echo ""
echo "Running benchmarks..."
echo "===================="

echo ""
echo "1. nginx performance (Docker):"
echo "------------------------------"
wrk -t4 -c100 -d10s --latency http://localhost:8080/index.html

echo ""
echo "2. miwidothttp performance (Native):"
echo "------------------------------------"
wrk -t4 -c100 -d10s --latency http://localhost:9001/index.html

# Cleanup
echo ""
echo "Cleaning up..."
kill $MIWI_PID 2>/dev/null
docker stop nginx-bench
docker rm nginx-bench

echo ""
echo "Benchmark complete!"