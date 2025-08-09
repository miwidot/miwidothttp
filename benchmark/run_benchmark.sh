#!/bin/bash
# Performance benchmark runner for miwidothttp vs nginx

set -e

# Configuration
DURATION=30
THREADS=12
CONNECTIONS=400
WARMUP_DURATION=10

# Test URLs
declare -a TESTS=(
    "/:Homepage"
    "/1kb.bin:1KB_File"
    "/10kb.bin:10KB_File"
    "/100kb.bin:100KB_File"
    "/1mb.bin:1MB_File"
)

# Results directory
RESULTS_DIR="benchmark/results/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

echo "================================"
echo "HTTP Server Performance Benchmark"
echo "================================"
echo "Duration: ${DURATION}s"
echo "Threads: ${THREADS}"
echo "Connections: ${CONNECTIONS}"
echo "Results: ${RESULTS_DIR}"
echo ""

# Function to run benchmark
run_benchmark() {
    local server_name=$1
    local port=$2
    local test_path=$3
    local test_name=$4
    
    echo "Testing ${server_name} - ${test_name}..."
    
    # Warmup
    wrk -t${THREADS} -c${CONNECTIONS} -d${WARMUP_DURATION}s \
        --latency "http://localhost:${port}${test_path}" > /dev/null 2>&1
    
    # Actual test
    wrk -t${THREADS} -c${CONNECTIONS} -d${DURATION}s \
        --latency \
        --script benchmark/report.lua \
        "http://localhost:${port}${test_path}" \
        > "${RESULTS_DIR}/${server_name}_${test_name}.txt" 2>&1
    
    # Extract key metrics
    local requests_sec=$(grep "Requests/sec:" "${RESULTS_DIR}/${server_name}_${test_name}.txt" | awk '{print $2}')
    local latency_avg=$(grep "Latency" "${RESULTS_DIR}/${server_name}_${test_name}.txt" | head -1 | awk '{print $2}')
    local latency_99=$(grep "99%" "${RESULTS_DIR}/${server_name}_${test_name}.txt" | awk '{print $2}')
    
    echo "  Requests/sec: ${requests_sec}"
    echo "  Latency (avg): ${latency_avg}"
    echo "  Latency (99%): ${latency_99}"
    echo ""
}

# Create Lua script for detailed reporting
cat > benchmark/report.lua << 'EOF'
done = function(summary, latency, requests)
    io.write("------------------------------\n")
    io.write(string.format("Requests:     %d\n", summary.requests))
    io.write(string.format("Duration:     %d ms\n", summary.duration))
    io.write(string.format("Errors:       %d\n", summary.errors.total))
    io.write(string.format("Requests/sec: %.2f\n", (summary.requests/summary.duration)*1000000))
    io.write(string.format("Bytes/sec:    %.2f KB\n", (summary.bytes/summary.duration)*1000))
    io.write("------------------------------\n")
    io.write("Latency Distribution:\n")
    for _, p in pairs({50, 75, 90, 99, 99.9}) do
        n = latency:percentile(p)
        io.write(string.format("  %g%%: %.2f ms\n", p, n/1000))
    end
end
EOF

# Start miwidothttp
echo "Starting miwidothttp..."
cargo build --release
./target/release/miwidothttp > /dev/null 2>&1 &
MIWI_PID=$!
sleep 2

# Start nginx
echo "Starting nginx..."
nginx -c "$(pwd)/benchmark/configs/nginx-bench.conf" -g "daemon off;" > /dev/null 2>&1 &
NGINX_PID=$!
sleep 2

echo ""
echo "Running benchmarks..."
echo "================================"

# Run tests for each server
for test in "${TESTS[@]}"; do
    IFS=':' read -r path name <<< "$test"
    
    echo "Test: ${name} (${path})"
    echo "--------------------------------"
    run_benchmark "miwidothttp" 8080 "$path" "$name"
    run_benchmark "nginx" 9080 "$path" "$name"
done

# Cleanup
echo "Cleaning up..."
kill $MIWI_PID 2>/dev/null || true
kill $NGINX_PID 2>/dev/null || true

# Generate summary report
echo "Generating summary report..."
cat > "${RESULTS_DIR}/summary.md" << EOF
# Performance Benchmark Results
Date: $(date)

## Configuration
- Duration: ${DURATION}s per test
- Threads: ${THREADS}
- Connections: ${CONNECTIONS}

## Results Summary

| Test | Server | Requests/sec | Latency (avg) | Latency (99%) |
|------|--------|--------------|---------------|---------------|
EOF

for test in "${TESTS[@]}"; do
    IFS=':' read -r path name <<< "$test"
    
    for server in miwidothttp nginx; do
        if [ -f "${RESULTS_DIR}/${server}_${name}.txt" ]; then
            requests_sec=$(grep "Requests/sec:" "${RESULTS_DIR}/${server}_${name}.txt" | awk '{print $2}')
            latency_avg=$(grep "Latency" "${RESULTS_DIR}/${server}_${name}.txt" | head -1 | awk '{print $2}')
            latency_99=$(grep "99%" "${RESULTS_DIR}/${server}_${name}.txt" | awk '{print $2}')
            echo "| ${name} | ${server} | ${requests_sec} | ${latency_avg} | ${latency_99} |" >> "${RESULTS_DIR}/summary.md"
        fi
    done
done

echo ""
echo "================================"
echo "Benchmark Complete!"
echo "Results saved to: ${RESULTS_DIR}"
echo "Summary: ${RESULTS_DIR}/summary.md"