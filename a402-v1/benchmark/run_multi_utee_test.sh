
echo "Test scenario: n U-TEEs -> coordinator TEE -> M-TEE"

NUM_REQUESTS=1000
NUM_MTEES=64
UTEE_TO_COORD_DELAY=5.0
COORD_TO_MTEE_DELAY=10.0
MTEE_PROC_DELAY=300.0
NUM_RUNS=5

UTEE_COUNTS=(2 4 6 8 10)

RESULTS_FILE="multi_utee_coordination_results.json"

echo "{" > $RESULTS_FILE
echo "  \"test_config\": {" >> $RESULTS_FILE
echo "    \"num_requests\": $NUM_REQUESTS," >> $RESULTS_FILE
echo "    \"num_mtees\": $NUM_MTEES," >> $RESULTS_FILE
echo "    \"utee_to_coordinator_delay_ms\": $UTEE_TO_COORD_DELAY," >> $RESULTS_FILE
echo "    \"coordinator_to_mtee_delay_ms\": $COORD_TO_MTEE_DELAY," >> $RESULTS_FILE
echo "    \"mtee_proc_delay_ms\": $MTEE_PROC_DELAY," >> $RESULTS_FILE
echo "    \"num_runs\": $NUM_RUNS" >> $RESULTS_FILE
echo "  }," >> $RESULTS_FILE
echo "  \"results\": [" >> $RESULTS_FILE

FIRST=true

for NUM_UTEES in "${UTEE_COUNTS[@]}"; do
    
    OUTPUT_FILE="multi_utee_${NUM_UTEES}_results.json"
    
    ./test_multi_utee_coordination \
        --requests $NUM_REQUESTS \
        --utees $NUM_UTEES \
        --mtees $NUM_MTEES \
        --utee-to-coord-delay $UTEE_TO_COORD_DELAY \
        --coord-to-mtee-delay $COORD_TO_MTEE_DELAY \
        --mtee-proc-delay $MTEE_PROC_DELAY \
        --runs $NUM_RUNS \
        --output $OUTPUT_FILE
    
    if [ $? -eq 0 ] && [ -f "$OUTPUT_FILE" ]; then

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
        "num_utees": $NUM_UTEES,
        "completed_operations": data["results"]["completed_operations"],
        "throughput_ops_per_sec": data["results"]["throughput_ops_per_sec"],
        "avg_latency_ms": data["results"]["avg_latency_ms"],
        "min_latency_ms": data["results"]["min_latency_ms"],
        "max_latency_ms": data["results"]["max_latency_ms"],
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
