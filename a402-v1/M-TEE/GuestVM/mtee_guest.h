#ifndef M_TEE_GUEST_H
#define M_TEE_GUEST_H

#include <stdint.h>
#include <stddef.h>
#include "../../Common/crypto/adapter_signature.h"
#include "../../Common/protocol/messages.h"




int mtee_guest_init(const uint8_t* sk, const uint8_t* pk);


int mtee_handle_compute_request(
    const uint8_t* channel_id,
    const uint8_t* request_data,
    size_t request_len,
    uint64_t payment_amount
);


int mtee_handle_signed_tx_pay(
    const uint8_t* channel_id,
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    const uint8_t* signature,
    const uint8_t* u_tee_pubkey
);


int mtee_reveal_secret_to_utee(const uint8_t* channel_id);


int mtee_execute_computation(
    const uint8_t* request_data,
    size_t request_len,
    uint8_t* response_data,
    size_t* response_len
);

#endif 

