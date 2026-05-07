#ifndef ADAPTER_SIGNATURE_H
#define ADAPTER_SIGNATURE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif


#define SECP256K1_PUBKEY_SIZE 33
#define SECP256K1_PRIVKEY_SIZE 32
#define SECP256K1_SIGNATURE_SIZE 64
#define ADAPTER_POINT_SIZE 33  


typedef struct {
    uint8_t secret_t[32];           
    uint8_t adapter_point_T[ADAPTER_POINT_SIZE];  
    uint8_t signature[SECP256K1_SIGNATURE_SIZE];   
} adapter_signature_ctx_t;



int generate_adapter_signature(
    const uint8_t* privkey,          
    const uint8_t* message,          
    size_t message_len,              
    adapter_signature_ctx_t* ctx     
);



int verify_adapter_signature(
    const uint8_t* pubkey,           
    const uint8_t* message,          
    size_t message_len,              
    const uint8_t* signature,        
    const uint8_t* adapter_point_T   
);



int extract_secret_from_tx(
    const uint8_t* tx_data,          
    size_t tx_len,                   
    const uint8_t* adapter_point_T,  
    uint8_t* secret_t                
);

#ifdef __cplusplus
}
#endif

#endif 

