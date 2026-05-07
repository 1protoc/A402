#include "a402_wrapper.h"
#include <cstdlib>
#include <cstring>
#include <cmath>
#include <thread>
#include <chrono>
#include <mutex>
#include <vector>
#include <algorithm>
#include <random>
#include <cstdio>
#include <unordered_map>
#include <array>






struct ChannelState {
    char channel_id[64];
    double total_amount;
    double locked_amount;
    double paid_amount;
    double user_c_amount;  
    double m_tee_amount;   
    int state;  
    uint64_t nonce;
};


struct RequestState {
    char channel_id[64];
    char request_id[64];
    double amount;
    uint8_t encrypted_response[4096];
    size_t encrypted_response_len;
    uint8_t adapter_signature[64];
    uint8_t adapter_point_T[33];
    uint8_t secret_t[32];
    bool secret_revealed;
};


class A402ProtocolImpl {
private:
    
    static constexpr int NUM_SHARDS = 128;
    
    struct Shard {
        std::unordered_map<std::string, ChannelState> channels;
        std::mutex mutex;
    };
    
    std::array<Shard, NUM_SHARDS> shards_;
    std::vector<RequestState> requests_;
    std::mutex requests_mutex_;  
    
    
    
    int get_shard_index(const char* channel_id) {
        uint32_t hash = 0;
        for (int i = 0; channel_id[i] != '\0' && i < 64; i++) {
            hash = hash * 31 + (unsigned char)channel_id[i];
        }
        return hash % NUM_SHARDS;
    }
    
    
    Shard& get_shard(const char* channel_id) {
        return shards_[get_shard_index(channel_id)];
    }
    
    
    double user_to_utee_ms_;
    double utee_to_mtee_ms_;
    double utee_request_processing_ms_;
    double mtee_execution_ms_;
    double utee_signature_verification_ms_;
    double mtee_reveal_processing_ms_;
    double utee_final_processing_ms_;
    
    void precise_sleep(double seconds) {
        if (seconds <= 0.0) return;
        auto start = std::chrono::high_resolution_clock::now();
        auto end = start + std::chrono::duration<double>(seconds);
        while (std::chrono::high_resolution_clock::now() < end) {
            std::this_thread::yield();
        }
    }
    
    
    void simulate_network_delay(double ms) {
        if (ms > 0) {
            precise_sleep(ms / 1000.0);
        }
    }
    
    
    void simulate_processing_delay(double ms) {
        if (ms > 0) {
            precise_sleep(ms / 1000.0);
        }
    }
    
    
    void generate_request_id(const char* channel_id, char* request_id) {
        snprintf(request_id, 64, "%s_req_%lu", channel_id, 
                 std::chrono::duration_cast<std::chrono::microseconds>(
                     std::chrono::high_resolution_clock::now().time_since_epoch()
                 ).count());
    }
    
    
    void generate_adapter_signature(uint8_t* signature, uint8_t* adapter_point_T) {
        
        
        std::random_device rd;
        std::mt19937 gen(rd());
        
        for (int i = 0; i < 64; i++) {
            signature[i] = gen() % 256;
        }
        for (int i = 0; i < 33; i++) {
            adapter_point_T[i] = gen() % 256;
        }
    }
    
    
    void encrypt_response(const uint8_t* data, size_t data_len, 
                          const uint8_t* secret_t, 
                          uint8_t* encrypted, size_t* encrypted_len) {
        
        *encrypted_len = data_len;
        for (size_t i = 0; i < data_len && i < 4096; i++) {
            encrypted[i] = data[i] ^ secret_t[i % 32];
        }
    }
    
    
    void decrypt_response(const uint8_t* encrypted, size_t encrypted_len,
                          const uint8_t* secret_t,
                          uint8_t* decrypted, size_t* decrypted_len) {
        
        *decrypted_len = encrypted_len;
        for (size_t i = 0; i < encrypted_len && i < 4096; i++) {
            decrypted[i] = encrypted[i] ^ secret_t[i % 32];
        }
    }
    
public:
    A402ProtocolImpl(double user_to_utee_ms = 1.5,
                     double utee_to_mtee_ms = 2.0,
                     double utee_request_processing_ms = 0.5,
                     double mtee_execution_ms = 3.0,
                     double utee_signature_verification_ms = 1.0,
                     double mtee_reveal_processing_ms = 0.5,
                     double utee_final_processing_ms = 1.0)
        : user_to_utee_ms_(user_to_utee_ms)
        , utee_to_mtee_ms_(utee_to_mtee_ms)
        , utee_request_processing_ms_(utee_request_processing_ms)
        , mtee_execution_ms_(mtee_execution_ms)
        , utee_signature_verification_ms_(utee_signature_verification_ms)
        , mtee_reveal_processing_ms_(mtee_reveal_processing_ms)
        , utee_final_processing_ms_(utee_final_processing_ms)
    {}
    
    
    bool create_channel(const char* channel_id, double amount) {
        Shard& shard = get_shard(channel_id);
        std::lock_guard<std::mutex> lock(shard.mutex);
        
        
        if (shard.channels.find(channel_id) != shard.channels.end()) {
            return false;
        }
        
        ChannelState channel;
        strncpy(channel.channel_id, channel_id, sizeof(channel.channel_id) - 1);
        channel.channel_id[sizeof(channel.channel_id) - 1] = '\0';
        channel.total_amount = amount;
        channel.locked_amount = 0.0;
        channel.paid_amount = 0.0;
        channel.user_c_amount = amount;
        channel.m_tee_amount = 0.0;
        channel.state = 0;  
        channel.nonce = 0;
        
        shard.channels[channel_id] = channel;
        return true;
    }
    
    
    bool execute_request(const char* channel_id, double amount, bool simulate_delay) {
        
        if (simulate_delay) {
            simulate_network_delay(user_to_utee_ms_);
        }
        
        ChannelState* channel = nullptr;
        {
            Shard& shard = get_shard(channel_id);
            std::lock_guard<std::mutex> lock(shard.mutex);
            auto it = shard.channels.find(channel_id);
            if (it != shard.channels.end()) {
                channel = &it->second;
            }
        }
        
        if (!channel || channel->state == 2) {
            return false;
        }
        
        
        if (simulate_delay) {
            simulate_processing_delay(utee_request_processing_ms_);
        }
        
        
        {
            Shard& shard = get_shard(channel_id);
            std::lock_guard<std::mutex> lock(shard.mutex);
            if (channel->total_amount - channel->locked_amount < amount) {
                return false;
            }
            channel->locked_amount += amount;
            if (channel->state == 0) {
                channel->state = 1;  
            }
        }
        
        
        RequestState request;
        strncpy(request.channel_id, channel_id, sizeof(request.channel_id) - 1);
        generate_request_id(channel_id, request.request_id);
        request.amount = amount;
        request.secret_revealed = false;
        
        
        if (simulate_delay) {
            simulate_network_delay(utee_to_mtee_ms_);
        }
        
        
        if (simulate_delay) {
            simulate_processing_delay(mtee_execution_ms_);
        }
        
        
        generate_adapter_signature(request.adapter_signature, request.adapter_point_T);
        
        
        std::random_device rd;
        std::mt19937 gen(rd());
        for (int i = 0; i < 32; i++) {
            request.secret_t[i] = gen() % 256;
        }
        
        
        const char* response_data = "Computation result";
        encrypt_response((const uint8_t*)response_data, strlen(response_data),
                        request.secret_t, request.encrypted_response,
                        &request.encrypted_response_len);
        
        
        if (simulate_delay) {
            simulate_network_delay(utee_to_mtee_ms_);
        }
        
        
        if (simulate_delay) {
            simulate_processing_delay(utee_signature_verification_ms_);
        }
        
        
        {
            std::lock_guard<std::mutex> lock(requests_mutex_);
            requests_.push_back(request);
        }
        
        
        if (simulate_delay) {
            simulate_network_delay(utee_to_mtee_ms_);
        }
        
        
        
        if (simulate_delay) {
            simulate_processing_delay(mtee_reveal_processing_ms_);
        }
        
        
        if (simulate_delay) {
            simulate_network_delay(utee_to_mtee_ms_);
        }
        
        
        if (simulate_delay) {
            simulate_processing_delay(utee_final_processing_ms_);
        }
        
        
        uint8_t decrypted[4096];
        size_t decrypted_len;
        decrypt_response(request.encrypted_response, request.encrypted_response_len,
                        request.secret_t, decrypted, &decrypted_len);
        
        
        {
            Shard& shard = get_shard(channel_id);
            std::lock_guard<std::mutex> lock(shard.mutex);
            channel->locked_amount -= amount;
            channel->paid_amount += amount;
            if (channel->locked_amount == 0) {
                channel->state = 0;  
            }
        }
        
        
        if (simulate_delay) {
            simulate_network_delay(user_to_utee_ms_);
        }
        
        return true;
    }
    
    
    bool update_channel(const char* channel_id, double user_c_amount, double m_tee_amount, bool simulate_delay) {
        
        
        
        
        ChannelState* channel = nullptr;
        {
            Shard& shard = get_shard(channel_id);
            std::lock_guard<std::mutex> lock(shard.mutex);
            auto it = shard.channels.find(channel_id);
            if (it != shard.channels.end()) {
                channel = &it->second;
            }
        }
        
        if (!channel || channel->state == 2) {  
            return false;
        }
        
        
        if (user_c_amount + m_tee_amount != channel->total_amount) {
            return false;
        }
        
        double payment_amount = channel->total_amount - user_c_amount;  
        
        
        
        
        
        
        
        {
            Shard& shard = get_shard(channel_id);
            std::lock_guard<std::mutex> lock(shard.mutex);
            if (channel->total_amount - channel->locked_amount < payment_amount) {
                return false;
            }
            channel->locked_amount += payment_amount;
            if (channel->state == 0) {
                channel->state = 1;  
            }
        }
        
        
        
        
        
        if (simulate_delay || utee_to_mtee_ms_ > 0) {
            simulate_network_delay(utee_to_mtee_ms_);
        }
        
        
        
        
        
        if (simulate_delay || mtee_execution_ms_ > 0) {
            simulate_processing_delay(mtee_execution_ms_);
        }
        
        
        const char* response_data = "Computation result";
        size_t response_len = strlen(response_data);
        
        
        RequestState request;
        strncpy(request.channel_id, channel_id, sizeof(request.channel_id) - 1);
        generate_request_id(channel_id, request.request_id);
        request.amount = payment_amount;
        request.secret_revealed = false;
        
        
        std::random_device rd;
        std::mt19937 gen(rd());
        for (int i = 0; i < 32; i++) {
            request.secret_t[i] = gen() % 256;
        }
        
        
        generate_adapter_signature(request.adapter_signature, request.adapter_point_T);
        
        
        encrypt_response((const uint8_t*)response_data, response_len,
                        request.secret_t, request.encrypted_response,
                        &request.encrypted_response_len);
        
        
        uint8_t tx_pay[256];
        size_t tx_pay_len = snprintf((char*)tx_pay, sizeof(tx_pay),
                                     "tx_pay:cid=%s,amount=%f", channel_id, payment_amount);
        
        
        
        if (simulate_delay || utee_to_mtee_ms_ > 0) {
            simulate_network_delay(utee_to_mtee_ms_);
        }
        
        
        
        
        
        
        
        
        uint8_t signed_tx_pay[320];
        size_t signed_tx_pay_len = tx_pay_len + 64;  
        memcpy(signed_tx_pay, tx_pay, tx_pay_len);
        for (size_t i = tx_pay_len; i < signed_tx_pay_len; i++) {
            signed_tx_pay[i] = gen() % 256;
        }
        
        
        {
            std::lock_guard<std::mutex> lock(requests_mutex_);
            requests_.push_back(request);
        }
        
        
        
        if (simulate_delay || utee_to_mtee_ms_ > 0) {
            simulate_network_delay(utee_to_mtee_ms_);
        }
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        if (simulate_delay || mtee_reveal_processing_ms_ > 0) {
            simulate_processing_delay(mtee_reveal_processing_ms_);
        }
        
        
        uint8_t encrypted_secret_t[48];
        size_t encrypted_secret_t_len;
        encrypt_response(request.secret_t, 32,
                        request.secret_t,  
                        encrypted_secret_t, &encrypted_secret_t_len);
        
        
        if (simulate_delay || utee_to_mtee_ms_ > 0) {
            simulate_network_delay(utee_to_mtee_ms_);
        }
        
        
        
        
        
        
        
        uint8_t decrypted_t[32];
        size_t decrypted_t_len;
        decrypt_response(encrypted_secret_t, encrypted_secret_t_len,
                        request.secret_t, decrypted_t, &decrypted_t_len);
        
        
        uint8_t decrypted_response[4096];
        size_t decrypted_response_len;
        decrypt_response(request.encrypted_response, request.encrypted_response_len,
                        decrypted_t, decrypted_response, &decrypted_response_len);
        
        
        {
            Shard& shard = get_shard(channel_id);
            std::lock_guard<std::mutex> lock(shard.mutex);
            channel->locked_amount -= payment_amount;
            channel->paid_amount += payment_amount;
            channel->user_c_amount = user_c_amount;
            channel->m_tee_amount = m_tee_amount;
            if (channel->locked_amount == 0) {
                channel->state = 0;  
            }
        }
        
        
        
        
        return true;
    }
};


static A402ProtocolImpl* g_a402_impl = nullptr;


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
    wrapper->config.mtee_execution_ms = mtee_execution_delay_ms > 0 ? mtee_execution_delay_ms : 0.5;
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
    
    
    g_a402_impl = new A402ProtocolImpl(
        wrapper->config.user_to_utee_ms,
        wrapper->config.utee_to_mtee_ms,
        wrapper->config.utee_request_processing_ms,
        wrapper->config.mtee_execution_ms,
        wrapper->config.utee_signature_verification_ms,
        wrapper->config.mtee_reveal_processing_ms,
        wrapper->config.utee_final_processing_ms
    );
    
    return wrapper;
}


void a402_wrapper_cleanup(a402_wrapper_t* wrapper) {
    if (wrapper) {
        delete[] wrapper->channels;
        delete wrapper;
    }
    if (g_a402_impl) {
        delete g_a402_impl;
        g_a402_impl = nullptr;
    }
}


bool a402_create_channel(a402_wrapper_t* wrapper, const char* channel_id, double amount) {
    if (!wrapper || !channel_id || wrapper->channel_count >= wrapper->max_channels) {
        return false;
    }
    
    
    if (g_a402_impl && !g_a402_impl->create_channel(channel_id, amount)) {
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
    if (!wrapper || !channel_id || !g_a402_impl) {
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
    
    
    bool success = g_a402_impl->update_channel(
        channel_id,
        user_c_amount,
        m_tee_amount,
        wrapper->config.simulate_realistic_delay
    );
    
    if (success) {
        
        channel->user_c_amount = user_c_amount;
        channel->m_tee_amount = m_tee_amount;
    }
    
    return success;
}


bool a402_batch_settle(
    a402_wrapper_t* wrapper,
    const char** channel_ids,
    int channel_count
) {
    if (!wrapper || !channel_ids || channel_count <= 0) {
        return false;
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
