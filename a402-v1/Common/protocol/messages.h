#ifndef MESSAGES_H
#define MESSAGES_H

#include <stdint.h>
#include <stddef.h>

#define MAX_REQUEST_SIZE 1024
#define MAX_RESPONSE_SIZE 4096
#define MAX_TX_PAY_SIZE 256


typedef enum {
    MSG_DEPOSIT = 1,
    MSG_RELEASE_DEPOSIT,
    MSG_CREATE_CHANNEL,
    MSG_COMPUTE_REQUEST,
    MSG_COMPUTE_RESPONSE,
    MSG_TX_PAY_SIGNED,
    MSG_REVEAL_SECRET,
    MSG_CLOSE_CHANNEL
} message_type_t;


typedef struct {
    uint8_t user_address[20];
    uint64_t amount;
    uint8_t tx_hash[32];  
} deposit_msg_t;


typedef struct {
    uint8_t channel_id[32];
    uint8_t m_tee_address[20];
    uint64_t amount;
} create_channel_msg_t;


typedef struct {
    uint8_t channel_id[32];
    uint8_t request_data[MAX_REQUEST_SIZE];
    size_t request_len;
    uint64_t payment_amount;  
} compute_request_msg_t;


typedef struct {
    uint8_t channel_id[32];
    uint8_t encrypted_response[MAX_RESPONSE_SIZE];
    size_t encrypted_len;
    uint8_t tx_pay[MAX_TX_PAY_SIZE];  
    size_t tx_pay_len;
    uint8_t adapter_point_T[33];      
    uint8_t tag[16];                  
} compute_response_msg_t;


typedef struct {
    uint8_t channel_id[32];
    uint8_t tx_pay[MAX_TX_PAY_SIZE];
    size_t tx_pay_len;
    uint8_t signature[64];  
} tx_pay_signed_msg_t;


typedef struct {
    uint8_t channel_id[32];
    uint8_t encrypted_secret_t[48];  
    size_t encrypted_len;
    uint8_t tag[16];
} reveal_secret_msg_t;


typedef struct {
    message_type_t type;
    uint32_t version;
    uint64_t timestamp;
    uint8_t nonce[16];
} message_header_t;


typedef struct {
    message_header_t header;
    union {
        deposit_msg_t deposit;
        create_channel_msg_t create_channel;
        compute_request_msg_t compute_request;
        compute_response_msg_t compute_response;
        tx_pay_signed_msg_t tx_pay_signed;
        reveal_secret_msg_t reveal_secret;
    } body;
} protocol_message_t;

#endif 

