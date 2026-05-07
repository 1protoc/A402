#include "commitment.h"
#include <openssl/sha.h>
#include <string.h>

int generate_commitment(
    const uint8_t* pk,
    const uint8_t* codehash,
    const uint8_t* attestation,
    size_t att_len,
    uint8_t* commitment)
{
    if (!pk || !codehash || !attestation || !commitment) {
        return -1;
    }
    
    
    SHA256_CTX ctx;
    SHA256_Init(&ctx);
    SHA256_Update(&ctx, pk, 33);
    SHA256_Update(&ctx, codehash, 32);
    SHA256_Update(&ctx, attestation, att_len);
    SHA256_Final(commitment, &ctx);
    
    return 0;
}

int verify_commitment(
    const uint8_t* pk,
    const uint8_t* codehash,
    const uint8_t* attestation,
    size_t att_len,
    const uint8_t* commitment)
{
    if (!pk || !codehash || !attestation || !commitment) {
        return -1;
    }
    
    uint8_t computed[32];
    if (generate_commitment(pk, codehash, attestation, att_len, computed) != 0) {
        return -1;
    }
    
    return memcmp(computed, commitment, 32) == 0 ? 0 : -1;
}
