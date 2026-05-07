#include "receipt.h"
#include <string.h>
#include <time.h>


extern int platform_sign_message(const uint8_t* message, size_t message_len,
                                  const uint8_t* privkey, uint8_t* signature);
extern int platform_verify_signature(const uint8_t* message, size_t message_len,
                                     const uint8_t* pubkey, const uint8_t* signature);

int generate_receipt(
    const uint8_t* cid,
    const uint8_t* rid,
    uint64_t amount,
    const uint8_t* u_tee_sk,
    receipt_t* receipt)
{
    if (!cid || !rid || !u_tee_sk || !receipt) {
        return -1;
    }
    
    memcpy(receipt->cid, cid, 32);
    memcpy(receipt->rid, rid, 32);
    receipt->amount = amount;
    receipt->timestamp = time(NULL);
    
    
    uint8_t sign_data[64];
    memcpy(sign_data, cid, 32);
    memcpy(sign_data + 32, rid, 32);
    
    if (platform_sign_message(sign_data, 64, u_tee_sk, receipt->signature) != 0) {
        return -1;
    }
    
    return 0;
}

int verify_receipt(
    const receipt_t* receipt,
    const uint8_t* u_tee_pubkey)
{
    if (!receipt || !u_tee_pubkey) {
        return -1;
    }
    
    
    uint8_t sign_data[64];
    memcpy(sign_data, receipt->cid, 32);
    memcpy(sign_data + 32, receipt->rid, 32);
    
    if (platform_verify_signature(sign_data, 64, u_tee_pubkey, receipt->signature) != 0) {
        return -1;
    }
    
    return 0;
}
