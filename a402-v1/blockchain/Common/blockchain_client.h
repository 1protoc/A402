#ifndef BLOCKCHAIN_CLIENT_H
#define BLOCKCHAIN_CLIENT_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif


typedef enum {
    BLOCKCHAIN_BITCOIN = 0,
    BLOCKCHAIN_ETHEREUM = 1
} blockchain_type_t;


typedef struct {
    int success;
    char txid[128];  
    char error[256]; 
} tx_result_t;


typedef struct {
    char channel_id[64];
    char u_tee_address[64];
    char m_tee_address[64];
    uint64_t total_amount;
    uint64_t paid_amount;
    uint32_t nonce;
    int is_open;
} channel_info_t;




int ethereum_init(
    const char* rpc_url,
    const char* contract_address,
    const char* private_key
);


int ethereum_create_channel(
    const char* channel_id,
    const char* m_tee_address,
    uint64_t amount_wei,
    tx_result_t* result
);


int ethereum_deposit(
    const char* channel_id,
    uint64_t amount_wei,
    tx_result_t* result
);


int ethereum_withdraw(
    const char* channel_id,
    uint64_t amount_wei,
    const char* to_address,
    tx_result_t* result
);


int ethereum_close_channel(
    const char* channel_id,
    tx_result_t* result
);


int ethereum_send_raw_transaction(
    const char* to_address,
    const char* data,
    uint64_t value_wei,
    tx_result_t* result
);


int ethereum_get_transaction_data(
    const char* tx_hash,
    uint8_t* data,
    size_t* data_len
);


int ethereum_get_channel_info(
    const char* channel_id,
    channel_info_t* info
);


int bitcoin_init(
    const char* rpc_url
);


int bitcoin_create_channel(
    const char* channel_id,
    const char* u_tee_pubkey,
    const char* m_tee_pubkey,
    const char* user_c_pubkey,
    uint64_t amount_satoshis,
    uint32_t challenge_period,
    tx_result_t* result
);


int bitcoin_deposit(
    const char* channel_id,
    uint64_t amount_satoshis,
    tx_result_t* result
);


int bitcoin_withdraw(
    const char* channel_id,
    uint64_t amount_satoshis,
    const char* to_address,
    tx_result_t* result
);


int bitcoin_close_channel(
    const char* channel_id,
    uint64_t user_c_amount,
    uint64_t m_tee_amount,
    const char* condition,
    tx_result_t* result
);


int bitcoin_send_raw_transaction(
    const char* hex_transaction,
    tx_result_t* result
);


int bitcoin_get_transaction_data(
    const char* txid,
    uint8_t* data,
    size_t* data_len
);


int bitcoin_get_channel_info(
    const char* channel_id,
    channel_info_t* info
);

#ifdef __cplusplus
}
#endif

#endif 
