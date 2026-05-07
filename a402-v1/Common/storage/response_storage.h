#ifndef RESPONSE_STORAGE_H
#define RESPONSE_STORAGE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define MAX_STORED_RESPONSES 100


typedef struct {
    uint8_t channel_id[32];
    uint8_t encrypted_response[4096];
    size_t encrypted_len;
    uint8_t tx_pay[256];
    size_t tx_pay_len;
    uint8_t adapter_point_T[33];
    uint8_t tag[16];
    uint64_t timestamp;
    int valid;
} stored_response_t;


int store_response(
    const uint8_t* channel_id,
    const uint8_t* encrypted_response,
    size_t encrypted_len,
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    const uint8_t* adapter_point_T,
    const uint8_t* tag
);


stored_response_t* get_stored_response(const uint8_t* channel_id);


int remove_stored_response(const uint8_t* channel_id);

#ifdef __cplusplus
}
#endif

#endif 


