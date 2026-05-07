#ifndef U_TEE_NETWORK_H
#define U_TEE_NETWORK_H

#include <stdint.h>
#include <stddef.h>
#include "../../Common/protocol/messages.h"




int utee_network_init(uint16_t port);


int utee_network_start(void);


void utee_network_stop(void);


int utee_handle_network_message(const uint8_t* data, size_t len, uint8_t* response, size_t* response_len);

#endif 


