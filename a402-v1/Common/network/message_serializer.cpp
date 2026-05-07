#include "message_serializer.h"
#include <string.h>
#include <stdio.h>



int serialize_message(const protocol_message_t* msg, uint8_t* buffer, size_t* buffer_len) {
    if (!msg || !buffer || !buffer_len) {
        return -1;
    }
    
    size_t offset = 0;
    
    
    if (*buffer_len < sizeof(message_header_t)) {
        return -1;
    }
    memcpy(buffer + offset, &msg->header, sizeof(message_header_t));
    offset += sizeof(message_header_t);
    
    
    size_t body_size = 0;
    switch (msg->header.type) {
        case MSG_DEPOSIT:
            body_size = sizeof(deposit_msg_t);
            if (offset + body_size > *buffer_len) {
                return -1;
            }
            memcpy(buffer + offset, &msg->body.deposit, body_size);
            break;
        case MSG_CREATE_CHANNEL:
            body_size = sizeof(create_channel_msg_t);
            if (offset + body_size > *buffer_len) {
                return -1;
            }
            memcpy(buffer + offset, &msg->body.create_channel, body_size);
            break;
        case MSG_COMPUTE_REQUEST:
            body_size = sizeof(compute_request_msg_t);
            if (offset + body_size > *buffer_len) {
                return -1;
            }
            memcpy(buffer + offset, &msg->body.compute_request, body_size);
            break;
        default:
            return -1;
    }
    
    offset += body_size;
    *buffer_len = offset;
    return 0;
}

int deserialize_message(const uint8_t* buffer, size_t buffer_len, protocol_message_t* msg) {
    if (!buffer || !msg) {
        return -1;
    }
    
    size_t offset = 0;
    
    
    if (buffer_len < sizeof(message_header_t)) {
        return -1;
    }
    memcpy(&msg->header, buffer + offset, sizeof(message_header_t));
    offset += sizeof(message_header_t);
    
    
    switch (msg->header.type) {
        case MSG_DEPOSIT:
            if (offset + sizeof(deposit_msg_t) > buffer_len) {
                return -1;
            }
            memcpy(&msg->body.deposit, buffer + offset, sizeof(deposit_msg_t));
            break;
        case MSG_CREATE_CHANNEL:
            if (offset + sizeof(create_channel_msg_t) > buffer_len) {
                return -1;
            }
            memcpy(&msg->body.create_channel, buffer + offset, sizeof(create_channel_msg_t));
            break;
        case MSG_COMPUTE_REQUEST:
            if (offset + sizeof(compute_request_msg_t) > buffer_len) {
                return -1;
            }
            memcpy(&msg->body.compute_request, buffer + offset, sizeof(compute_request_msg_t));
            break;
        default:
            return -1;
    }
    
    return 0;
}

int serialize_deposit_msg(const deposit_msg_t* msg, uint8_t* buffer, size_t* buffer_len) {
    if (!msg || !buffer || !buffer_len) {
        return -1;
    }
    
    if (*buffer_len < sizeof(deposit_msg_t)) {
        return -1;
    }
    
    memcpy(buffer, msg, sizeof(deposit_msg_t));
    *buffer_len = sizeof(deposit_msg_t);
    return 0;
}

int serialize_create_channel_msg(const create_channel_msg_t* msg, uint8_t* buffer, size_t* buffer_len) {
    if (!msg || !buffer || !buffer_len) {
        return -1;
    }
    
    if (*buffer_len < sizeof(create_channel_msg_t)) {
        return -1;
    }
    
    memcpy(buffer, msg, sizeof(create_channel_msg_t));
    *buffer_len = sizeof(create_channel_msg_t);
    return 0;
}

int serialize_compute_request_msg(const compute_request_msg_t* msg, uint8_t* buffer, size_t* buffer_len) {
    if (!msg || !buffer || !buffer_len) {
        return -1;
    }
    
    size_t needed = sizeof(compute_request_msg_t);
    if (*buffer_len < needed) {
        return -1;
    }
    
    memcpy(buffer, msg, needed);
    *buffer_len = needed;
    return 0;
}

int deserialize_deposit_msg(const uint8_t* buffer, size_t buffer_len, deposit_msg_t* msg) {
    if (!buffer || !msg) {
        return -1;
    }
    
    if (buffer_len < sizeof(deposit_msg_t)) {
        return -1;
    }
    
    memcpy(msg, buffer, sizeof(deposit_msg_t));
    return 0;
}

int deserialize_create_channel_msg(const uint8_t* buffer, size_t buffer_len, create_channel_msg_t* msg) {
    if (!buffer || !msg) {
        return -1;
    }
    
    if (buffer_len < sizeof(create_channel_msg_t)) {
        return -1;
    }
    
    memcpy(msg, buffer, sizeof(create_channel_msg_t));
    return 0;
}

int deserialize_compute_request_msg(const uint8_t* buffer, size_t buffer_len, compute_request_msg_t* msg) {
    if (!buffer || !msg) {
        return -1;
    }
    
    if (buffer_len < sizeof(compute_request_msg_t)) {
        return -1;
    }
    
    memcpy(msg, buffer, sizeof(compute_request_msg_t));
    return 0;
}


