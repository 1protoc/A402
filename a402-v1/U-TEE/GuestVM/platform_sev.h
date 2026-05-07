#ifndef PLATFORM_SEV_H
#define PLATFORM_SEV_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif




int platform_sev_init(void);


void platform_sha256(const uint8_t* data, size_t len, uint8_t* hash);


void* platform_malloc(size_t size);
void platform_free(void* ptr);


int platform_get_random(uint8_t* buffer, size_t len);


int platform_sign_message(const uint8_t* message, size_t message_len, 
                          const uint8_t* privkey, uint8_t* signature);


int platform_verify_signature(const uint8_t* message, size_t message_len,
                              const uint8_t* pubkey, const uint8_t* signature);


int platform_verify_page(void* page_addr);


int platform_get_certificate(uint8_t* cert, size_t* cert_len);

#ifdef __cplusplus
}
#endif

#endif 

