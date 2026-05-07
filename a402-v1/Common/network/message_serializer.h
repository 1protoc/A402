#ifndef MESSAGE_SERIALIZER_H
#define MESSAGE_SERIALIZER_H

#include <stdint.h>
#include <stddef.h>
#include "../protocol/messages.h"




int serialize_message(const protocol_message_t* msg, uint8_t* buffer, size_t* buffer_len);


int deserialize_message(const uint8_t* buffer, size_t buffer_len, protocol_message_t* msg);


int serialize_deposit_msg(const deposit_msg_t* msg, uint8_t* buffer, size_t* buffer_len);


int serialize_create_channel_msg(const create_channel_msg_t* msg, uint8_t* buffer, size_t* buffer_len);


int serialize_compute_request_msg(const compute_request_msg_t* msg, uint8_t* buffer, size_t* buffer_len);


int deserialize_deposit_msg(const uint8_t* buffer, size_t buffer_len, deposit_msg_t* msg);


int deserialize_create_channel_msg(const uint8_t* buffer, size_t buffer_len, create_channel_msg_t* msg);


int deserialize_compute_request_msg(const uint8_t* buffer, size_t buffer_len, compute_request_msg_t* msg);

#endif 


