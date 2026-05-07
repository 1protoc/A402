

#include "utee_guest.h"
#include "platform_sev.h"
#include "sev_vm_communication.h"
#include "../../Common/crypto/adapter_signature.h"
#include "../../Common/crypto/encryption.h"
#include "../../Common/crypto/key_exchange.h"
#include "../../Common/storage/response_storage.h"
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
} utee_state_t;


utee_state_t g_utee_state = {0};


#define MAX_CHANNELS 100
payment_channel_t channels[MAX_CHANNELS];
int channel_count = 0;


typedef struct {
    uint8_t user_address[20];
    uint64_t balance;
} user_balance_t;

#define MAX_USERS 1000
static user_balance_t user_balances[MAX_USERS];
static int user_count = 0;


static payment_channel_t* find_channel(const uint8_t* channel_id) {
    for (int i = 0; i < channel_count; i++) {
        if (memcmp(channels[i].channel_id, channel_id, 32) == 0) {
            return &channels[i];
        }
    }
    return NULL;
}


static user_balance_t* find_or_create_user(const uint8_t* user_address) {
    for (int i = 0; i < user_count; i++) {
        if (memcmp(user_balances[i].user_address, user_address, 20) == 0) {
            return &user_balances[i];
        }
    }
    if (user_count < MAX_USERS) {
        memcpy(user_balances[user_count].user_address, user_address, 20);
        user_balances[user_count].balance = 0;
        return &user_balances[user_count++];
    }
    return NULL;
}


static int send_to_mtee_via_host(const uint8_t* data, size_t len) {
    
    
    uint32_t m_tee_vm_id = 2; 
    
    return sev_vm_comm_send(m_tee_vm_id, data, len);
}

int utee_guest_init(const uint8_t* sk, const uint8_t* pk) {
    if (!sk || !pk) {
        return -1;
    }
    
    
    if (platform_sev_init() != 0) {
        return -1;
    }
    
    
    uint32_t utee_vm_id = 1; 
    if (sev_vm_comm_init(utee_vm_id, SEV_COMM_CHANNEL_HOST_MEDIATED) != 0) {
        return -1;
    }
    
    memcpy(g_utee_state.sk, sk, 32);
    memcpy(g_utee_state.pk, pk, 33);
    g_utee_state.initialized = 1;
    
    
    
    
    uint8_t m_tee_pubkey[33] = {0x02}; 
    if (compute_shared_secret(sk, m_tee_pubkey, g_utee_state.shared_key_sk_m) != 0) {
        
        memset(g_utee_state.shared_key_sk_m, 0x42, 32);
    }
    
    LOG_INFO("U-TEE Guest VM initialized");
    return 0;
}

int utee_handle_deposit(const uint8_t* user_address, uint64_t amount, const uint8_t* tx_hash) {
    if (!g_utee_state.initialized || !user_address || !tx_hash) {
        return -1;
    }
    
    user_balance_t* user = find_or_create_user(user_address);
    if (!user) {
        LOG_ERROR("Failed to find or create user");
        return -1;
    }
    
    
    
    user->balance += amount;
    LOG_INFO("Deposit: user=%02x%02x..., amount=%llu", 
             user_address[0], user_address[1], amount);
    
    return 0;
}

int utee_release_deposit(const uint8_t* user_address, uint64_t amount) {
    if (!g_utee_state.initialized || !user_address) {
        return -1;
    }
    
    user_balance_t* user = find_or_create_user(user_address);
    if (!user || user->balance < amount) {
        LOG_ERROR("Release deposit failed: insufficient balance");
        return -1;
    }
    
    user->balance -= amount;
    LOG_INFO("Release deposit: user=%02x%02x..., amount=%llu", 
             user_address[0], user_address[1], amount);
    
    
    
    return 0;
}

int utee_create_channel(const uint8_t* m_tee_address, uint64_t amount, uint8_t* channel_id) {
    if (!g_utee_state.initialized || !m_tee_address || !channel_id) {
        return -1;
    }
    
    if (channel_count >= MAX_CHANNELS) {
        return -1;
    }
    
    payment_channel_t* channel = &channels[channel_count++];
    
    
    if (platform_get_random(channel->channel_id, 32) != 0) {
        return -1;
    }
    
    memcpy(channel->u_tee_address, g_utee_state.address, 20);
    memcpy(channel->m_tee_address, m_tee_address, 20);
    channel->total_amount = amount;
    channel->locked_amount = 0;
    channel->paid_amount = 0;
    channel->state = CHANNEL_STATE_OPEN;
    channel->nonce = 0;
    
    memcpy(channel_id, channel->channel_id, 32);
    
    LOG_INFO("Channel created: channel_id=%02x%02x..., amount=%llu", 
             channel->channel_id[0], channel->channel_id[1], amount);
    
    return 0;
}

int utee_handle_compute_request(
    const uint8_t* channel_id,
    const uint8_t* request_data,
    size_t request_len,
    uint64_t payment_amount)
{
    if (!g_utee_state.initialized || !channel_id || !request_data) {
        return -1;
    }
    
    payment_channel_t* channel = find_channel(channel_id);
    if (!channel || channel->state != CHANNEL_STATE_OPEN) {
        return -1;
    }
    
    
    if (channel->total_amount - channel->locked_amount < payment_amount) {
        LOG_ERROR("Insufficient channel balance");
        return -1;
    }
    
    channel->locked_amount += payment_amount;
    channel->state = CHANNEL_STATE_LOCKED;
    LOG_INFO("Assets locked: channel_id=%02x%02x..., amount=%llu", 
             channel_id[0], channel_id[1], payment_amount);
    
    
    compute_request_msg_t msg;
    memcpy(msg.channel_id, channel_id, 32);
    memcpy(msg.request_data, request_data, request_len);
    msg.request_len = request_len;
    msg.payment_amount = payment_amount;
    
    
    uint8_t encrypted[4096];
    uint8_t tag[16];
    int encrypted_len = encrypt_with_shared_key(
        (uint8_t*)&msg, sizeof(msg),
        g_utee_state.shared_key_sk_m,
        encrypted, sizeof(encrypted),
        tag
    );
    
    if (encrypted_len < 0) {
        LOG_ERROR("Failed to encrypt compute request");
        return -1;
    }
    
    
    if (send_to_mtee_via_host(encrypted, encrypted_len) != 0) {
        LOG_ERROR("Failed to send compute request to M-TEE");
        return -1;
    }
    
    LOG_INFO("Compute request sent to M-TEE: channel_id=%02x%02x...", 
             channel_id[0], channel_id[1]);
    
    return 0;
}

int utee_handle_compute_response(
    const uint8_t* channel_id,
    const uint8_t* encrypted_response,
    size_t encrypted_len,
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    const uint8_t* adapter_point_T,
    const uint8_t* tag)
{
    if (!g_utee_state.initialized || !channel_id || !encrypted_response || !tx_pay || !adapter_point_T || !tag) {
        return -1;
    }
    
    payment_channel_t* channel = find_channel(channel_id);
    if (!channel) {
        return -1;
    }
    
    
    if (store_response(channel_id, encrypted_response, encrypted_len,
                       tx_pay, tx_pay_len, adapter_point_T, tag) != 0) {
        LOG_ERROR("Failed to store compute response");
        return -1;
    }
    
    LOG_INFO("Compute response stored: channel_id=%02x%02x...", 
             channel_id[0], channel_id[1]);
    
    return 0;
}

int utee_sign_and_send_tx_pay(
    const uint8_t* channel_id,
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    uint8_t* signature)
{
    if (!g_utee_state.initialized || !channel_id || !tx_pay || !signature) {
        return -1;
    }
    
    
    if (platform_sign_message(tx_pay, tx_pay_len, g_utee_state.sk, signature) != 0) {
        LOG_ERROR("Failed to sign tx_pay");
        return -1;
    }
    
    LOG_INFO("tx_pay signed: channel_id=%02x%02x...", channel_id[0], channel_id[1]);
    
    
    tx_pay_signed_msg_t signed_msg;
    memcpy(signed_msg.channel_id, channel_id, 32);
    memcpy(signed_msg.tx_pay, tx_pay, tx_pay_len);
    signed_msg.tx_pay_len = tx_pay_len;
    memcpy(signed_msg.signature, signature, 64);
    
    
    uint8_t encrypted[4096];
    uint8_t tag[16];
    int encrypted_len = encrypt_with_shared_key(
        (uint8_t*)&signed_msg, sizeof(signed_msg),
        g_utee_state.shared_key_sk_m,
        encrypted, sizeof(encrypted),
        tag
    );
    
    if (encrypted_len < 0) {
        return -1;
    }
    
    if (send_to_mtee_via_host(encrypted, encrypted_len) != 0) {
        return -1;
    }
    
    return 0;
}

int utee_handle_revealed_secret(
    const uint8_t* channel_id,
    const uint8_t* encrypted_secret_t,
    size_t encrypted_len,
    const uint8_t* tag,
    uint8_t* decrypted_response,
    size_t* response_len)
{
    if (!g_utee_state.initialized || !channel_id || !encrypted_secret_t || !tag || !decrypted_response) {
        return -1;
    }
    
    
    uint8_t secret_t[32];
    int decrypted_t_len = decrypt_with_shared_key(
        encrypted_secret_t, encrypted_len,
        g_utee_state.shared_key_sk_m,
        tag,
        secret_t, sizeof(secret_t)
    );
    
    if (decrypted_t_len != 32) {
        return -1;
    }
    
    
    stored_response_t* stored = get_stored_response(channel_id);
    if (!stored) {
        LOG_ERROR("No stored response found for channel");
        return -1;
    }
    
    
    uint8_t iv[16] = {0}; 
    int decrypted_len = decrypt_data(
        stored->encrypted_response, stored->encrypted_len,
        secret_t, iv,
        stored->tag,
        decrypted_response, *response_len
    );
    
    if (decrypted_len < 0) {
        LOG_ERROR("Failed to decrypt response");
        return -1;
    }
    
    *response_len = decrypted_len;
    
    
    payment_channel_t* channel = find_channel(channel_id);
    if (channel) {
        
        
        
        uint64_t payment_amount = 0; 
        
        if (payment_amount > 0 && channel->locked_amount >= payment_amount) {
            channel->locked_amount -= payment_amount;
            channel->paid_amount += payment_amount;
            
            
        }
        
        
        if (channel->locked_amount == 0 && channel->state == CHANNEL_STATE_LOCKED) {
            channel->state = CHANNEL_STATE_OPEN;
        }
        
        LOG_INFO("Channel state updated: cid=%02x%02x..., locked=%llu, paid=%llu",
                 channel_id[0], channel_id[1], channel->locked_amount, channel->paid_amount);
    }
    
    
    remove_stored_response(channel_id);
    
    LOG_INFO("Response decrypted and ready to return to user: channel_id=%02x%02x..., len=%d", 
             channel_id[0], channel_id[1], decrypted_len);
    
    
    
    
    return 0;
}

int utee_extract_secret_from_chain_tx(
    const uint8_t* channel_id,
    const uint8_t* tx_data,
    size_t tx_len,
    uint8_t* decrypted_response,
    size_t* response_len)
{
    if (!g_utee_state.initialized || !channel_id || !tx_data || !decrypted_response) {
        return -1;
    }
    
    payment_channel_t* channel = find_channel(channel_id);
    if (!channel) {
        return -1;
    }
    
    
    stored_response_t* stored = get_stored_response(channel_id);
    if (!stored) {
        return -1;
    }
    
    uint8_t adapter_point_T[33];
    memcpy(adapter_point_T, stored->adapter_point_T, 33);
    
    
    uint8_t secret_t[32];
    if (extract_secret_from_tx(tx_data, tx_len, adapter_point_T, secret_t) != 0) {
        LOG_ERROR("Failed to extract secret from transaction");
        return -1;
    }
    
    LOG_INFO("Secret extracted from transaction: channel_id=%02x%02x...", 
             channel_id[0], channel_id[1]);
    
    
    uint8_t iv[16] = {0}; 
    int decrypted_len = decrypt_data(
        stored->encrypted_response, stored->encrypted_len,
        secret_t, iv,
        stored->tag,
        decrypted_response, *response_len
    );
    
    if (decrypted_len < 0) {
        LOG_ERROR("Failed to decrypt response from transaction");
        return -1;
    }
    
    *response_len = decrypted_len;
    
    
    remove_stored_response(channel_id);
    
    LOG_INFO("Response decrypted from transaction: channel_id=%02x%02x..., len=%d", 
             channel_id[0], channel_id[1], decrypted_len);
    
    return 0;
}

int utee_get_channel_info(const uint8_t* channel_id, uint64_t* total_amount, uint64_t* locked_amount) {
    if (!channel_id || !total_amount || !locked_amount) {
        return -1;
    }
    
    payment_channel_t* channel = find_channel(channel_id);
    if (!channel) {
        return -1;
    }
    
    *total_amount = channel->total_amount;
    *locked_amount = channel->locked_amount;
    
    return 0;
}


extern int utee_host_channel_deposit(const uint8_t* channel_id, uint64_t amount);
extern int utee_host_channel_withdraw(const uint8_t* channel_id, uint64_t amount, const uint8_t* to_address);

int utee_channel_deposit(const uint8_t* channel_id, uint64_t amount) {
    if (!g_utee_state.initialized || !channel_id || amount == 0) {
        return -1;
    }
    
    payment_channel_t* channel = find_channel(channel_id);
    if (!channel || channel->state != CHANNEL_STATE_OPEN) {
        LOG_ERROR("Channel not found or not open");
        return -1;
    }
    
    LOG_INFO("Channel deposit: channel_id=%02x%02x..., amount=%llu", 
             channel_id[0], channel_id[1], amount);
    
    
    int ret = utee_host_channel_deposit(channel_id, amount);
    if (ret != 0) {
        LOG_ERROR("Failed to deposit to channel on chain");
        return -1;
    }
    
    
    channel->total_amount += amount;
    
    LOG_INFO("Channel deposit successful: new total=%llu", channel->total_amount);
    
    return 0;
}

int utee_channel_withdraw(const uint8_t* channel_id, uint64_t amount, const uint8_t* to_address) {
    if (!g_utee_state.initialized || !channel_id || !to_address || amount == 0) {
        return -1;
    }
    
    payment_channel_t* channel = find_channel(channel_id);
    if (!channel || channel->state != CHANNEL_STATE_OPEN) {
        LOG_ERROR("Channel not found or not open");
        return -1;
    }
    
    
    uint64_t available = channel->total_amount - channel->paid_amount;
    if (amount > available) {
        LOG_ERROR("Insufficient channel balance: available=%llu, requested=%llu", 
                 available, amount);
        return -1;
    }
    
    LOG_INFO("Channel withdraw: channel_id=%02x%02x..., amount=%llu, to=%02x%02x...", 
             channel_id[0], channel_id[1], amount, to_address[0], to_address[1]);
    
    
    int ret = utee_host_channel_withdraw(channel_id, amount, to_address);
    if (ret != 0) {
        LOG_ERROR("Failed to withdraw from channel on chain");
        return -1;
    }
    
    
    channel->total_amount -= amount;
    
    LOG_INFO("Channel withdraw successful: new total=%llu", channel->total_amount);
    
    return 0;
}