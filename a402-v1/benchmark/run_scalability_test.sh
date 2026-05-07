
NUM_MTEES=64
COMM_DELAY=10.0
PROC_DELAY=300.0

REQUESTS=(100 500 1000 2000 5000 10000)

RESULTS_FILE="scalability_test_results.json"

echo "{" > $RESULTS_FILE
echo "  \"test_config\": {" >> $RESULTS_FILE
echo "    \"num_mtees\": $NUM_MTEES," >> $RESULTS_FILE
echo "    \"communication_delay_ms\": $COMM_DELAY," >> $RESULTS_FILE
echo "    \"processing_delay_ms\": $PROC_DELAY," >> $RESULTS_FILE
echo "    \"parallel_threads_per_mtee\": 100" >> $RESULTS_FILE
echo "  }," >> $RESULTS_FILE
echo "  \"results\": [" >> $RESULTS_FILE

FIRST=true

for NUM_REQUESTS in "${REQUESTS[@]}"; do
    
    OUTPUT_FILE="scalability_${NUM_REQUESTS}_requests.json"
    
    timeout 300 ./test_utee_mtee_distribution \
        --requests $NUM_REQUESTS \
        --mtees $NUM_MTEES \
        --comm-delay $COMM_DELAY \
        --proc-delay $PROC_DELAY \
        --output $OUTPUT_FILE
    
    if [ $? -eq 0 ]; then

        if [ "$FIRST" = false ]; then
            echo "," >> $RESULTS_FILE
        fi
        FIRST=false
        
        python3 << EOF
import json
import sys

try:
    with open('$OUTPUT_FILE', 'r') as f:
        data = json.load(f)
    
    result = {
        "num_requests": $NUM_REQUESTS,
        "completed_operations": data["results"]["completed_operations"],
        "throughput_ops_per_sec": data["results"]["throughput_ops_per_sec"],
        "avg_latency_ms": data["results"]["avg_latency_ms"],
        "min_latency_ms": data["results"].get("min_latency_ms", data["results"]["avg_latency_ms"]),
        "max_latency_ms": data["results"].get("max_latency_ms", data["results"]["avg_latency_ms"]),
        "total_time_sec": data["results"]["total_time_sec"]
    }
    
    print(json.dumps(result, indent=2), end='')
except Exception as e:
    print(f'{{"error": "Failed to parse {OUTPUT_FILE}: {e}"}}', file=sys.stderr)
    sys.exit(1)
EOF
        echo "" >> $RESULTS_FILE
    else
    fi
done

echo "  ]" >> $RESULTS_FILE
echo "}" >> $RESULTS_FILE

