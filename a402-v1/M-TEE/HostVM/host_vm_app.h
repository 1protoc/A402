#ifndef HOST_VM_APP_H
#define HOST_VM_APP_H

#include <stdint.h>
#include <stddef.h>
#include "../GuestVM/mtee_guest.h"




int mtee_host_init(const uint8_t* sk, const uint8_t* pk);


int mtee_host_generate_and_send_tx(
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    const uint8_t* signature,
    const uint8_t* adapter_point_T,
    uint8_t* tx_data,
    size_t* tx_len
);

#endif 


