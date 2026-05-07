#ifndef COMMITMENT_H
#define COMMITMENT_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif


int generate_commitment(
    const uint8_t* pk,           
    const uint8_t* codehash,     
    const uint8_t* attestation,  
    size_t att_len,
    uint8_t* commitment          
);


int verify_commitment(
    const uint8_t* pk,
    const uint8_t* codehash,
    const uint8_t* attestation,
    size_t att_len,
    const uint8_t* commitment
);

#ifdef __cplusplus
}
#endif

#endif 
