#include "App.h"
#include "Enclave_u.h"
#include <stdio.h>
#include <string.h>
#include <unistd.h>


void ocall_send_to_utee(const uint8_t* data, size_t len) {
    printf("[M-TEE App] Sending %zu bytes to U-TEE\n", len);
}

void ocall_receive_from_utee(uint8_t* data, size_t* len) {
    *len = 0;
    printf("[M-TEE App] Receiving from U-TEE\n");
}

void ocall_generate_and_send_tx(
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    const uint8_t* signature,
    const uint8_t* adapter_point_T,
    uint8_t* tx_data,
    size_t* tx_len)
{
    
    
    
    
    printf("[M-TEE App] Generating and sending transaction\n");
    *tx_len = 0;
}

void ocall_print(const char* str) {
    printf("%s", str);
}

int main(int argc, char* argv[]) {
    printf("M-TEE Computation Service\n");
    
    
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
    
    ret = init_mtee(eid, sk, pk);
    if (ret != SGX_SUCCESS) {
        printf("Failed to initialize M-TEE: %d\n", ret);
        sgx_destroy_enclave(eid);
        return 1;
    }
    
    printf("M-TEE initialized successfully\n");
    
    
    
    
    sgx_destroy_enclave(eid);
    return 0;
}

