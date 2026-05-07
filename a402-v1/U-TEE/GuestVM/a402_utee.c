

#include "utee_guest.h"
#include "platform_sev.h"
#include "../../Common/crypto/commitment.h"
#include "../../Common/crypto/adapter_signature.h"
#include "../../Common/crypto/encryption.h"
#include "../../Common/storage/response_storage.h"
#include "../../Common/vault/vault.h"
#include "../../Common/utils/logger.h"
#include "../../Common/utils/timelock.h"
#include "../../Common/mixer/receipt.h"
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <inttypes.h>


typedef struct {
    uint8_t sk[32];
    uint8_t pk[33];
    uint8_t address[20];
    uint8_t shared_key_sk_m[32];
    int initialized;
} utee_state_t;

extern utee_state_t g_utee_state;
#define MAX_CHANNELS 100
extern payment_channel_t channels[MAX_CHANNELS];
extern int channel_count;


int register_uvault(const register_uvault_input_t* input, register_uvault_output_t* output) {
    if (!input || !output) {
        return -1;
    }
    
    
    if (generate_commitment(
            input->pk_U,
            input->codehash_U,
            input->att_U,
            input->att_U_len,
            output->H_U) != 0) {
        LOG_ERROR("Failed to generate commitment for U-Vault");
        return -1;
    }
    
    
    
    LOG_INFO("U-Vault registered: H_U=%02x%02x...", output->H_U[0], output->H_U[1]);
    
    return 0;
}



int create_asc(const create_asc_input_t* input, create_asc_output_t* output) {
    if (!input || !output) {
        return -1;
    }
    
    if (input->mode != 0) {
        LOG_ERROR("create_asc called with wrong mode, use create_asc_privacy for mode 1-B");
        return -1;
    }
    
    
    
    
    
    uint8_t user_address[20] = {0}; 
    uint64_t amount = 0; 
    
    
    if (channel_count >= MAX_CHANNELS) {
        LOG_ERROR("Channel limit reached");
        return -1;
    }
    
    payment_channel_t* channel = &channels[channel_count++];
    
    
    if (platform_get_random(channel->channel_id, 32) != 0) {
        return -1;
    }
    
    memcpy(channel->u_tee_address, g_utee_state.address, 20);
    memcpy(channel->m_tee_address, input->S_address, 20);
    
    channel->total_amount = amount;
    channel->locked_amount = 0;
    channel->paid_amount = 0;
    channel->state = CHANNEL_STATE_OPEN;
    channel->nonce = 0;
    
    
    if (vault_bind_to_channel(user_address, amount) != 0) {
        LOG_ERROR("Failed to bind assets to channel");
        return -1;
    }
    
    memcpy(output->cid, channel->channel_id, 32);
    
    
    uint8_t gamma_data[128];
    memcpy(gamma_data, channel->channel_id, 32);
    memcpy(gamma_data + 32, channel->u_tee_address, 20);
    memcpy(gamma_data + 52, channel->m_tee_address, 20);
    memcpy(gamma_data + 72, &channel->total_amount, 8);
    memcpy(gamma_data + 80, &channel->locked_amount, 8);
    memcpy(gamma_data + 88, &channel->paid_amount, 8);
    gamma_data[96] = (uint8_t)channel->state;
    
    memcpy(output->Gamma_cid, gamma_data, 97);
    output->Gamma_cid_len = 97;
    
    
    output->interfaces_len = 0;
    output->min_amount = 0;
    output->max_amount = 0;
    
    LOG_INFO("ASC created (mode 1-A): cid=%02x%02x..., amount=%llu", 
             output->cid[0], output->cid[1], amount);
    
    return 0;
}


int create_asc_privacy(const create_asc_privacy_input_t* input, create_asc_output_t* output) {
    if (!input || !output) {
        return -1;
    }
    
    
    uint64_t free_balance = 0, locked_balance = 0;
    if (vault_get_balance(input->user_address, &free_balance, &locked_balance) != 0) {
        LOG_ERROR("Failed to get vault balance");
        return -1;
    }
    
    if (free_balance < input->amount) {
        LOG_ERROR("Insufficient free balance");
        return -1;
    }
    
    
    if (channel_count >= MAX_CHANNELS) {
        LOG_ERROR("Channel limit reached");
        return -1;
    }
    
    payment_channel_t* channel = &channels[channel_count++];
    
    
    if (platform_get_random(channel->channel_id, 32) != 0) {
        return -1;
    }
    
    memcpy(channel->u_tee_address, g_utee_state.address, 20);
    memcpy(channel->m_tee_address, input->S_address, 20);
    
    
    if (vault_bind_to_channel(input->user_address, input->amount) != 0) {
        LOG_ERROR("Failed to bind assets to channel");
        return -1;
    }
    
    channel->total_amount = input->amount;
    channel->locked_amount = 0;
    channel->paid_amount = 0;
    channel->state = CHANNEL_STATE_OPEN;
    channel->nonce = 0;
    
    memcpy(output->cid, channel->channel_id, 32);
    
    
    uint8_t gamma_data[128];
    memcpy(gamma_data, channel->channel_id, 32);
    memcpy(gamma_data + 32, channel->u_tee_address, 20);
    memcpy(gamma_data + 52, channel->m_tee_address, 20);
    memcpy(gamma_data + 72, &channel->total_amount, 8);
    memcpy(gamma_data + 80, &channel->locked_amount, 8);
    memcpy(gamma_data + 88, &channel->paid_amount, 8);
    gamma_data[96] = (uint8_t)channel->state;
    
    memcpy(output->Gamma_cid, gamma_data, 97);
    output->Gamma_cid_len = 97;
    
    
    const char* interfaces = "compute,storage,network"; 
    size_t interfaces_str_len = strlen(interfaces);
    if (interfaces_str_len < sizeof(output->accepted_interfaces)) {
        memcpy(output->accepted_interfaces, interfaces, interfaces_str_len);
        output->interfaces_len = interfaces_str_len;
    }
    output->min_amount = 1000;  
    output->max_amount = 1000000; 
    
    LOG_INFO("ASC created (mode 1-B): cid=%02x%02x..., amount=%llu", 
             output->cid[0], output->cid[1], input->amount);
    
    return 0;
}


int send_request(const send_request_input_t* input, send_request_output_t* output) {
    if (!input || !output) {
        return -1;
    }
    
    
    payment_channel_t* channel = NULL;
    for (int i = 0; i < channel_count; i++) {
        if (memcmp(channels[i].channel_id, input->cid, 32) == 0) {
            channel = &channels[i];
            break;
        }
    }
    
    if (!channel) {
        LOG_ERROR("Channel not found");
        return -1;
    }
    
    
    if (channel->total_amount - channel->locked_amount < input->a) {
        LOG_ERROR("Insufficient free assets");
        return -1;
    }
    
    channel->locked_amount += input->a;
    if (channel->state == CHANNEL_STATE_OPEN) {
        channel->state = CHANNEL_STATE_LOCKED;
    }
    
    
    uint8_t rid_data[64];
    memcpy(rid_data, input->cid, 32);
    memcpy(rid_data + 32, input->req, input->req_len < 32 ? input->req_len : 32);
    platform_sha256(rid_data, 32 + (input->req_len < 32 ? input->req_len : 32), output->rid);
    
    
    if (timelock_record_request(input->cid, output->rid, input->a, DEFAULT_TIMELOCK_SECONDS) != 0) {
        LOG_ERROR("Failed to record timelock request");
        return -1;
    }
    
    
    
    exec_request_input_t exec_input;
    memcpy(exec_input.cid, input->cid, 32);
    memcpy(exec_input.rid, output->rid, 32);
    memcpy(exec_input.req, input->req, input->req_len);
    exec_input.req_len = input->req_len;
    exec_input.delta = input->a;
    
    
    uint8_t encrypted[4096];
    uint8_t tag[16];
    int encrypted_len = encrypt_with_shared_key(
        (uint8_t*)&exec_input, sizeof(exec_input),
        g_utee_state.shared_key_sk_m,
        encrypted, sizeof(encrypted),
        tag
    );
    
    if (encrypted_len < 0) {
        LOG_ERROR("Failed to encrypt request");
        
        timelock_clear_request(input->cid, output->rid);
        return -1;
    }
    
    
    
    
    
    
    LOG_INFO("Request sent to M-TEE: cid=%02x%02x..., rid=%02x%02x..., amount=%llu",
             input->cid[0], input->cid[1], output->rid[0], output->rid[1], input->a);
    
    return 0;
}


int on_exec_reply(const on_exec_reply_input_t* input, on_exec_reply_output_t* output) {
    if (!input || !output) {
        return -1;
    }
    
    
    uint8_t server_pubkey[33] = {0}; 
    
    
    uint8_t verify_msg[1024];
    size_t verify_msg_len = snprintf((char*)verify_msg, sizeof(verify_msg),
        "cid=%.32s,rid=%.32s", input->cid, input->rid);
    
    
    if (verify_adapter_signature(
            server_pubkey,
            verify_msg, verify_msg_len,
            input->hat_sigma_S,
            input->T) != 0) {
        LOG_ERROR("Invalid adapter signature from server");
        return -1;
    }
    
    
    
    uint8_t tag[16] = {0}; 
    if (store_response(
            input->cid,
            input->EncRes,
            input->EncRes_len,
            NULL, 0, 
            input->T,
            tag) != 0) {
        LOG_ERROR("Failed to store response");
        return -1;
    }
    
    
    uint8_t sign_msg[128];
    memcpy(sign_msg, input->cid, 32);
    memcpy(sign_msg + 32, input->rid, 32);
    memcpy(sign_msg + 64, input->T, 33);
    
    if (platform_sign_message(sign_msg, 97, g_utee_state.sk, output->sigma_U) != 0) {
        LOG_ERROR("Failed to sign reply");
        return -1;
    }
    
    LOG_INFO("Exec reply processed: cid=%02x%02x..., rid=%02x%02x...",
             input->cid[0], input->cid[1], input->rid[0], input->rid[1]);
    
    return 0;
}



int reveal_secret(const reveal_secret_input_t* input) {
    
    
    LOG_ERROR("reveal_secret should be called on M-TEE, not U-TEE");
    return -1;
}


int close_asc(const close_asc_input_t* input, close_asc_output_t* output) {
    if (!input || !output) {
        return -1;
    }
    
    
    payment_channel_t* channel = NULL;
    for (int i = 0; i < channel_count; i++) {
        if (memcmp(channels[i].channel_id, input->cid, 32) == 0) {
            channel = &channels[i];
            break;
        }
    }
    
    if (!channel) {
        LOG_ERROR("Channel not found");
        return -1;
    }
    
    
    uint8_t tx_data[256];
    size_t tx_len = snprintf((char*)tx_data, sizeof(tx_data),
        "close_asc:cid=%.32s,unlock=%lu", input->cid, (unsigned long)(channel->total_amount - channel->paid_amount));
    
    if (tx_len >= sizeof(output->tx_unlock)) {
        tx_len = sizeof(output->tx_unlock) - 1;
    }
    memcpy(output->tx_unlock, tx_data, tx_len);
    output->tx_unlock_len = tx_len;
    
    
    channel->state = CHANNEL_STATE_CLOSED;
    
    LOG_INFO("ASC closed: cid=%02x%02x...", input->cid[0], input->cid[1]);
    
    return 0;
}


int bind_to_channel(const bind_to_channel_input_t* input) {
    if (!input) {
        return -1;
    }
    
    
    payment_channel_t* channel = NULL;
    for (int i = 0; i < channel_count; i++) {
        if (memcmp(channels[i].channel_id, input->cid, 32) == 0) {
            channel = &channels[i];
            break;
        }
    }
    
    if (!channel) {
        LOG_ERROR("Channel not found");
        return -1;
    }
    
    
    if (channel->state != CHANNEL_STATE_OPEN && channel->state != CHANNEL_STATE_LOCKED) {
        LOG_ERROR("Channel not in valid state for binding");
        return -1;
    }
    
    
    
    if (vault_bind_to_channel(input->vid, input->delta) != 0) {
        LOG_ERROR("Failed to bind assets from vault");
        return -1;
    }
    
    
    channel->total_amount += input->delta;
    
    LOG_INFO("Bound to channel: vid=%02x%02x..., cid=%02x%02x..., delta=%lu",
             input->vid[0], input->vid[1], input->cid[0], input->cid[1], (unsigned long)input->delta);
    
    return 0;
}


int unbind_from_channel(const unbind_from_channel_input_t* input) {
    if (!input) {
        return -1;
    }
    
    
    payment_channel_t* channel = NULL;
    for (int i = 0; i < channel_count; i++) {
        if (memcmp(channels[i].channel_id, input->cid, 32) == 0) {
            channel = &channels[i];
            break;
        }
    }
    
    if (!channel) {
        LOG_ERROR("Channel not found");
        return -1;
    }
    
    
    if (channel->state != CHANNEL_STATE_OPEN) {
        LOG_ERROR("Channel must be OPEN to unbind");
        return -1;
    }
    
    
    uint64_t unbindable = channel->total_amount - channel->locked_amount - channel->paid_amount;
    
    if (unbindable == 0) {
        LOG_ERROR("No funds to unbind");
        return -1;
    }
    
    
    
    if (vault_unbind_from_channel(input->vid, unbindable) != 0) {
        LOG_ERROR("Failed to unbind assets to vault");
        return -1;
    }
    
    channel->total_amount -= unbindable;
    
    LOG_INFO("Unbound from channel: vid=%02x%02x..., cid=%02x%02x..., amount=%llu",
             input->vid[0], input->vid[1], input->cid[0], input->cid[1], unbindable);
    
    return 0;
}


int channel_deposit(const channel_deposit_input_t* input) {
    if (!input) {
        return -1;
    }
    
    
    return utee_channel_deposit(input->cid, input->amount);
}


int channel_withdraw(const channel_withdraw_input_t* input) {
    if (!input) {
        return -1;
    }
    
    
    return utee_channel_withdraw(input->cid, input->amount, input->to_address);
}


int batch_settlement(const batch_settlement_input_t* input, batch_settlement_output_t* output) {
    if (!input || !output) {
        return -1;
    }
    
    
    uint64_t total_settle = 0;
    int settle_count = 0;
    
    for (int i = 0; i < channel_count; i++) {
        
        
        if (channels[i].state == CHANNEL_STATE_CLOSING || channels[i].state == CHANNEL_STATE_CLOSED) {
            uint64_t settle_amount = channels[i].total_amount - channels[i].paid_amount;
            total_settle += settle_amount;
            settle_count++;
        }
    }
    
    if (settle_count == 0) {
        LOG_ERROR("No channels to settle");
        return -1;
    }
    
    
    uint8_t tx_data[512];
    size_t tx_len = snprintf((char*)tx_data, sizeof(tx_data),
        "batch_settlement:owner=%.20s,count=%d,total=%" PRIu64,
        input->owner, settle_count, total_settle);
    
    memcpy(output->tx_batch, tx_data, tx_len);
    output->tx_batch_len = tx_len;
    
    LOG_INFO("Batch settlement: owner=%02x%02x..., count=%d, total=%llu",
             input->owner[0], input->owner[1], settle_count, total_settle);
    
    return 0;
}
