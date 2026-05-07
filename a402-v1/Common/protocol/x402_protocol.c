

#include "x402_protocol.h"
#include <string.h>
#include <openssl/sha.h>
#include <openssl/rand.h>


#define MAX_CHANNELS 100
static x402_channel_t channels[MAX_CHANNELS];
static uint8_t channel_count = 0;


static void generate_channel_id(const uint8_t* party_a, const uint8_t* party_b,
                                uint64_t timestamp, uint8_t* channel_id_out) {
    uint8_t data[72];
    memcpy(data, party_a, 32);
    memcpy(data + 32, party_b, 32);
    memcpy(data + 64, &timestamp, 8);
    
    SHA256(data, 72, channel_id_out);
}

int x402_create_channel(const x402_create_channel_input_t* input, uint8_t* channel_id_out) {
    if (channel_count >= MAX_CHANNELS) {
        return -1; 
    }
    
    x402_channel_t* channel = &channels[channel_count];
    
    
    uint64_t timestamp = 0; 
    generate_channel_id(input->party_a, input->party_b, timestamp, channel->channel_id);
    memcpy(channel_id_out, channel->channel_id, 32);
    
    
    memcpy(channel->party_a, input->party_a, 32);
    memcpy(channel->party_b, input->party_b, 32);
    channel->balance_a = input->initial_balance_a;
    channel->balance_b = input->initial_balance_b;
    channel->total_capacity = input->initial_balance_a + input->initial_balance_b;
    channel->sequence_number = 0;
    channel->state = X402_CHANNEL_STATE_OPEN;
    channel->dispute_period = input->dispute_period;
    channel->htlc_count = 0;
    
    channel_count++;
    return 0;
}

int x402_update_channel(const x402_update_channel_input_t* input) {
    
    x402_channel_t* channel = NULL;
    for (int i = 0; i < channel_count; i++) {
        if (memcmp(channels[i].channel_id, input->channel_id, 32) == 0) {
            channel = &channels[i];
            break;
        }
    }
    
    if (!channel || channel->state != X402_CHANNEL_STATE_OPEN) {
        return -1; 
    }
    
    
    if (input->sequence_number <= channel->sequence_number) {
        return -2; 
    }
    
    
    if (channel->htlc_count >= 10) {
        return -3; 
    }
    
    
    x402_htlc_t* htlc = &channel->htlc_list[channel->htlc_count];
    memcpy(htlc->payment_hash, input->payment_hash, 32);
    htlc->amount = input->amount;
    htlc->expiry_block = input->expiry_block;
    memset(htlc->preimage, 0, 32);
    htlc->is_settled = 0;
    
    channel->htlc_count++;
    channel->sequence_number = input->sequence_number;
    
    return 0;
}

int x402_settle_htlc(const x402_settle_htlc_input_t* input) {
    
    x402_channel_t* channel = NULL;
    for (int i = 0; i < channel_count; i++) {
        if (memcmp(channels[i].channel_id, input->channel_id, 32) == 0) {
            channel = &channels[i];
            break;
        }
    }
    
    if (!channel || channel->state != X402_CHANNEL_STATE_OPEN) {
        return -1;
    }
    
    
    x402_htlc_t* htlc = NULL;
    for (int i = 0; i < channel->htlc_count; i++) {
        if (memcmp(channel->htlc_list[i].payment_hash, input->payment_hash, 32) == 0) {
            htlc = &channel->htlc_list[i];
            break;
        }
    }
    
    if (!htlc || htlc->is_settled) {
        return -2; 
    }
    
    
    uint8_t computed_hash[32];
    x402_generate_payment_hash(input->preimage, 32, computed_hash);
    if (memcmp(computed_hash, input->payment_hash, 32) != 0) {
        return -3; 
    }
    
    
    if (channel->balance_b < htlc->amount) {
        return -4; 
    }
    
    channel->balance_b -= htlc->amount;
    channel->balance_a += htlc->amount;
    htlc->is_settled = 1;
    memcpy(htlc->preimage, input->preimage, 32);
    
    return 0;
}

int x402_close_channel(const x402_close_channel_input_t* input) {
    
    x402_channel_t* channel = NULL;
    for (int i = 0; i < channel_count; i++) {
        if (memcmp(channels[i].channel_id, input->channel_id, 32) == 0) {
            channel = &channels[i];
            break;
        }
    }
    
    if (!channel) {
        return -1;
    }
    
    
    if (input->sequence_number < channel->sequence_number) {
        return -2;
    }
    
    
    channel->state = X402_CHANNEL_STATE_CLOSED;
    channel->sequence_number = input->sequence_number;
    
    return 0;
}

int x402_get_channel(const uint8_t* channel_id, x402_channel_t* channel_out) {
    for (int i = 0; i < channel_count; i++) {
        if (memcmp(channels[i].channel_id, channel_id, 32) == 0) {
            memcpy(channel_out, &channels[i], sizeof(x402_channel_t));
            return 0;
        }
    }
    return -1;
}

void x402_generate_payment_hash(const uint8_t* preimage, size_t preimage_len, uint8_t* hash_out) {
    SHA256(preimage, preimage_len, hash_out);
}
