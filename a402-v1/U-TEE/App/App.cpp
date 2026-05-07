#include "App.h"
#include "Enclave_u.h"
#include <stdio.h>
#include <string.h>
#include <unistd.h>


void ocall_send_to_mtee(const uint8_t* data, size_t len) {
    printf("[U-TEE App] Sending %zu bytes to M-TEE\n", len);
}

void ocall_receive_from_mtee(uint8_t* data, size_t* len) {
    *len = 0;
    printf("[U-TEE App] Receiving from M-TEE\n");
}

void ocall_send_onchain_tx(const uint8_t* tx_data, size_t len) {
    printf("[U-TEE App] Sending on-chain transaction (%zu bytes)\n", len);
}

void ocall_get_onchain_tx(const uint8_t* tx_hash, uint8_t* tx_data, size_t* len) {
    *len = 0;
    printf("[U-TEE App] Getting on-chain transaction\n");
}

void ocall_print(const char* str) {
    printf("%s", str);
}

int main(int argc, char* argv[]) {
    printf("U-TEE Payment Channel System\n");
    
    
    sgx_enclave_id_t eid;
    sgx_status_t ret = sgx_create_enclave(
        "Enclave/enclave.signed.so",
        SGX_DEBUG_FLAG,
        NULL, NULL,
        &eid, NULL
    );
    
    if (ret != SGX_SUCCESS) {
        printf("Failed to create enclave: %d\n", ret);
        return 1;
    }
    
    
    uint8_t sk[32] = {0};
    uint8_t pk[33] = {0};
    
    ret = init_utee(eid, sk, pk);
    if (ret != SGX_SUCCESS) {
        printf("Failed to initialize U-TEE: %d\n", ret);
        sgx_destroy_enclave(eid);
        return 1;
    }
    
    printf("U-TEE initialized successfully\n");
    
    
    
    
    sgx_destroy_enclave(eid);
    return 0;
}

