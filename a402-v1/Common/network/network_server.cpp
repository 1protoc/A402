#include "network_server.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <pthread.h>
#include <errno.h>

#define MAX_CLIENTS 10
#define BUFFER_SIZE 4096

static int g_server_fd = -1;
static uint16_t g_port = 0;
static bool g_running = false;
static pthread_t g_server_thread;
static message_handler_t g_message_handler = NULL;


typedef struct {
    int fd;
    struct sockaddr_in addr;
    bool active;
} client_connection_t;

static client_connection_t g_clients[MAX_CLIENTS];
static int g_client_count = 0;


static void* handle_client(void* arg) {
    client_connection_t* client = (client_connection_t*)arg;
    uint8_t buffer[BUFFER_SIZE];
    uint8_t response[BUFFER_SIZE];
    
    printf("[Network] Client connected from %s:%d\n",
           inet_ntoa(client->addr.sin_addr), ntohs(client->addr.sin_port));
    
    while (client->active && g_running) {
        ssize_t recv_len = recv(client->fd, buffer, sizeof(buffer), 0);
        
        if (recv_len <= 0) {
            if (recv_len == 0) {
                printf("[Network] Client disconnected\n");
            } else {
                perror("recv");
            }
            break;
        }
        
        
        if (g_message_handler) {
            size_t response_len = sizeof(response);
            int ret = g_message_handler(buffer, recv_len, response, &response_len);
            
            if (ret == 0 && response_len > 0) {
                send(client->fd, response, response_len, 0);
            }
        }
    }
    
    close(client->fd);
    client->active = false;
    return NULL;
}


static void* server_main_loop(void* arg) {
    struct sockaddr_in client_addr;
    socklen_t client_len = sizeof(client_addr);
    
    printf("[Network] Server started on port %d\n", g_port);
    
    while (g_running) {
        int client_fd = accept(g_server_fd, (struct sockaddr*)&client_addr, &client_len);
        
        if (client_fd < 0) {
            if (g_running) {
                perror("accept");
            }
            continue;
        }
        
        
        int client_idx = -1;
        for (int i = 0; i < MAX_CLIENTS; i++) {
            if (!g_clients[i].active) {
                client_idx = i;
                break;
            }
        }
        
        if (client_idx >= 0) {
            g_clients[client_idx].fd = client_fd;
            g_clients[client_idx].addr = client_addr;
            g_clients[client_idx].active = true;
            
            pthread_t thread;
            if (pthread_create(&thread, NULL, handle_client, &g_clients[client_idx]) != 0) {
                close(client_fd);
                g_clients[client_idx].active = false;
            } else {
                pthread_detach(thread);
            }
        } else {
            printf("[Network] Too many clients, rejecting connection\n");
            close(client_fd);
        }
    }
    
    return NULL;
}

int network_server_init(uint16_t port) {
    if (g_server_fd >= 0) {
        return 0; 
    }
    
    g_port = port;
    
    
    g_server_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (g_server_fd < 0) {
        perror("socket");
        return -1;
    }
    
    
    int opt = 1;
    if (setsockopt(g_server_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt)) < 0) {
        perror("setsockopt");
        close(g_server_fd);
        g_server_fd = -1;
        return -1;
    }
    
    
    struct sockaddr_in server_addr;
    memset(&server_addr, 0, sizeof(server_addr));
    server_addr.sin_family = AF_INET;
    server_addr.sin_addr.s_addr = INADDR_ANY;
    server_addr.sin_port = htons(port);
    
    if (bind(g_server_fd, (struct sockaddr*)&server_addr, sizeof(server_addr)) < 0) {
        perror("bind");
        close(g_server_fd);
        g_server_fd = -1;
        return -1;
    }
    
    
    if (listen(g_server_fd, MAX_CLIENTS) < 0) {
        perror("listen");
        close(g_server_fd);
        g_server_fd = -1;
        return -1;
    }
    
    memset(g_clients, 0, sizeof(g_clients));
    g_client_count = 0;
    
    printf("[Network] Server initialized on port %d\n", port);
    return 0;
}

int network_server_start(message_handler_t handler) {
    if (g_server_fd < 0) {
        return -1;
    }
    
    if (g_running) {
        return 0; 
    }
    
    g_message_handler = handler;
    g_running = true;
    
    if (pthread_create(&g_server_thread, NULL, server_main_loop, NULL) != 0) {
        g_running = false;
        return -1;
    }
    
    return 0;
}

void network_server_stop(void) {
    if (!g_running) {
        return;
    }
    
    g_running = false;
    
    
    if (g_server_fd >= 0) {
        shutdown(g_server_fd, SHUT_RDWR);
        close(g_server_fd);
        g_server_fd = -1;
    }
    
    
    for (int i = 0; i < MAX_CLIENTS; i++) {
        if (g_clients[i].active) {
            close(g_clients[i].fd);
            g_clients[i].active = false;
        }
    }
    
    
    if (g_server_thread) {
        pthread_join(g_server_thread, NULL);
    }
    
    printf("[Network] Server stopped\n");
}

int network_server_send_response(int client_fd, const uint8_t* data, size_t len) {
    if (client_fd < 0 || !data || len == 0) {
        return -1;
    }
    
    ssize_t sent = send(client_fd, data, len, 0);
    if (sent < 0) {
        perror("send");
        return -1;
    }
    
    return 0;
}


