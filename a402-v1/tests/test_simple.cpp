

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

#define U_TEE_PORT 8080
#define BUFFER_SIZE 1024

int main(int argc, char* argv[]) {
    const char* server_ip = "127.0.0.1";
    if (argc > 1) {
        server_ip = argv[1];
    }
    
    printf("=== Simple Connection Test ===\n");
    printf("Connecting to %s:%d...\n", server_ip, U_TEE_PORT);
    
    
    int sock = socket(AF_INET, SOCK_STREAM, 0);
    if (sock < 0) {
        perror("socket");
        return 1;
    }
    
    
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons(U_TEE_PORT);
    
    if (inet_pton(AF_INET, server_ip, &addr.sin_addr) <= 0) {
        perror("inet_pton");
        close(sock);
        return 1;
    }
    
    
    if (connect(sock, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        perror("connect");
        printf("\nERROR: Cannot connect to server.\n");
        printf("Make sure U-TEE server is running:\n");
        printf("  cd U-TEE/HostVM && ./utee_host_app\n");
        close(sock);
        return 1;
    }
    
    printf("Connected successfully!\n");
    
    
    const char* test_msg = "Hello from test client";
    ssize_t sent = send(sock, test_msg, strlen(test_msg), 0);
    if (sent < 0) {
        perror("send");
    } else {
        printf("Sent %zd bytes: %s\n", sent, test_msg);
    }
    
    
    char buffer[BUFFER_SIZE];
    ssize_t received = recv(sock, buffer, sizeof(buffer) - 1, 0);
    if (received < 0) {
        perror("recv");
    } else if (received > 0) {
        buffer[received] = '\0';
        printf("Received %zd bytes: %s\n", received, buffer);
    } else {
        printf("Server closed connection\n");
    }
    
    close(sock);
    printf("\nTest completed.\n");
    return 0;
}


