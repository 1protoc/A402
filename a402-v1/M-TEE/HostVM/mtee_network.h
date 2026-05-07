#ifndef M_TEE_NETWORK_H
#define M_TEE_NETWORK_H

#include <stdint.h>
#include <stddef.h>
#include "../../Common/protocol/messages.h"




int mtee_network_init(uint16_t port);


int mtee_network_start(void);


void mtee_network_stop(void);


int mtee_handle_network_message(const uint8_t* data, size_t len, uint8_t* response, size_t* response_len);


void mtee_set_guest_function_pointer(int (*func)(const uint8_t*, const uint8_t*, size_t, uint64_t));

#endif 

