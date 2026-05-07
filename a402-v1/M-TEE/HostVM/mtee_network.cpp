#include "mtee_network.h"
#include "host_vm_app.h"
#include "../../Common/network/network_server.h"
#include "../../Common/network/message_serializer.h"
#include "../GuestVM/mtee_guest.h"
#include <stdio.h>
#include <string.h>


static int (*g_mtee_guest_handle_compute_request)(const uint8_t*, const uint8_t*, size_t, uint64_t) = NULL;


void mtee_set_guest_function_pointer(int (*func)(const uint8_t*, const uint8_t*, size_t, uint64_t)) {
    g_mtee_guest_handle_compute_request = func;
}


int mtee_handle_network_message(const uint8_t* data, size_t len, uint8_t* response, size_t* response_len) {
    if (!data || len == 0 || !response || !response_len) {
        return -1;
    }
    
    protocol_message_t msg;
    if (deserialize_message(data, len, &msg) != 0) {
        printf("[M-TEE Network] Failed to deserialize message\n");
        return -1;
    }
    
    int ret = 0;
    protocol_message_t response_msg;
    memset(&response_msg, 0, sizeof(response_msg));
    
    switch (msg.header.type) {
        case MSG_COMPUTE_REQUEST: {
            compute_request_msg_t* compute = &msg.body.compute_request;
            ret = mtee_handle_compute_request(
                compute->channel_id,
                compute->request_data,
                compute->request_len,
                compute->payment_amount
            );
            
            response_msg.header.type = MSG_COMPUTE_RESPONSE;
            response_msg.header.version = 1;
            break;
        }
        
        default:
            printf("[M-TEE Network] Unknown message type: %d\n", msg.header.type);
            return -1;
    }
    
    
    size_t resp_len = *response_len;
    if (serialize_message(&response_msg, response, &resp_len) != 0) {
        return -1;
    }
    *response_len = resp_len;
    
    return ret;
}

int mtee_network_init(uint16_t port) {
    return network_server_init(port);
}

int mtee_network_start(void) {
    return network_server_start(mtee_handle_network_message);
}

void mtee_network_stop(void) {
    network_server_stop();
}

