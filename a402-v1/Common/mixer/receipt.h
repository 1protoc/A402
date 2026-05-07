#ifndef RECEIPT_H
#define RECEIPT_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif


typedef struct {
    uint8_t cid[32];             
    uint8_t rid[32];             
    uint64_t amount;             
    uint64_t timestamp;          
    uint8_t signature[64];       
} receipt_t;


int generate_receipt(
    const uint8_t* cid,
    const uint8_t* rid,
    uint64_t amount,
    const uint8_t* u_tee_sk,
    receipt_t* receipt
);


int verify_receipt(
    const receipt_t* receipt,
    const uint8_t* u_tee_pubkey
);

#ifdef __cplusplus
}
#endif

#endif 
