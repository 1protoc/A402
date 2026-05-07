#include "utee_network.h"
#include "host_vm_app.h"
#include "../../Common/network/network_server.h"
#include "../../Common/network/message_serializer.h"
#include <stdio.h>
#include <string.h>


int utee_handle_network_message(const uint8_t* data, size_t len, uint8_t* response, size_t* response_len) {
    if (!data || len == 0 || !response || !response_len) {
        return -1;
    }
    
    protocol_message_t msg;
    if (deserialize_message(data, len, &msg) != 0) {
        printf("[U-TEE Network] Failed to deserialize message\n");
        return -1;
    }
    
    int ret = 0;
    protocol_message_t response_msg;
    memset(&response_msg, 0, sizeof(response_msg));
    
    switch (msg.header.type) {
        case MSG_DEPOSIT: {
            deposit_msg_t* deposit = &msg.body.deposit;
            ret = utee_host_handle_deposit(deposit->user_address, deposit->amount, deposit->tx_hash);
            
            response_msg.header.type = MSG_DEPOSIT;
            response_msg.header.version = 1;
            
            break;
        }
        
        case MSG_RELEASE_DEPOSIT: {
            deposit_msg_t* deposit = &msg.body.deposit;
            ret = utee_host_release_deposit(deposit->user_address, deposit->amount);
            
            response_msg.header.type = MSG_RELEASE_DEPOSIT;
            response_msg.header.version = 1;
            break;
        }
        
        case MSG_CREATE_CHANNEL: {
            create_channel_msg_t* create = &msg.body.create_channel;
            uint8_t channel_id[32];
            ret = utee_host_create_channel(create->m_tee_address, create->amount, channel_id);
            
            response_msg.header.type = MSG_CREATE_CHANNEL;
            response_msg.header.version = 1;
            if (ret == 0) {
                memcpy(response_msg.body.create_channel.channel_id, channel_id, 32);
            }
            break;
        }
        
        case MSG_COMPUTE_REQUEST: {
            compute_request_msg_t* compute = &msg.body.compute_request;
            ret = utee_host_handle_compute_request(
                compute->channel_id,
                compute->request_data,
                compute->request_len,
                compute->payment_amount
            );
            
            response_msg.header.type = MSG_COMPUTE_REQUEST;
            response_msg.header.version = 1;
            break;
        }
        
        default:
            printf("[U-TEE Network] Unknown message type: %d\n", msg.header.type);
            return -1;
    }
    
    
    size_t resp_len = *response_len;
    if (serialize_message(&response_msg, response, &resp_len) != 0) {
        return -1;
    }
    *response_len = resp_len;
    
    return ret;
}

int utee_network_init(uint16_t port) {
    return network_server_init(port);
}

int utee_network_start(void) {
    return network_server_start(utee_handle_network_message);
}

void utee_network_stop(void) {
    network_server_stop();
}


