#!/bin/bash

# Comprehensive Benchmark Script for miwidothttp vs nginx
# This script performs various load tests and generates a detailed report

set -e

echo "==================================================================="
echo "           PERFORMANCE BENCHMARK: miwidothttp vs nginx"
echo "==================================================================="
echo ""

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Check for required tools
check_requirements() {
    echo "Checking requirements..."
    
    for tool in docker docker-compose wrk ab curl; do
        if ! command -v $tool &> /dev/null; then
            echo -e "${RED}❌ $tool is not installed${NC}"
            echo "Please install $tool to continue"
            exit 1
        else
            echo -e "${GREEN}✓${NC} $tool found"
        fi
    done
    echo ""
}

# Start containers
start_containers() {
    echo -e "${BLUE}Starting Docker containers...${NC}"
    docker-compose -f docker-compose-benchmark.yml down 2>/dev/null || true
    docker-compose -f docker-compose-benchmark.yml build
    docker-compose -f docker-compose-benchmark.yml up -d
    
    echo "Waiting for services to be ready..."
    sleep 10
    
    # Health check
    for port in 8081 8082 8083 8084 9001 9002; do
        if curl -s -o /dev/null -w "%{http_code}" http://localhost:$port/health | grep -q "200\|404"; then
            echo -e "${GREEN}✓${NC} Service on port $port is ready"
        else
            echo -e "${RED}✗${NC} Service on port $port is not responding"
        fi
    done
    echo ""
}

# Warm up servers
warmup() {
    echo -e "${YELLOW}Warming up servers...${NC}"
    for port in 9001 9002; do
        wrk -t2 -c10 -d10s --latency http://localhost:$port/index.html > /dev/null 2>&1
    done
    echo "Warmup complete"
    echo ""
}

# Run benchmark test
run_test() {
    local name=$1
    local url=$2
    local threads=$3
    local connections=$4
    local duration=$5
    local file=$6
    
    echo -e "${BLUE}Test: $name${NC}"
    echo "URL: $url"
    echo "Threads: $threads, Connections: $connections, Duration: ${duration}s"
    echo "----------------------------------------"
    
    # Run wrk benchmark
    wrk -t$threads -c$connections -d${duration}s --latency $url 2>&1 | tee ${file}
    
    # Extract key metrics
    local requests=$(grep "Requests/sec:" ${file} | awk '{print $2}')
    local latency_avg=$(grep "Latency" ${file} | awk '{print $2}')
    local latency_99=$(grep "99%" ${file} | awk '{print $2}')
    local errors=$(grep "Non-2xx" ${file} | awk '{print $NF}' || echo "0")
    
    echo ""
    echo -e "${GREEN}Results:${NC}"
    echo "  Requests/sec: $requests"
    echo "  Avg Latency: $latency_avg"
    echo "  99% Latency: $latency_99"
    echo "  Errors: $errors"
    echo ""
    
    # Save to results file
    echo "$name,$requests,$latency_avg,$latency_99,$errors" >> results.csv
}

# Run Apache Bench test for comparison
run_ab_test() {
    local name=$1
    local url=$2
    local requests=$3
    local concurrency=$4
    local file=$5
    
    echo -e "${BLUE}Apache Bench Test: $name${NC}"
    echo "Requests: $requests, Concurrency: $concurrency"
    echo "----------------------------------------"
    
    ab -n $requests -c $concurrency -k $url 2>&1 | tee ${file}
    
    # Extract metrics
    local rps=$(grep "Requests per second:" ${file} | awk '{print $4}')
    local tpr=$(grep "Time per request:" ${file} | head -1 | awk '{print $4}')
    local transfer=$(grep "Transfer rate:" ${file} | awk '{print $3}')
    
    echo ""
    echo -e "${GREEN}Results:${NC}"
    echo "  Requests/sec: $rps"
    echo "  Time per request: ${tpr}ms"
    echo "  Transfer rate: ${transfer} KB/sec"
    echo ""
}

# Main benchmark suite
run_benchmark_suite() {
    echo -e "${YELLOW}=== STARTING BENCHMARK SUITE ===${NC}"
    echo ""
    
    # Create results directory
    RESULTS_DIR="benchmark-results-$(date +%Y%m%d-%H%M%S)"
    mkdir -p $RESULTS_DIR
    cd $RESULTS_DIR
    
    # Initialize results CSV
    echo "Test,Requests/sec,Avg Latency,99% Latency,Errors" > results.csv
    
    # Test 1: Small file, low concurrency
    echo -e "${YELLOW}--- Test 1: Small File, Low Concurrency ---${NC}"
    run_test "miwidothttp-small-low" "http://localhost:9001/1kb.txt" 2 50 30 "miwidothttp-1.txt"
    run_test "nginx-small-low" "http://localhost:9002/1kb.txt" 2 50 30 "nginx-1.txt"
    
    # Test 2: Small file, high concurrency
    echo -e "${YELLOW}--- Test 2: Small File, High Concurrency ---${NC}"
    run_test "miwidothttp-small-high" "http://localhost:9001/1kb.txt" 8 500 30 "miwidothttp-2.txt"
    run_test "nginx-small-high" "http://localhost:9002/1kb.txt" 8 500 30 "nginx-2.txt"
    
    # Test 3: Medium file, medium concurrency
    echo -e "${YELLOW}--- Test 3: Medium File (100KB), Medium Concurrency ---${NC}"
    run_test "miwidothttp-medium" "http://localhost:9001/100kb.bin" 4 200 30 "miwidothttp-3.txt"
    run_test "nginx-medium" "http://localhost:9002/100kb.bin" 4 200 30 "nginx-3.txt"
    
    # Test 4: Large file, low concurrency
    echo -e "${YELLOW}--- Test 4: Large File (1MB), Low Concurrency ---${NC}"
    run_test "miwidothttp-large" "http://localhost:9001/1mb.bin" 2 50 30 "miwidothttp-4.txt"
    run_test "nginx-large" "http://localhost:9002/1mb.bin" 2 50 30 "nginx-4.txt"
    
    # Test 5: HTML page, mixed load
    echo -e "${YELLOW}--- Test 5: HTML Page, Mixed Load ---${NC}"
    run_test "miwidothttp-html" "http://localhost:9001/index.html" 4 250 30 "miwidothttp-5.txt"
    run_test "nginx-html" "http://localhost:9002/index.html" 4 250 30 "nginx-5.txt"
    
    # Test 6: Extreme concurrency test
    echo -e "${YELLOW}--- Test 6: Extreme Concurrency Test ---${NC}"
    run_test "miwidothttp-extreme" "http://localhost:9001/1kb.txt" 16 1000 30 "miwidothttp-6.txt"
    run_test "nginx-extreme" "http://localhost:9002/1kb.txt" 16 1000 30 "nginx-6.txt"
    
    # Apache Bench comparison
    echo -e "${YELLOW}--- Apache Bench Comparison ---${NC}"
    run_ab_test "miwidothttp-ab" "http://localhost:9001/index.html" 10000 100 "miwidothttp-ab.txt"
    run_ab_test "nginx-ab" "http://localhost:9002/index.html" 10000 100 "nginx-ab.txt"
    
    cd ..
}

# Generate report
generate_report() {
    echo ""
    echo -e "${YELLOW}=== GENERATING REPORT ===${NC}"
    echo ""
    
    cd $RESULTS_DIR
    
    # Create summary report
    cat > report.md << EOF
# Benchmark Report: miwidothttp vs nginx
Date: $(date)

## Test Environment
- Docker containers with 2 CPU cores and 1GB RAM each
- 2 instances of each server behind HAProxy load balancer
- Tests performed with wrk and Apache Bench

## Results Summary

### Requests per Second (Higher is Better)
\`\`\`
$(grep miwidothttp results.csv | awk -F',' '{sum+=$2; count++} END {print "miwidothttp Average: " sum/count " req/s"}')
$(grep nginx results.csv | awk -F',' '{sum+=$2; count++} END {print "nginx Average:       " sum/count " req/s"}')
\`\`\`

### Test Results Table
| Test | miwidothttp (req/s) | nginx (req/s) | Winner | Margin |
|------|---------------------|---------------|---------|---------|
EOF
    
    # Compare results
    while IFS=',' read -r test rps lat99 lat_avg errors; do
        if [[ $test == *"miwidothttp"* ]]; then
            miwi_rps=$rps
            test_name=${test%-miwidothttp*}
        elif [[ $test == *"nginx"* ]]; then
            nginx_rps=$rps
            if (( $(echo "$miwi_rps > $nginx_rps" | bc -l) )); then
                winner="miwidothttp"
                margin=$(echo "scale=2; ($miwi_rps - $nginx_rps) / $nginx_rps * 100" | bc)
                echo "| $test_name | $miwi_rps | $nginx_rps | **$winner** | +${margin}% |" >> report.md
            else
                winner="nginx"
                margin=$(echo "scale=2; ($nginx_rps - $miwi_rps) / $miwi_rps * 100" | bc)
                echo "| $test_name | $miwi_rps | $nginx_rps | $winner | +${margin}% |" >> report.md
            fi
        fi
    done < results.csv
    
    echo "" >> report.md
    echo "## Conclusion" >> report.md
    
    # Calculate overall winner
    miwi_total=$(grep miwidothttp results.csv | awk -F',' '{sum+=$2} END {print sum}')
    nginx_total=$(grep nginx results.csv | awk -F',' '{sum+=$2} END {print sum}')
    
    if (( $(echo "$miwi_total > $nginx_total" | bc -l) )); then
        improvement=$(echo "scale=2; ($miwi_total - $nginx_total) / $nginx_total * 100" | bc)
        echo "**miwidothttp outperformed nginx by ${improvement}% overall**" >> report.md
    else
        improvement=$(echo "scale=2; ($nginx_total - $miwi_total) / $miwi_total * 100" | bc)
        echo "nginx outperformed miwidothttp by ${improvement}% overall" >> report.md
    fi
    
    cd ..
    
    echo ""
    echo -e "${GREEN}Report generated: $RESULTS_DIR/report.md${NC}"
    cat $RESULTS_DIR/report.md
}

# Cleanup
cleanup() {
    echo ""
    echo -e "${YELLOW}Cleaning up...${NC}"
    docker-compose -f docker-compose-benchmark.yml down
    echo -e "${GREEN}Cleanup complete${NC}"
}

# Main execution
main() {
    check_requirements
    start_containers
    warmup
    run_benchmark_suite
    generate_report
    cleanup
    
    echo ""
    echo -e "${GREEN}=== BENCHMARK COMPLETE ===${NC}"
    echo "Results saved in: $RESULTS_DIR"
}

# Handle interrupts
trap cleanup EXIT

# Run benchmark
main