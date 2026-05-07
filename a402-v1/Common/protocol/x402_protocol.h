

#ifndef X402_PROTOCOL_H
#define X402_PROTOCOL_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif


#define X402_VERSION 1


typedef enum {
    X402_CHANNEL_STATE_CLOSED = 0,
    X402_CHANNEL_STATE_OPENING,
    X402_CHANNEL_STATE_OPEN,
    X402_CHANNEL_STATE_CLOSING,
    X402_CHANNEL_STATE_DISPUTE
} x402_channel_state_t;


typedef struct {
    uint8_t payment_hash[32];      
    uint64_t amount;                
    uint32_t expiry_block;          
    uint8_t preimage[32];           
    uint8_t is_settled;             
} x402_htlc_t;


typedef struct {
    uint8_t channel_id[32];         
    uint8_t party_a[32];             
    uint8_t party_b[32];             
    uint64_t balance_a;              
    uint64_t balance_b;              
    uint64_t total_capacity;         
    uint32_t sequence_number;        
    x402_channel_state_t state;      
    uint32_t dispute_period;         
    x402_htlc_t htlc_list[10];       
    uint8_t htlc_count;              
} x402_channel_t;


typedef struct {
    uint8_t party_a[32];             
    uint8_t party_b[32];             
    uint64_t initial_balance_a;      
    uint64_t initial_balance_b;      
    uint32_t dispute_period;         
} x402_create_channel_input_t;


typedef struct {
    uint8_t channel_id[32];         
    uint64_t amount;                
    uint8_t payment_hash[32];       
    uint32_t expiry_block;          
    uint32_t sequence_number;       
} x402_update_channel_input_t;


typedef struct {
    uint8_t channel_id[32];         
    uint8_t payment_hash[32];       
    uint8_t preimage[32];           
} x402_settle_htlc_input_t;


typedef struct {
    uint8_t channel_id[32];         
    uint32_t sequence_number;       
} x402_close_channel_input_t;


int x402_create_channel(const x402_create_channel_input_t* input, uint8_t* channel_id_out);


int x402_update_channel(const x402_update_channel_input_t* input);


int x402_settle_htlc(const x402_settle_htlc_input_t* input);


int x402_close_channel(const x402_close_channel_input_t* input);


int x402_get_channel(const uint8_t* channel_id, x402_channel_t* channel_out);


void x402_generate_payment_hash(const uint8_t* preimage, size_t preimage_len, uint8_t* hash_out);

#ifdef __cplusplus
}
#endif

#endif 
