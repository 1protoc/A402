#ifndef A402_WRAPPER_H
#define A402_WRAPPER_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    bool simulate_realistic_delay;  
    int parallel_degree;            
    int batch_size;                 
    int utee_cores;                 
    int mtee_cores;                 
    
    double user_to_utee_ms;         
    double utee_to_mtee_ms;         
    double utee_request_processing_ms;  
    double mtee_execution_ms;       
    double utee_signature_verification_ms;  
    double mtee_reveal_processing_ms;  
    double utee_final_processing_ms;  
    
    double total_delay_ms;          
    double effective_delay_ms;      
} a402_config_t;

typedef struct {
    char channel_id[64];
    double total_amount;
    double user_c_amount;
    double m_tee_amount;
    int state;  
} a402_channel_t;

typedef struct {
    a402_config_t config;
    a402_channel_t* channels;
    int channel_count;
    int max_channels;
} a402_wrapper_t;

a402_wrapper_t* a402_wrapper_init(
    bool simulate_realistic_delay,
    int parallel_degree,
    int batch_size,
    int utee_cores,
    int mtee_cores,
    double mtee_execution_delay_ms,
    double utee_to_mtee_comm_delay_ms  
);

void a402_wrapper_cleanup(a402_wrapper_t* wrapper);

bool a402_create_channel(a402_wrapper_t* wrapper, const char* channel_id, double amount);

bool a402_update_channel(
    a402_wrapper_t* wrapper,
    const char* channel_id,
    double user_c_amount,
    double m_tee_amount
);

bool a402_batch_settle(
    a402_wrapper_t* wrapper,
    const char** channel_ids,
    int channel_count
);

#ifdef __cplusplus
}
#endif

#endif 
