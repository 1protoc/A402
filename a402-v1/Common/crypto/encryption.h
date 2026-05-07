#ifndef ENCRYPTION_H
#define ENCRYPTION_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define AES_KEY_SIZE 32
#define AES_IV_SIZE 16
#define MAX_ENCRYPTED_SIZE 4096



int encrypt_data(
    const uint8_t* plaintext,        
    size_t plaintext_len,            
    const uint8_t* key,              
    const uint8_t* iv,               
    uint8_t* ciphertext,             
    size_t ciphertext_max_len,       
    uint8_t* tag                     
);



int decrypt_data(
    const uint8_t* ciphertext,       
    size_t ciphertext_len,           
    const uint8_t* key,              
    const uint8_t* iv,               
    const uint8_t* tag,              
    uint8_t* plaintext,              
    size_t plaintext_max_len         
);


int encrypt_with_shared_key(
    const uint8_t* plaintext,
    size_t plaintext_len,
    const uint8_t* shared_key_sk_m,  
    uint8_t* ciphertext,
    size_t ciphertext_max_len,
    uint8_t* tag
);


int decrypt_with_shared_key(
    const uint8_t* ciphertext,
    size_t ciphertext_len,
    const uint8_t* shared_key_sk_m,
    const uint8_t* tag,
    uint8_t* plaintext,
    size_t plaintext_max_len
);

#ifdef __cplusplus
}
#endif

#endif 

