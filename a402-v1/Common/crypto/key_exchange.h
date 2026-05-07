#ifndef KEY_EXCHANGE_H
#define KEY_EXCHANGE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SHARED_KEY_SIZE 32




int generate_keypair(uint8_t* privkey, uint8_t* pubkey);


int compute_shared_secret(
    const uint8_t* my_privkey,
    const uint8_t* peer_pubkey,
    uint8_t* shared_secret
);

#ifdef __cplusplus
}
#endif

#endif 


