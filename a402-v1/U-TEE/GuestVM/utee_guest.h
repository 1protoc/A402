#ifndef U_TEE_GUEST_H
#define U_TEE_GUEST_H

#include <stdint.h>
#include <stddef.h>
#include "../../Common/channel/channel.h"
#include "../../Common/protocol/messages.h"
#include "../../Common/protocol/a402_protocol.h"




int utee_guest_init(const uint8_t* sk, const uint8_t* pk);


int utee_handle_deposit(const uint8_t* user_address, uint64_t amount, const uint8_t* tx_hash);


int utee_release_deposit(const uint8_t* user_address, uint64_t amount);


int utee_create_channel(const uint8_t* m_tee_address, uint64_t amount, uint8_t* channel_id);


int utee_handle_compute_request(
    const uint8_t* channel_id,
    const uint8_t* request_data,
    size_t request_len,
    uint64_t payment_amount
);


int utee_handle_compute_response(
    const uint8_t* channel_id,
    const uint8_t* encrypted_response,
    size_t encrypted_len,
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    const uint8_t* adapter_point_T,
    const uint8_t* tag
);


int utee_sign_and_send_tx_pay(
    const uint8_t* channel_id,
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    uint8_t* signature
);


int utee_handle_revealed_secret(
    const uint8_t* channel_id,
    const uint8_t* encrypted_secret_t,
    size_t encrypted_len,
    const uint8_t* tag,
    uint8_t* decrypted_response,
    size_t* response_len
);


int utee_extract_secret_from_chain_tx(
    const uint8_t* channel_id,
    const uint8_t* tx_data,
    size_t tx_len,
    uint8_t* decrypted_response,
    size_t* response_len
);


int utee_get_channel_info(const uint8_t* channel_id, uint64_t* total_amount, uint64_t* locked_amount);


int utee_channel_deposit(const uint8_t* channel_id, uint64_t amount);


int utee_channel_withdraw(const uint8_t* channel_id, uint64_t amount, const uint8_t* to_address);

#endif 

