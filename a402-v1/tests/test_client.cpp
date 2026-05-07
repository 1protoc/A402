

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <stdint.h>
#include <time.h>
#include "../Common/protocol/messages.h"
#include "../Common/network/message_serializer.h"

#define U_TEE_PORT 8080
#define M_TEE_PORT 8081
#define BUFFER_SIZE 4096


static int connect_to_server(const char* ip, uint16_t port) {
    int sock = socket(AF_INET, SOCK_STREAM, 0);
    if (sock < 0) {
        perror("socket");
        return -1;
    }
    
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons(port);
    
    if (inet_pton(AF_INET, ip, &addr.sin_addr) <= 0) {
        perror("inet_pton");
        close(sock);
        return -1;
    }
    
    if (connect(sock, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        perror("connect");
        close(sock);
        return -1;
    }
    
    printf("[Test Client] Connected to %s:%d\n", ip, port);
    return sock;
}


static int send_and_receive(int sock, const uint8_t* data, size_t data_len, uint8_t* response, size_t* response_len) {
    
    ssize_t sent = send(sock, data, data_len, 0);
    if (sent < 0) {
        perror("send");
        return -1;
    }
    printf("[Test Client] Sent %zd bytes\n", sent);
    
    
    ssize_t received = recv(sock, response, *response_len, 0);
    if (received < 0) {
        perror("recv");
        return -1;
    }
    
    *response_len = received;
    printf("[Test Client] Received %zd bytes\n", received);
    return 0;
}


static int test_deposit(int sock) {
    printf("\n=== Test Deposit ===\n");
    
    protocol_message_t msg;
    memset(&msg, 0, sizeof(msg));
    msg.header.type = MSG_DEPOSIT;
    msg.header.version = 1;
    msg.header.timestamp = time(NULL);
    
    
    uint8_t user_address[20] = {0x01, 0x02, 0x03, 0x04, 0x05};
    memcpy(msg.body.deposit.user_address, user_address, 20);
    msg.body.deposit.amount = 1000000; 
    uint8_t tx_hash[32] = {0x11, 0x22, 0x33, 0x44};
    memcpy(msg.body.deposit.tx_hash, tx_hash, 32);
    
    
    uint8_t buffer[BUFFER_SIZE];
    size_t buffer_len = sizeof(buffer);
    if (serialize_message(&msg, buffer, &buffer_len) != 0) {
        printf("[Test] Failed to serialize deposit message\n");
        return -1;
    }
    
    
    uint8_t response[BUFFER_SIZE];
    size_t response_len = sizeof(response);
    if (send_and_receive(sock, buffer, buffer_len, response, &response_len) != 0) {
        return -1;
    }
    
    
    protocol_message_t resp_msg;
    if (deserialize_message(response, response_len, &resp_msg) == 0) {
        printf("[Test] Deposit response: type=%d, version=%d\n", 
               resp_msg.header.type, resp_msg.header.version);
    }
    
    return 0;
}


static int test_create_channel(int sock) {
    printf("\n=== Test Create Channel ===\n");
    
    protocol_message_t msg;
    memset(&msg, 0, sizeof(msg));
    msg.header.type = MSG_CREATE_CHANNEL;
    msg.header.version = 1;
    msg.header.timestamp = time(NULL);
    
    
    uint8_t m_tee_address[20] = {0xAA, 0xBB, 0xCC, 0xDD, 0xEE};
    memcpy(msg.body.create_channel.m_tee_address, m_tee_address, 20);
    msg.body.create_channel.amount = 500000; 
    
    
    uint8_t buffer[BUFFER_SIZE];
    size_t buffer_len = sizeof(buffer);
    if (serialize_message(&msg, buffer, &buffer_len) != 0) {
        printf("[Test] Failed to serialize create channel message\n");
        return -1;
    }
    
    
    uint8_t response[BUFFER_SIZE];
    size_t response_len = sizeof(response);
    if (send_and_receive(sock, buffer, buffer_len, response, &response_len) != 0) {
        return -1;
    }
    
    
    protocol_message_t resp_msg;
    if (deserialize_message(response, response_len, &resp_msg) == 0) {
        printf("[Test] Create channel response: type=%d\n", resp_msg.header.type);
        if (resp_msg.header.type == MSG_CREATE_CHANNEL) {
            printf("[Test] Channel ID: ");
            for (int i = 0; i < 32; i++) {
                printf("%02x", resp_msg.body.create_channel.channel_id[i]);
            }
            printf("\n");
        }
    }
    
    return 0;
}


static int test_compute_request(int sock) {
    printf("\n=== Test Compute Request ===\n");
    
    protocol_message_t msg;
    memset(&msg, 0, sizeof(msg));
    msg.header.type = MSG_COMPUTE_REQUEST;
    msg.header.version = 1;
    msg.header.timestamp = time(NULL);
    
    
    uint8_t channel_id[32] = {0x11, 0x22, 0x33, 0x44};
    memcpy(msg.body.compute_request.channel_id, channel_id, 32);
    
    const char* request_data = "Compute: 1 + 1";
    size_t request_len = strlen(request_data);
    if (request_len > MAX_REQUEST_SIZE) {
        request_len = MAX_REQUEST_SIZE;
    }
    memcpy(msg.body.compute_request.request_data, request_data, request_len);
    msg.body.compute_request.request_len = request_len;
    msg.body.compute_request.payment_amount = 10000; 
    
    
    uint8_t buffer[BUFFER_SIZE];
    size_t buffer_len = sizeof(buffer);
    if (serialize_message(&msg, buffer, &buffer_len) != 0) {
        printf("[Test] Failed to serialize compute request message\n");
        return -1;
    }
    
    
    uint8_t response[BUFFER_SIZE];
    size_t response_len = sizeof(response);
    if (send_and_receive(sock, buffer, buffer_len, response, &response_len) != 0) {
        return -1;
    }
    
    
    protocol_message_t resp_msg;
    if (deserialize_message(response, response_len, &resp_msg) == 0) {
        printf("[Test] Compute request response: type=%d\n", resp_msg.header.type);
    }
    
    return 0;
}


static int test_release_deposit(int sock) {
    printf("\n=== Test Release Deposit ===\n");
    
    protocol_message_t msg;
    memset(&msg, 0, sizeof(msg));
    msg.header.type = MSG_RELEASE_DEPOSIT;
    msg.header.version = 1;
    msg.header.timestamp = time(NULL);
    
    
    uint8_t user_address[20] = {0x01, 0x02, 0x03, 0x04, 0x05};
    memcpy(msg.body.deposit.user_address, user_address, 20);
    msg.body.deposit.amount = 100000; 
    
    
    uint8_t buffer[BUFFER_SIZE];
    size_t buffer_len = sizeof(buffer);
    if (serialize_message(&msg, buffer, &buffer_len) != 0) {
        printf("[Test] Failed to serialize release deposit message\n");
        return -1;
    }
    
    
    uint8_t response[BUFFER_SIZE];
    size_t response_len = sizeof(response);
    if (send_and_receive(sock, buffer, buffer_len, response, &response_len) != 0) {
        return -1;
    }
    
    
    protocol_message_t resp_msg;
    if (deserialize_message(response, response_len, &resp_msg) == 0) {
        printf("[Test] Release deposit response: type=%d\n", resp_msg.header.type);
    }
    
    return 0;
}


int main(int argc, char* argv[]) {
    printf("=== FlashPay Test Client ===\n\n");
    
    const char* server_ip = "127.0.0.1";
    if (argc > 1) {
        server_ip = argv[1];
    }
    
    
    int utee_sock = connect_to_server(server_ip, U_TEE_PORT);
    if (utee_sock < 0) {
        printf("Failed to connect to U-TEE\n");
        return 1;
    }
    
    
    int failed = 0;
    
    if (test_deposit(utee_sock) != 0) {
        printf("[Test] Deposit test failed\n");
        failed++;
    } else {
        printf("[Test] Deposit test passed\n");
    }
    
    sleep(1); 
    
    if (test_create_channel(utee_sock) != 0) {
        printf("[Test] Create channel test failed\n");
        failed++;
    } else {
        printf("[Test] Create channel test passed\n");
    }
    
    sleep(1);
    
    if (test_compute_request(utee_sock) != 0) {
        printf("[Test] Compute request test failed\n");
        failed++;
    } else {
        printf("[Test] Compute request test passed\n");
    }
    
    sleep(1);
    
    if (test_release_deposit(utee_sock) != 0) {
        printf("[Test] Release deposit test failed\n");
        failed++;
    } else {
        printf("[Test] Release deposit test passed\n");
    }
    
    close(utee_sock);
    
    printf("\n=== Test Summary ===\n");
    printf("Total tests: 4\n");
    printf("Failed: %d\n", failed);
    printf("Passed: %d\n", 4 - failed);
    
    return (failed > 0) ? 1 : 0;
}

