

#include "host_vm_app.h"
#include "mtee_network.h"
#include "../GuestVM/mtee_guest.h"
#include "../../Common/utils/logger.h"
#include "../../Common/crypto/key_exchange.h"
#include "../../blockchain/Common/blockchain_client.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <signal.h>
#include <unistd.h>


static bool g_guest_initialized = false;


static bool g_blockchain_initialized = false;
static char g_contract_address[64] = "";

int mtee_host_init(const uint8_t* sk, const uint8_t* pk) {
    if (g_guest_initialized) {
        return 0;
    }
    
    
    
    
    if (mtee_guest_init(sk, pk) != 0) {
        printf("Failed to initialize M-TEE Guest VM\n");
        return -1;
    }
    
    
    extern void mtee_set_guest_function_pointer(int (*)(const uint8_t*, const uint8_t*, size_t, uint64_t));
    mtee_set_guest_function_pointer(mtee_handle_compute_request);
    
    g_guest_initialized = true;
    printf("[M-TEE Host VM] Initialized\n");
    return 0;
}

int mtee_host_generate_and_send_tx(
    const uint8_t* tx_pay,
    size_t tx_pay_len,
    const uint8_t* signature,
    const uint8_t* adapter_point_T,
    uint8_t* tx_data,
    size_t* tx_len)
{
    if (!g_blockchain_initialized) {
        LOG_ERROR("Blockchain not initialized");
        return -1;
    }
    
    
    
    
    
    if (*tx_len < tx_pay_len + 64 + 33) {
        LOG_ERROR("Output buffer too small");
        return -1;
    }
    
    memcpy(tx_data, tx_pay, tx_pay_len);
    memcpy(tx_data + tx_pay_len, signature, 64);
    memcpy(tx_data + tx_pay_len + 64, adapter_point_T, 33);
    
    
    char hex_data[2048];
    size_t total_len = tx_pay_len + 64 + 33;
    for (size_t i = 0; i < total_len && i < sizeof(hex_data) / 2 - 1; i++) {
        sprintf(hex_data + i * 2, "%02x", tx_data[i]);
    }
    hex_data[total_len * 2] = '\0';
    
    
    tx_result_t result;
    int ret = ethereum_send_raw_transaction(
        g_contract_address,
        hex_data,
        0,  
        &result
    );
    
    if (ret == 0 && result.success) {
        LOG_INFO("Transaction sent successfully: %s", result.txid);
        *tx_len = total_len;
    return 0;
    } else {
        LOG_ERROR("Failed to send transaction: %s", result.error);
        *tx_len = 0;
        return -1;
    }
}

int main(int argc, char* argv[]) {
    
    logger_init(LOG_INFO);
    
    LOG_INFO("=== M-TEE Host VM Computation Service ===");
    
    
    uint8_t sk[32] = {0};
    uint8_t pk[33] = {0};
    
    if (generate_keypair(sk, pk) != 0) {
        LOG_ERROR("Failed to generate keypair");
        return 1;
    }
    
    if (mtee_host_init(sk, pk) != 0) {
        LOG_ERROR("Failed to initialize M-TEE");
        return 1;
    }
    
    LOG_INFO("M-TEE initialized successfully");
    
    
    uint16_t port = 8081; 
    if (mtee_network_init(port) != 0) {
        LOG_ERROR("Failed to initialize network server");
        return 1;
    }
    
    
    if (mtee_network_start() != 0) {
        LOG_ERROR("Failed to start network server");
        return 1;
    }
    
    LOG_INFO("M-TEE network server started on port %d", port);
    LOG_INFO("Press Ctrl+C to stop...");
    
    
    while (1) {
        sleep(1);
    }
    
    return 0;
}

