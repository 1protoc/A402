
#ifndef BENCHMARK_FRAMEWORK_H
#define BENCHMARK_FRAMEWORK_H

#include <stdint.h>
#include <time.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uint64_t create_channel_time_us;      
    uint64_t update_channel_time_us;      
    uint64_t settle_time_us;              
    uint64_t close_channel_time_us;       
    uint64_t total_time_us;               
    uint64_t memory_usage_bytes;          
    uint32_t transaction_size_bytes;      
    uint32_t operations_count;            
} benchmark_metrics_t;

typedef struct {
    char protocol_name[16];               
    benchmark_metrics_t metrics;          
    double throughput_ops_per_sec;       
    double latency_ms;                   
} benchmark_result_t;

typedef struct {
    uint32_t num_channels;               
    uint32_t num_payments_per_channel;   
    uint32_t num_iterations;             
    uint8_t enable_memory_tracking;      
    uint8_t enable_tx_size_tracking;     
} benchmark_config_t;

int benchmark_a402(const benchmark_config_t* config, benchmark_result_t* result);

int benchmark_x402(const benchmark_config_t* config, benchmark_result_t* result);

void compare_results(const benchmark_result_t* a402_result, const benchmark_result_t* x402_result);

void generate_report(const benchmark_result_t* a402_result, 
                    const benchmark_result_t* x402_result,
                    const char* output_file);

#ifdef __cplusplus
}
#endif

#endif 
