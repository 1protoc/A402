#ifndef HOST_VM_APP_H
#define HOST_VM_APP_H

#include <stdint.h>
#include <stddef.h>
#include "../GuestVM/utee_guest.h"




int utee_host_init(const uint8_t* sk, const uint8_t* pk);


int utee_host_handle_deposit(const uint8_t* user_address, uint64_t amount, const uint8_t* tx_hash);


int utee_host_release_deposit(const uint8_t* user_address, uint64_t amount);


int utee_host_create_channel(const uint8_t* m_tee_address, uint64_t amount, uint8_t* channel_id);


int utee_host_handle_compute_request(
    const uint8_t* channel_id,
    const uint8_t* request_data,
    size_t request_len,
    uint64_t payment_amount
);


int utee_host_send_onchain_tx(const uint8_t* tx_data, size_t len);


int utee_host_get_onchain_tx(const uint8_t* tx_hash, uint8_t* tx_data, size_t* len);


int utee_host_channel_deposit(const uint8_t* channel_id, uint64_t amount);


int utee_host_channel_withdraw(const uint8_t* channel_id, uint64_t amount, const uint8_t* to_address);

#endif 


