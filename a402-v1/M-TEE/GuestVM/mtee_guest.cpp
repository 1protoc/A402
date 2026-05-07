

#include "mtee_guest.h"
#include "platform_sev.h"
#include "sev_vm_communication.h"
#include "../../Common/crypto/adapter_signature.h"
#include "../../Common/crypto/encryption.h"
#include "../../Common/crypto/key_exchange.h"
#include "../../Common/utils/logger.h"
#include <string.h>
#include <stdlib.h>
#include <stdio.h>


typedef struct {
    uint8_t sk[32];              
    uint8_t pk[33];             
    uint8_t address[20];        
    uint8_t shared_key_sk_m[32]; 
    int initialized;
} mtee_state_t;


mtee_state_t g_mtee_state = {0};


typedef struct {
    uint8_t channel_id[32];
    uint8_t request_data[1024];
    size_t request_len;
    uint64_t payment_amount;
    uint8_t response_data[4096];
    size_t response_len;
    uint8_t secret_t[32];
    uint8_t adapter_point_T[33];
    uint8_t encrypted_response[4096];
    size_t encrypted_len;
    uint8_t tag[16];
    int processed;
} compute_context_t;

#define MAX_COMPUTE_CONTEXTS 100
static compute_context_t compute_contexts[MAX_COMPUTE_CONTEXTS];
static int context_count = 0;


static compute_context_t* find_context(const uint8_t* channel_id) {
    for (int i = 0; i < context_count; i++) {
        if (memcmp(compute_contexts[i].channel_id, channel_id, 32) == 0 && 
            !compute_contexts[i].processed) {
            return &compute_contexts[i];
        }
    }
    return NULL;
}


static int send_to_utee_via_host(const uint8_t* data, size_t len) {
    uint32_t utee_vm_id = 1; 
    return sev_vm_comm_send(utee_vm_id, data, len);
}

int mtee_guest_init(const uint8_t* sk, const uint8_t* pk) {
    if (!sk || !pk) {
        return -1;
    }
    
    
    if (platform_sev_init() != 0) {
        return -1;
    }
    
    
    uint32_t mtee_vm_id = 2; 
    if (sev_vm_comm_init(mtee_vm_id, SEV_COMM_CHANNEL_HOST_MEDIATED) != 0) {
        return -1;
    }
    
    memcpy(g_mtee_state.sk, sk, 32);
    memcpy(g_mtee_state.pk, pk, 33);
    g_mtee_state.initialized = 1;
    
    
    
    
    uint8_t u_tee_pubkey[33] = {0x02}; 
    if (compute_shared_secret(sk, u_tee_pubkey, g_mtee_state.shared_key_sk_m) != 0) {
        
        memset(g_mtee_state.shared_key_sk_m, 0x42, 32);
    }
    
    LOG_INFO("M-TEE Guest VM initialized");
    return 0;
}

int mtee_execute_computation(
    const uint8_t* request_data,
    size_t request_len,
    uint8_t* response_data,
    size_t* response_len)
{
    if (!request_data || !response_data || !response_len) {
        return -1;
    }
    
    
    
    
    
    LOG_INFO("Executing computation: request_len=%zu", request_len);
    
    
    char result[4096];
    snprintf(result, sizeof(result), "Computation result for request (len=%zu): ", request_len);
    size_t result_len = strlen(result);
    
    
    uint8_t hash[32];
    platform_sha256(request_data, request_len < 1024 ? request_len : 1024, hash);
    for (int i = 0; i < 8 && result_len < sizeof(result) - 3; i++) {
        result_len += snprintf(result + result_len, sizeof(result) - result_len, "%02x", hash[i]);
    }
    
    if (result_len > 4096) {
        result_len = 4096;
    }
    
    memcpy(response_data, result, result_len);
    *response_len = result_len;
    
    LOG_INFO("Computation completed: response_len=%zu", result_len);
    
    return 0;
}

int mtee_handle_compute_request(
    const uint8_t* channel_id,
    const uint8_t* request_data,
    size_t request_len,
    uint64_t payment_amount)
{
    if (!g_mtee_state.initialized || !channel_id || !request_data) {
        return -1;
    }
    
    if (context_count >= MAX_COMPUTE_CONTEXTS) {
        return -1;
    }
    
    compute_context_t* ctx = &compute_contexts[context_count++];
    memcpy(ctx->channel_id, channel_id, 32);
    memcpy(ctx->request_data, request_data, request_len);
    ctx->request_len = request_len;
    ctx->payment_amount = payment_amount;
    ctx->processed = 0;
    
    
    if (mtee_execute_computation(
        request_data, request_len,
        ctx->response_data, &ctx->response_len) != 0) {
        return -1;
    }
    
    
    adapter_signature_ctx_t adapter_ctx;
    
    
    uint8_t tx_pay[256];
    size_t tx_pay_len = snprintf((char*)tx_pay, sizeof(tx_pay),
        "channel_id=%.32s,amount=%llu", channel_id, payment_amount);
    
    if (generate_adapter_signature(
        g_mtee_state.sk,
        tx_pay, tx_pay_len,
        &adapter_ctx) != 0) {
        LOG_ERROR("Failed to generate adapter signature");
        return -1;
    }
    
    LOG_INFO("Adapter signature generated: channel_id=%02x%02x...", 
             channel_id[0], channel_id[1]);
    
    
    memcpy(ctx->secret_t, adapter_ctx.secret_t, 32);
    memcpy(ctx->adapter_point_T, adapter_ctx.adapter_point_T, 33);
    
    
    uint8_t iv[16];
    if (platform_get_random(iv, 16) != 0) {
        LOG_ERROR("Failed to generate IV");
        return -1;
    }
    
    int encrypted_len = encrypt_data(
        ctx->response_data, ctx->response_len,
        ctx->secret_t, iv,
        ctx->encrypted_response, sizeof(ctx->encrypted_response),
        ctx->tag
    );
    
    if (encrypted_len < 0) {
        LOG_ERROR("Failed to encrypt response");
        return -1;
    }
    
    ctx->encrypted_len = encrypted_len;
    LOG_INFO("Response encrypted: channel_id=%02x%02x..., len=%d", 
             channel_id[0], channel_id[1], encrypted_len);
    
    
    compute_response_msg_t response_msg;
    memcpy(response_msg.channel_id, channel_id, 32);
    memcpy(response_msg.encrypted_response, ctx->encrypted_response, ctx->encrypted_len);
    response_msg.encrypted_len = ctx->encrypted_len;
    memcpy(response_msg.tx_pay, tx_pay, tx_pay_len);
    response_msg.tx_pay_len = tx_pay_len;
    memcpy(response_msg.adapter_point_T, ctx->adapter_point_T, 33);
    memcpy(response_msg.tag, ctx->tag, 16);
    
    
    uint8_t encrypted[4096];
    uint8_t tag[16];
    int msg_encrypted_len = encrypt_with_shared_key(
        (uint8_t*)&response_msg, sizeof(response_msg),
        g_mtee_state.shared_key_sk_m,
        encrypted, sizeof(encrypted),
        tag
    );
    
    if (msg_encrypted_len < 0) {
        return -1;
    }
    
    if (send_to_utee_via_host(encrypted, msg_encrypted_len) != 0) {
        LOG_ERROR("Failed to send compute response to U-TEE");
        return -1;
    }
    
    LOG_INFO("Compute response sent to U-TEE: channel_id=%02x%02x...", 
             channel_id[0], channel_id[1]);
    
    return 0;
}

int mtee_handle_signed_tx_pay(
    const uint8_t* channel_id,
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    const uint8_t* signature,
    const uint8_t* u_tee_pubkey)
{
    if (!g_mtee_state.initialized || !channel_id || !tx_pay || !signature || !u_tee_pubkey) {
        return -1;
    }
    
    
    if (platform_verify_signature(tx_pay, tx_pay_len, u_tee_pubkey, signature) != 0) {
        LOG_ERROR("Invalid U-TEE signature");
        return -1;
    }
    
    LOG_INFO("U-TEE signature verified: channel_id=%02x%02x...", 
             channel_id[0], channel_id[1]);
    
    compute_context_t* ctx = find_context(channel_id);
    if (!ctx) {
        LOG_ERROR("Compute context not found");
        return -1;
    }
    
    
    uint8_t tx_data[1024];
    size_t tx_len;
    
    
    
    
    memcpy(tx_data, tx_pay, tx_pay_len);
    memcpy(tx_data + tx_pay_len, ctx->adapter_point_T, 33);
    memcpy(tx_data + tx_pay_len + 33, ctx->secret_t, 32);
    tx_len = tx_pay_len + 33 + 32;
    
    
    
    
    ctx->processed = 1;
    
    LOG_INFO("Transaction generated: channel_id=%02x%02x..., tx_len=%zu", 
             channel_id[0], channel_id[1], tx_len);
    
    return 0;
}


int mtee_reveal_secret_to_utee(const uint8_t* channel_id) {
    if (!g_mtee_state.initialized || !channel_id) {
        return -1;
    }
    
    compute_context_t* ctx = find_context(channel_id);
    if (!ctx) {
        return -1;
    }
    
    
    uint8_t encrypted_secret[48];
    uint8_t tag[16];
    
    int encrypted_len = encrypt_with_shared_key(
        ctx->secret_t, 32,
        g_mtee_state.shared_key_sk_m,
        encrypted_secret, sizeof(encrypted_secret),
        tag
    );
    
    if (encrypted_len < 0) {
        return -1;
    }
    
    
    reveal_secret_msg_t reveal_msg;
    memcpy(reveal_msg.channel_id, channel_id, 32);
    memcpy(reveal_msg.encrypted_secret_t, encrypted_secret, encrypted_len);
    reveal_msg.encrypted_len = encrypted_len;
    memcpy(reveal_msg.tag, tag, 16);
    
    
    uint8_t final_encrypted[4096];
    uint8_t final_tag[16];
    int final_len = encrypt_with_shared_key(
        (uint8_t*)&reveal_msg, sizeof(reveal_msg),
        g_mtee_state.shared_key_sk_m,
        final_encrypted, sizeof(final_encrypted),
        final_tag
    );
    
    if (final_len < 0) {
        return -1;
    }
    
    if (send_to_utee_via_host(final_encrypted, final_len) != 0) {
        LOG_ERROR("Failed to send revealed secret to U-TEE");
        return -1;
    }
    
    LOG_INFO("Secret revealed to U-TEE: channel_id=%02x%02x...", 
             channel_id[0], channel_id[1]);
    
    return 0;
}

