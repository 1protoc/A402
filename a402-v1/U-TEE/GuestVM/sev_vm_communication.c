#include "sev_vm_communication.h"
#include "platform_sev.h"
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <stdbool.h>


static struct {
    uint32_t vm_id;
    sev_comm_channel_type_t channel_type;
    bool initialized;
    sev_vm_message_callback_t callback;
    void* channel_handle;
} g_comm_state = {0};


static int send_via_host(uint32_t dst_vm_id, const uint8_t* data, size_t len) {
    
    printf("[VM Comm] Sending %zu bytes to VM %u via Host\n", len, dst_vm_id);
    return 0;
}

int sev_vm_comm_init(uint32_t vm_id, sev_comm_channel_type_t channel_type) {
    if (g_comm_state.initialized) {
        return 0;
    }
    
    g_comm_state.vm_id = vm_id;
    g_comm_state.channel_type = channel_type;
    g_comm_state.initialized = true;
    
    printf("[VM Comm] Initialized VM %u with channel type %d\n", vm_id, channel_type);
    return 0;
}

int sev_vm_comm_send(uint32_t dst_vm_id, const uint8_t* data, size_t len) {
    if (!g_comm_state.initialized) {
        return -1;
    }
    
    switch (g_comm_state.channel_type) {
        case SEV_COMM_CHANNEL_HOST_MEDIATED:
            return send_via_host(dst_vm_id, data, len);
        case SEV_COMM_CHANNEL_SHARED_MEMORY:
            return -1;
        case SEV_COMM_CHANNEL_VIRTUAL_NET:
            return -1;
        default:
            return -1;
    }
}

int sev_vm_comm_receive(uint8_t* buffer, size_t* buffer_len, uint32_t* src_vm_id) {
    if (!g_comm_state.initialized || !buffer || !buffer_len) {
        return -1;
    }
    
    *buffer_len = 0;
    return 0;
}

int sev_vm_comm_register_callback(sev_vm_message_callback_t callback) {
    if (!callback) {
        return -1;
    }
    
    g_comm_state.callback = callback;
    return 0;
}

void sev_vm_comm_close(void) {
    g_comm_state.initialized = false;
    g_comm_state.callback = NULL;
}

