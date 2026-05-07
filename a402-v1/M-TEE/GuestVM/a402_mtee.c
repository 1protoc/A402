

#include "mtee_guest.h"
#include "platform_sev.h"
#include "../../Common/protocol/a402_protocol.h"
#include "../../Common/protocol/messages.h"
#include "../../Common/crypto/commitment.h"
#include "../../Common/crypto/adapter_signature.h"
#include "../../Common/crypto/encryption.h"
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

extern mtee_state_t g_mtee_state;


typedef struct {
    uint8_t cid[32];
    uint8_t rid[32];
    uint8_t secret_t[32];
    int valid;
} stored_secret_t;

#define MAX_STORED_SECRETS 100
static stored_secret_t stored_secrets[MAX_STORED_SECRETS];
static int stored_secret_count = 0;

static stored_secret_t* find_stored_secret(const uint8_t* cid, const uint8_t* rid) {
    for (int i = 0; i < stored_secret_count; i++) {
        if (stored_secrets[i].valid &&
            memcmp(stored_secrets[i].cid, cid, 32) == 0 &&
            memcmp(stored_secrets[i].rid, rid, 32) == 0) {
            return &stored_secrets[i];
        }
    }
    return NULL;
}


int register_server(const register_server_input_t* input, register_server_output_t* output) {
    if (!input || !output) {
        return -1;
    }
    
    
    uint8_t commit_data[32 + 33 + 32 + 512];
    memcpy(commit_data, input->sid, 32);
    memcpy(commit_data + 32, input->pk_S, 33);
    memcpy(commit_data + 65, input->codehash_S, 32);
    memcpy(commit_data + 97, input->att_S, input->att_S_len < 512 ? input->att_S_len : 512);
    
    platform_sha256(commit_data, 97 + (input->att_S_len < 512 ? input->att_S_len : 512), output->H_S);
    
    
    LOG_INFO("Server registered: sid=%02x%02x..., H_S=%02x%02x...",
             input->sid[0], input->sid[1], output->H_S[0], output->H_S[1]);
    
    return 0;
}


int exec_request(const exec_request_input_t* input, exec_request_output_t* output) {
    if (!input || !output) {
        return -1;
    }
    
    
    uint8_t response_data[4096];
    size_t response_len = sizeof(response_data);
    
    
    const char* result = "Computation result";
    response_len = strlen(result);
    memcpy(response_data, result, response_len);
    
    
    adapter_signature_ctx_t adapter_ctx;
    
    
    uint8_t sign_msg[1024];
    size_t sign_msg_len = snprintf((char*)sign_msg, sizeof(sign_msg),
        "exec:cid=%.32s,rid=%.32s,delta=%lu", input->cid, input->rid, (unsigned long)input->delta);
    
    if (generate_adapter_signature(
            g_mtee_state.sk,
            sign_msg,
            sign_msg_len,
            &adapter_ctx) != 0) {
        LOG_ERROR("Failed to generate adapter signature");
        return -1;
    }
    
    
    uint8_t iv[16];
    if (platform_get_random(iv, 16) != 0) {
        LOG_ERROR("Failed to generate IV");
        return -1;
    }
    
    uint8_t tag[16];
    int encrypted_len = encrypt_data(
        response_data,
        response_len,
        adapter_ctx.secret_t,
        iv,
        output->EncRes,
        sizeof(output->EncRes),
        tag
    );
    
    if (encrypted_len < 0) {
        LOG_ERROR("Failed to encrypt response");
        return -1;
    }
    
    output->EncRes_len = encrypted_len;
    
    
    memcpy(output->hat_sigma_S, adapter_ctx.signature, 64);
    memcpy(output->T, adapter_ctx.adapter_point_T, 33);
    
    
    if (stored_secret_count >= MAX_STORED_SECRETS) {
        LOG_ERROR("Secret storage full");
        return -1;
    }
    stored_secret_t* stored = &stored_secrets[stored_secret_count++];
    memcpy(stored->cid, input->cid, 32);
    memcpy(stored->rid, input->rid, 32);
    memcpy(stored->secret_t, adapter_ctx.secret_t, 32);
    stored->valid = 1;
    
    LOG_INFO("Request executed: cid=%02x%02x..., rid=%02x%02x..., delta=%llu",
             input->cid[0], input->cid[1], input->rid[0], input->rid[1], input->delta);
    
    return 0;
}


int reveal_secret(const reveal_secret_input_t* input) {
    if (!input) {
        return -1;
    }
    
    
    stored_secret_t* stored = find_stored_secret(input->cid, input->rid);
    if (!stored) {
        LOG_ERROR("Secret not found for cid=%02x%02x..., rid=%02x%02x...",
                  input->cid[0], input->cid[1], input->rid[0], input->rid[1]);
        return -1;
    }
    
    
    if (memcmp(stored->secret_t, input->t, 32) != 0) {
        LOG_ERROR("Secret mismatch");
        return -1;
    }
    
    
    uint8_t encrypted_secret[48];
    uint8_t tag[16];
    uint8_t iv[16];
    if (platform_get_random(iv, 16) != 0) {
        LOG_ERROR("Failed to generate IV");
        return -1;
    }
    
    int encrypted_len = encrypt_data(
        input->t,
        32,
        g_mtee_state.shared_key_sk_m,
        iv,
        encrypted_secret,
        sizeof(encrypted_secret),
        tag
    );
    
    if (encrypted_len < 0) {
        LOG_ERROR("Failed to encrypt secret");
        return -1;
    }
    
    
    
    reveal_secret_msg_t reveal_msg;
    memcpy(reveal_msg.channel_id, input->cid, 32);
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
        LOG_ERROR("Failed to encrypt reveal message");
        return -1;
    }
    
    
    
    
    
    
    
    stored->valid = 0;
    
    LOG_INFO("Secret revealed to U-TEE: cid=%02x%02x..., rid=%02x%02x...",
             input->cid[0], input->cid[1], input->rid[0], input->rid[1]);
    
    return 0;
}


int s_batch_settlement(const s_batch_settlement_input_t* input, s_batch_settlement_output_t* output) {
    if (!input || !output) {
        return -1;
    }
    
    
    
    uint64_t total_settle = 0;
    
    
    
    
    uint8_t tx_data[512];
    size_t tx_len = snprintf((char*)tx_data, sizeof(tx_data),
        "s_batch_settlement:s=%.20s,count=%d,total=%lu",
        input->s_address, input->cid_count, (unsigned long)total_settle);
    
    if (tx_len >= sizeof(output->tx_batch)) {
        tx_len = sizeof(output->tx_batch) - 1;
    }
    memcpy(output->tx_batch, tx_data, tx_len);
    output->tx_batch_len = tx_len;
    
    LOG_INFO("S batch settlement: s=%02x%02x..., count=%d, total=%llu",
             input->s_address[0], input->s_address[1], input->cid_count, total_settle);
    
    return 0;
}
