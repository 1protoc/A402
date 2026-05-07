#ifndef NETWORK_SERVER_H
#define NETWORK_SERVER_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>




typedef int (*message_handler_t)(const uint8_t* data, size_t len, uint8_t* response, size_t* response_len);


int network_server_init(uint16_t port);


int network_server_start(message_handler_t handler);


void network_server_stop(void);


int network_server_send_response(int client_fd, const uint8_t* data, size_t len);

#endif 


