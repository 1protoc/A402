#include "response_storage.h"
#include <string.h>
#include <stdlib.h>
#include <time.h>

static stored_response_t g_stored_responses[MAX_STORED_RESPONSES];
static int g_stored_count = 0;

int store_response(
    const uint8_t* channel_id,
    const uint8_t* encrypted_response,
    size_t encrypted_len,
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    const uint8_t* adapter_point_T,
    const uint8_t* tag)
{
    if (!channel_id || !encrypted_response || !tx_pay || !adapter_point_T || !tag) {
        return -1;
    }
    
    
    for (int i = 0; i < g_stored_count; i++) {
        if (g_stored_responses[i].valid && 
            memcmp(g_stored_responses[i].channel_id, channel_id, 32) == 0) {
            
            stored_response_t* resp = &g_stored_responses[i];
            memcpy(resp->encrypted_response, encrypted_response, encrypted_len);
            resp->encrypted_len = encrypted_len;
            memcpy(resp->tx_pay, tx_pay, tx_pay_len);
            resp->tx_pay_len = tx_pay_len;
            memcpy(resp->adapter_point_T, adapter_point_T, 33);
            memcpy(resp->tag, tag, 16);
            resp->timestamp = time(NULL);
            return 0;
        }
    }
    
    
    if (g_stored_count >= MAX_STORED_RESPONSES) {
        return -1; 
    }
    
    stored_response_t* resp = &g_stored_responses[g_stored_count++];
    memcpy(resp->channel_id, channel_id, 32);
    memcpy(resp->encrypted_response, encrypted_response, encrypted_len);
    resp->encrypted_len = encrypted_len;
    memcpy(resp->tx_pay, tx_pay, tx_pay_len);
    resp->tx_pay_len = tx_pay_len;
    memcpy(resp->adapter_point_T, adapter_point_T, 33);
    memcpy(resp->tag, tag, 16);
    resp->timestamp = time(NULL);
    resp->valid = 1;
    
    return 0;
}

stored_response_t* get_stored_response(const uint8_t* channel_id) {
    if (!channel_id) {
        return NULL;
    }
    
    for (int i = 0; i < g_stored_count; i++) {
        if (g_stored_responses[i].valid && 
            memcmp(g_stored_responses[i].channel_id, channel_id, 32) == 0) {
            return &g_stored_responses[i];
        }
    }
    
    return NULL;
}

int remove_stored_response(const uint8_t* channel_id) {
    if (!channel_id) {
        return -1;
    }
    
    for (int i = 0; i < g_stored_count; i++) {
        if (g_stored_responses[i].valid && 
            memcmp(g_stored_responses[i].channel_id, channel_id, 32) == 0) {
            g_stored_responses[i].valid = 0;
            return 0;
        }
    }
    
    return -1;
}


