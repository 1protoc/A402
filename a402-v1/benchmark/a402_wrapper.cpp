#include "a402_wrapper.h"
#include <cstdlib>
#include <cstring>
#include <cmath>
#include <thread>
#include <chrono>
#include <mutex>
#include <vector>
#include <algorithm>

static void precise_sleep(double seconds) {
    if (seconds <= 0.0) return;
    
    auto start = std::chrono::high_resolution_clock::now();
    auto end = start + std::chrono::duration<double>(seconds);
    
    while (std::chrono::high_resolution_clock::now() < end) {
        std::this_thread::yield();
    }
}

a402_wrapper_t* a402_wrapper_init(
    bool simulate_realistic_delay,
    int parallel_degree,
    int batch_size,
    int utee_cores,
    int mtee_cores,
    double mtee_execution_delay_ms,
    double utee_to_mtee_comm_delay_ms
) {
    a402_wrapper_t* wrapper = new a402_wrapper_t();
    
    wrapper->config.simulate_realistic_delay = simulate_realistic_delay;
    wrapper->config.parallel_degree = parallel_degree;
    wrapper->config.batch_size = batch_size;
    wrapper->config.utee_cores = utee_cores;
    wrapper->config.mtee_cores = mtee_cores;
    
    wrapper->config.user_to_utee_ms = 1.5;
    wrapper->config.utee_to_mtee_ms = utee_to_mtee_comm_delay_ms > 0 ? utee_to_mtee_comm_delay_ms : 2.0;
    wrapper->config.utee_request_processing_ms = 0.5;
    wrapper->config.mtee_execution_ms = mtee_execution_delay_ms > 0 ? mtee_execution_delay_ms : 3.0;
    wrapper->config.utee_signature_verification_ms = 1.0;
    wrapper->config.mtee_reveal_processing_ms = 0.5;
    wrapper->config.utee_final_processing_ms = 1.0;
    
    wrapper->config.total_delay_ms = (
        wrapper->config.user_to_utee_ms * 2 +  
        wrapper->config.utee_to_mtee_ms * 3 +  
        wrapper->config.utee_request_processing_ms +
        wrapper->config.mtee_execution_ms +
        wrapper->config.utee_signature_verification_ms +
        wrapper->config.mtee_reveal_processing_ms +
        wrapper->config.utee_final_processing_ms
    );
    
    int base_cores = 16;
    int base_parallel = 300;
    
    if (parallel_degree == 10) {  
        int utee_parallel = base_parallel * (utee_cores / base_cores);
        int mtee_parallel = base_parallel * (mtee_cores / base_cores);
        wrapper->config.parallel_degree = std::min(utee_parallel, mtee_parallel);
    }
    
    wrapper->config.effective_delay_ms = wrapper->config.total_delay_ms / wrapper->config.parallel_degree;
    
    wrapper->max_channels = 1000;
    wrapper->channels = new a402_channel_t[wrapper->max_channels];
    wrapper->channel_count = 0;
    
    return wrapper;
}

void a402_wrapper_cleanup(a402_wrapper_t* wrapper) {
    if (wrapper) {
        delete[] wrapper->channels;
        delete wrapper;
    }
}

bool a402_create_channel(a402_wrapper_t* wrapper, const char* channel_id, double amount) {
    if (!wrapper || !channel_id || wrapper->channel_count >= wrapper->max_channels) {
        return false;
    }
    
    a402_channel_t* channel = &wrapper->channels[wrapper->channel_count++];
    strncpy(channel->channel_id, channel_id, sizeof(channel->channel_id) - 1);
    channel->channel_id[sizeof(channel->channel_id) - 1] = '\0';
    channel->total_amount = amount;
    channel->user_c_amount = amount;
    channel->m_tee_amount = 0.0;
    channel->state = 0;  
    
    return true;
}

bool a402_update_channel(
    a402_wrapper_t* wrapper,
    const char* channel_id,
    double user_c_amount,
    double m_tee_amount
) {
    if (!wrapper || !channel_id) {
        return false;
    }
    
    a402_channel_t* channel = nullptr;
    for (int i = 0; i < wrapper->channel_count; i++) {
        if (strcmp(wrapper->channels[i].channel_id, channel_id) == 0) {
            channel = &wrapper->channels[i];
            break;
        }
    }
    
    if (!channel || channel->state != 0) {
        return false;
    }
    
    if (user_c_amount + m_tee_amount != channel->total_amount) {
        return false;
    }
    
    if (wrapper->config.simulate_realistic_delay) {
        
        double delay_seconds = wrapper->config.effective_delay_ms / 1000.0;
        precise_sleep(delay_seconds);
    }
    
    channel->user_c_amount = user_c_amount;
    channel->m_tee_amount = m_tee_amount;
    
    return true;
}

bool a402_batch_settle(
    a402_wrapper_t* wrapper,
    const char** channel_ids,
    int channel_count
) {
    if (!wrapper || !channel_ids || channel_count <= 0) {
        return false;
    }
    
    if (wrapper->config.simulate_realistic_delay) {
        double delay_per_channel = wrapper->config.effective_delay_ms / channel_count / 1000.0;
        precise_sleep(delay_per_channel);
    }
    
    for (int i = 0; i < channel_count; i++) {
        for (int j = 0; j < wrapper->channel_count; j++) {
            if (strcmp(wrapper->channels[j].channel_id, channel_ids[i]) == 0) {
                wrapper->channels[j].state = 2;  
                break;
            }
        }
    }
    
    return true;
}
