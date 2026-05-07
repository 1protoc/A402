

#include "host_vm_mediator.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#define MAX_VMS 10


static guest_vm_info_t g_vm_registry[MAX_VMS];
static int g_vm_count = 0;
static bool g_mediator_initialized = false;

int host_vm_mediator_init(void) {
    if (g_mediator_initialized) {
        return 0;
    }
    
    memset(g_vm_registry, 0, sizeof(g_vm_registry));
    g_vm_count = 0;
    g_mediator_initialized = true;
    
    printf("[Host VM Mediator] Initialized\n");
    return 0;
}

int host_vm_mediator_register_vm(uint32_t vm_id, void* vm_handle) {
    if (!g_mediator_initialized) {
        return -1;
    }
    
    if (g_vm_count >= MAX_VMS) {
        return -1;
    }
    
    
    for (int i = 0; i < g_vm_count; i++) {
        if (g_vm_registry[i].vm_id == vm_id) {
            
            g_vm_registry[i].vm_handle = vm_handle;
            g_vm_registry[i].is_active = true;
            printf("[Host VM Mediator] Updated VM %u\n", vm_id);
            return 0;
        }
    }
    
    
    g_vm_registry[g_vm_count].vm_id = vm_id;
    g_vm_registry[g_vm_count].vm_handle = vm_handle;
    g_vm_registry[g_vm_count].is_active = true;
    g_vm_count++;
    
    printf("[Host VM Mediator] Registered VM %u\n", vm_id);
    return 0;
}

int host_vm_mediator_forward_message(uint32_t src_vm_id,
                                     uint32_t dst_vm_id,
                                     const uint8_t* data,
                                     size_t len)
{
    if (!g_mediator_initialized || !data) {
        return -1;
    }
    
    
    guest_vm_info_t* dst_vm = NULL;
    for (int i = 0; i < g_vm_count; i++) {
        if (g_vm_registry[i].vm_id == dst_vm_id && g_vm_registry[i].is_active) {
            dst_vm = &g_vm_registry[i];
            break;
        }
    }
    
    if (!dst_vm) {
        printf("[Host VM Mediator] VM %u not found or inactive\n", dst_vm_id);
        return -1;
    }
    
    
    printf("[Host VM Mediator] Forwarding %zu bytes from VM %u to VM %u\n", 
           len, src_vm_id, dst_vm_id);
    
    return 0;
}

int host_vm_mediator_broadcast(uint32_t src_vm_id,
                               const uint8_t* data,
                               size_t len)
{
    if (!g_mediator_initialized || !data) {
        return -1;
    }
    
    int success_count = 0;
    for (int i = 0; i < g_vm_count; i++) {
        if (g_vm_registry[i].vm_id != src_vm_id && g_vm_registry[i].is_active) {
            if (host_vm_mediator_forward_message(src_vm_id, 
                                                  g_vm_registry[i].vm_id,
                                                  data, len) == 0) {
                success_count++;
            }
        }
    }
    
    printf("[Host VM Mediator] Broadcast to %d VMs\n", success_count);
    return success_count;
}

int host_vm_mediator_receive_from_vm(uint32_t vm_id,
                                      uint8_t* buffer,
                                      size_t* buffer_len)
{
    if (!g_mediator_initialized || !buffer || !buffer_len) {
        return -1;
    }
    
    
    *buffer_len = 0;
    return 0;
}

void host_vm_mediator_close(void) {
    g_mediator_initialized = false;
    g_vm_count = 0;
    memset(g_vm_registry, 0, sizeof(g_vm_registry));
    printf("[Host VM Mediator] Closed\n");
}


