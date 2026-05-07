

#include "host_vm_app.h"
#include "utee_network.h"
#include "../GuestVM/utee_guest.h"
#include "../../Common/utils/logger.h"
#include "../../Common/crypto/key_exchange.h"
#include "../../blockchain/Common/blockchain_client.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <signal.h>
#include <unistd.h>
#include <sstream>
#include <iomanip>


static bool g_guest_initialized = false;


static int invoke_guest_function(void* params, size_t param_size, void* result, size_t* result_size) {
    if (!g_guest_initialized) {
        return -1;
    }
    
    
    
    
    return 0;
}

int utee_host_init(const uint8_t* sk, const uint8_t* pk) {
    if (g_guest_initialized) {
        return 0;
    }
    
    
    
    
    if (utee_guest_init(sk, pk) != 0) {
        printf("Failed to initialize U-TEE Guest VM\n");
        return -1;
    }
    
    g_guest_initialized = true;
    printf("[U-TEE Host VM] Initialized\n");
    return 0;
}

int utee_host_handle_deposit(const uint8_t* user_address, uint64_t amount, const uint8_t* tx_hash) {
    if (!g_guest_initialized) {
        return -1;
    }
    
    
    return utee_handle_deposit(user_address, amount, tx_hash);
}

int utee_host_release_deposit(const uint8_t* user_address, uint64_t amount) {
    if (!g_guest_initialized) {
        return -1;
    }
    
    
    return utee_release_deposit(user_address, amount);
}

int utee_host_create_channel(const uint8_t* m_tee_address, uint64_t amount, uint8_t* channel_id) {
    if (!g_guest_initialized) {
        return -1;
    }
    
    
    return utee_create_channel(m_tee_address, amount, channel_id);
}

int utee_host_handle_compute_request(
    const uint8_t* channel_id,
    const uint8_t* request_data,
    size_t request_len,
    uint64_t payment_amount)
{
    if (!g_guest_initialized) {
        return -1;
    }
    
    
    return utee_handle_compute_request(channel_id, request_data, request_len, payment_amount);
}


static bool g_blockchain_initialized = false;
static char g_contract_address[64] = "";

int utee_host_send_onchain_tx(const uint8_t* tx_data, size_t len) {
    if (!g_blockchain_initialized) {
        LOG_ERROR("Blockchain not initialized");
        return -1;
    }
    
    
    char hex_data[2048];
    for (size_t i = 0; i < len && i < sizeof(hex_data) / 2 - 1; i++) {
        sprintf(hex_data + i * 2, "%02x", tx_data[i]);
    }
    hex_data[len * 2] = '\0';
    
    
    tx_result_t result;
    int ret = ethereum_send_raw_transaction(
        g_contract_address,
        hex_data,
        0,  
        &result
    );
    
    if (ret == 0 && result.success) {
        LOG_INFO("Transaction sent successfully: %s", result.txid);
        return 0;
    } else {
        LOG_ERROR("Failed to send transaction: %s", result.error);
        return -1;
    }
}

int utee_host_get_onchain_tx(const uint8_t* tx_hash, uint8_t* tx_data, size_t* len) {
    if (!g_blockchain_initialized) {
        LOG_ERROR("Blockchain not initialized");
        return -1;
    }
    
    
    char tx_hash_str[128];
    for (int i = 0; i < 32; i++) {
        sprintf(tx_hash_str + i * 2, "%02x", tx_hash[i]);
    }
    tx_hash_str[64] = '\0';
    
    
    size_t data_len = *len;
    int ret = ethereum_get_transaction_data(tx_hash_str, tx_data, &data_len);
    
    if (ret == 0) {
        *len = data_len;
        LOG_INFO("Transaction data retrieved: %zu bytes", data_len);
        return 0;
    } else {
        LOG_ERROR("Failed to get transaction data");
        *len = 0;
        return -1;
    }
}

int main(int argc, char* argv[]) {
    
    logger_init(LOG_INFO);
    
    LOG_INFO("=== U-TEE Host VM Payment Channel System ===");
    
    
    const char* contract_addr = getenv("PAYMENT_CHANNEL_CONTRACT");
    if (!contract_addr) {
        
        FILE* f = fopen("../../blockchain/ethereum/deployment.json", "r");
        if (f) {
            char line[256];
            while (fgets(line, sizeof(line), f)) {
                if (strstr(line, "\"address\"")) {
                    char* addr_start = strstr(line, "\"0x");
                    if (addr_start) {
                        strncpy(g_contract_address, addr_start + 1, 42);
                        g_contract_address[42] = '\0';
                        contract_addr = g_contract_address;
                    }
                }
            }
            fclose(f);
        }
    } else {
        strncpy(g_contract_address, contract_addr, sizeof(g_contract_address) - 1);
    }
    
    if (contract_addr && strlen(contract_addr) > 0) {
        const char* rpc_url = getenv("ETH_RPC_URL");
        if (!rpc_url) {
            rpc_url = "http://127.0.0.1:8545";
        }
        
        if (ethereum_init(rpc_url, contract_addr, nullptr) == 0) {
            g_blockchain_initialized = true;
            LOG_INFO("Blockchain initialized: contract=%s, rpc=%s", contract_addr, rpc_url);
        } else {
            LOG_WARN("Failed to initialize blockchain, continuing without blockchain support");
        }
    } else {
        LOG_WARN("Contract address not found, blockchain features disabled");
    }
    
    
    uint8_t sk[32] = {0};
    uint8_t pk[33] = {0};
    
    if (generate_keypair(sk, pk) != 0) {
        LOG_ERROR("Failed to generate keypair");
        return 1;
    }
    
    if (utee_host_init(sk, pk) != 0) {
        LOG_ERROR("Failed to initialize U-TEE");
        return 1;
    }
    
    LOG_INFO("U-TEE initialized successfully");
    
    
    uint16_t port = 8080; 
    if (utee_network_init(port) != 0) {
        LOG_ERROR("Failed to initialize network server");
        return 1;
    }
    
    
    if (utee_network_start() != 0) {
        LOG_ERROR("Failed to start network server");
        return 1;
    }
    
    LOG_INFO("U-TEE network server started on port %d", port);
    LOG_INFO("Press Ctrl+C to stop...");
    
    
    while (1) {
        sleep(1);
    }
    
    return 0;
}

