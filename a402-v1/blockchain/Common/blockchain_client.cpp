

#include "blockchain_client.h"
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <string>
#include <sstream>
#include <fstream>
#include <iostream>
#include <algorithm>
#include <sys/wait.h>


static char g_ethereum_rpc_url[256] = "http://127.0.0.1:8545";
static char g_ethereum_contract_address[64] = "";
static bool g_ethereum_initialized = false;

static char g_bitcoin_rpc_url[256] = "http://127.0.0.1:18332";
static bool g_bitcoin_initialized = false;


static int execute_command(const std::string& cmd, std::string& output) {
    FILE* pipe = popen(cmd.c_str(), "r");
    if (!pipe) {
        return -1;
    }
    
    char buffer[128];
    output.clear();
    while (fgets(buffer, sizeof(buffer), pipe) != nullptr) {
        output += buffer;
    }
    
    int status = pclose(pipe);
    return WEXITSTATUS(status);
}


static int extract_txid_from_json(const std::string& json, char* txid, size_t txid_len) {
    
    size_t pos = json.find("\"txid\"");
    if (pos == std::string::npos) {
        pos = json.find("\"hash\"");
    }
    if (pos == std::string::npos) {
        return -1;
    }
    
    pos = json.find("\"", pos + 1);
    if (pos == std::string::npos) {
        return -1;
    }
    
    size_t start = pos + 1;
    size_t end = json.find("\"", start);
    if (end == std::string::npos) {
        return -1;
    }
    
    std::string txid_str = json.substr(start, end - start);
    if (txid_str.length() >= txid_len) {
        return -1;
    }
    
    strncpy(txid, txid_str.c_str(), txid_len - 1);
    txid[txid_len - 1] = '\0';
    
    return 0;
}

int ethereum_init(const char* rpc_url, const char* contract_address, const char* private_key) {
    if (rpc_url) {
        strncpy(g_ethereum_rpc_url, rpc_url, sizeof(g_ethereum_rpc_url) - 1);
    }
    if (contract_address) {
        strncpy(g_ethereum_contract_address, contract_address, sizeof(g_ethereum_contract_address) - 1);
    }
    
    g_ethereum_initialized = true;
    return 0;
}

int ethereum_create_channel(
    const char* channel_id,
    const char* m_tee_address,
    uint64_t amount_wei,
    tx_result_t* result)
{
    if (!result || !channel_id || !m_tee_address) {
        return -1;
    }
    
    if (!g_ethereum_initialized || strlen(g_ethereum_contract_address) == 0) {
        strcpy(result->error, "Ethereum not initialized");
        result->success = 0;
        return -1;
    }
    
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../ethereum && ";
    cmd << "node -e \"";
    cmd << "const { ethers } = require('ethers');";
    cmd << "const fs = require('fs');";
    cmd << "const deployment = JSON.parse(fs.readFileSync('./deployment.json'));";
    cmd << "const provider = new ethers.JsonRpcProvider('" << g_ethereum_rpc_url << "');";
    cmd << "const wallet = provider.getSigner(0);";
    cmd << "const contract = new ethers.Contract(deployment.address, ";
    cmd << "[{\"inputs\":[{\"name\":\"channelId\",\"type\":\"bytes32\"},{\"name\":\"mTee\",\"type\":\"address\"}],";
    cmd << "\"name\":\"createChannel\",\"outputs\":[],\"stateMutability\":\"payable\",\"type\":\"function\"}], wallet);";
    cmd << "contract.createChannel('" << channel_id << "', '" << m_tee_address << "', {value: " << amount_wei << "})";
    cmd << ".then(tx => { console.log(JSON.stringify({txid: tx.hash})); tx.wait(); })";
    cmd << ".catch(err => { console.log(JSON.stringify({error: err.message})); });\"";
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && extract_txid_from_json(output, result->txid, sizeof(result->txid)) == 0) {
        result->success = 1;
        return 0;
    } else {
        result->success = 0;
        strncpy(result->error, output.c_str(), sizeof(result->error) - 1);
        return -1;
    }
}

int ethereum_send_raw_transaction(
    const char* to_address,
    const char* data,
    uint64_t value_wei,
    tx_result_t* result)
{
    if (!result || !to_address || !data) {
        return -1;
    }
    
    if (!g_ethereum_initialized) {
        strcpy(result->error, "Ethereum not initialized");
        result->success = 0;
        return -1;
    }
    
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../ethereum && ";
    cmd << "ETH_RPC_URL=" << g_ethereum_rpc_url << " ";
    cmd << "node scripts/blockchain_rpc.js sendRaw ";
    cmd << to_address << " " << data << " " << value_wei;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && extract_txid_from_json(output, result->txid, sizeof(result->txid)) == 0) {
        result->success = 1;
        return 0;
    } else {
        result->success = 0;
        strncpy(result->error, output.c_str(), sizeof(result->error) - 1);
        return -1;
    }
}

int ethereum_get_transaction_data(
    const char* tx_hash,
    uint8_t* data,
    size_t* data_len)
{
    if (!tx_hash || !data || !data_len) {
        return -1;
    }
    
    if (!g_ethereum_initialized) {
        return -1;
    }
    
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../ethereum && ";
    cmd << "ETH_RPC_URL=" << g_ethereum_rpc_url << " ";
    cmd << "node scripts/blockchain_rpc.js getTx " << tx_hash;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && output.length() > 2 && output.substr(0, 2) == "0x") {
        
        std::string hex = output.substr(2);
        
        hex.erase(std::remove(hex.begin(), hex.end(), '\n'), hex.end());
        hex.erase(std::remove(hex.begin(), hex.end(), '\r'), hex.end());
        
        size_t hex_len = hex.length();
        if (hex_len % 2 != 0) {
            return -1;
        }
        
        size_t data_size = hex_len / 2;
        if (data_size > *data_len) {
            return -1;
        }
        
        for (size_t i = 0; i < data_size; i++) {
            std::string byte_str = hex.substr(i * 2, 2);
            data[i] = (uint8_t)strtoul(byte_str.c_str(), nullptr, 16);
        }
        
        *data_len = data_size;
        return 0;
    }
    
    return -1;
}

int ethereum_deposit(const char* channel_id, uint64_t amount_wei, tx_result_t* result) {
    if (!result || !channel_id) {
        return -1;
    }
    
    if (!g_ethereum_initialized || strlen(g_ethereum_contract_address) == 0) {
        strcpy(result->error, "Ethereum not initialized");
        result->success = 0;
        return -1;
    }
    
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../ethereum && ";
    cmd << "ETH_RPC_URL=" << g_ethereum_rpc_url << " ";
    cmd << "node scripts/blockchain_rpc.js deposit ";
    cmd << channel_id << " " << amount_wei;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && extract_txid_from_json(output, result->txid, sizeof(result->txid)) == 0) {
        result->success = 1;
        return 0;
    } else {
        result->success = 0;
        strncpy(result->error, output.c_str(), sizeof(result->error) - 1);
        return -1;
    }
}

int ethereum_withdraw(const char* channel_id, uint64_t amount_wei, const char* to_address, tx_result_t* result) {
    if (!result || !channel_id || !to_address) {
        return -1;
    }
    
    if (!g_ethereum_initialized || strlen(g_ethereum_contract_address) == 0) {
        strcpy(result->error, "Ethereum not initialized");
        result->success = 0;
        return -1;
    }
    
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../ethereum && ";
    cmd << "ETH_RPC_URL=" << g_ethereum_rpc_url << " ";
    cmd << "node scripts/blockchain_rpc.js withdraw ";
    cmd << channel_id << " " << to_address << " " << amount_wei;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && extract_txid_from_json(output, result->txid, sizeof(result->txid)) == 0) {
        result->success = 1;
        return 0;
    } else {
        result->success = 0;
        strncpy(result->error, output.c_str(), sizeof(result->error) - 1);
        return -1;
    }
}

int ethereum_close_channel(const char* channel_id, tx_result_t* result) {
    if (!result || !channel_id) {
        return -1;
    }
    
    if (!g_ethereum_initialized || strlen(g_ethereum_contract_address) == 0) {
        strcpy(result->error, "Ethereum not initialized");
        result->success = 0;
        return -1;
    }
    
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../ethereum && ";
    cmd << "ETH_RPC_URL=" << g_ethereum_rpc_url << " ";
    cmd << "node scripts/blockchain_rpc.js closeChannel " << channel_id;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && extract_txid_from_json(output, result->txid, sizeof(result->txid)) == 0) {
        result->success = 1;
        return 0;
    } else {
        result->success = 0;
        strncpy(result->error, output.c_str(), sizeof(result->error) - 1);
        return -1;
    }
}

int ethereum_get_channel_info(const char* channel_id, channel_info_t* info) {
    return -1;
}

int bitcoin_init(const char* rpc_url) {
    if (rpc_url) {
        strncpy(g_bitcoin_rpc_url, rpc_url, sizeof(g_bitcoin_rpc_url) - 1);
    }
    g_bitcoin_initialized = true;
    return 0;
}

int bitcoin_create_channel(
    const char* channel_id,
    const char* u_tee_pubkey,
    const char* m_tee_pubkey,
    const char* user_c_pubkey,
    uint64_t amount_satoshis,
    uint32_t challenge_period,
    tx_result_t* result)
{
    if (!result || !channel_id || !u_tee_pubkey || !m_tee_pubkey || !user_c_pubkey) {
        return -1;
    }
    
    if (!g_bitcoin_initialized) {
        strcpy(result->error, "Bitcoin not initialized");
        result->success = 0;
        return -1;
    }
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../bitcoin && ";
    cmd << "BITCOIN_RPC_URL=" << g_bitcoin_rpc_url << " ";
    cmd << "python3 blockchain_rpc.py createChannel ";
    cmd << channel_id << " " << u_tee_pubkey << " " << m_tee_pubkey << " ";
    cmd << user_c_pubkey << " " << amount_satoshis << " " << challenge_period;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && extract_txid_from_json(output, result->txid, sizeof(result->txid)) == 0) {
        result->success = 1;
        return 0;
    } else {
        result->success = 0;
        strncpy(result->error, output.c_str(), sizeof(result->error) - 1);
        return -1;
    }
}

int bitcoin_deposit(const char* channel_id, uint64_t amount_satoshis, tx_result_t* result) {
    if (!result || !channel_id) {
        return -1;
    }
    
    if (!g_bitcoin_initialized) {
        strcpy(result->error, "Bitcoin not initialized");
        result->success = 0;
        return -1;
    }
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../bitcoin && ";
    cmd << "BITCOIN_RPC_URL=" << g_bitcoin_rpc_url << " ";
    cmd << "python3 blockchain_rpc.py deposit ";
    cmd << channel_id << " " << amount_satoshis;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && extract_txid_from_json(output, result->txid, sizeof(result->txid)) == 0) {
        result->success = 1;
        return 0;
    } else {
        result->success = 0;
        strncpy(result->error, output.c_str(), sizeof(result->error) - 1);
        return -1;
    }
}

int bitcoin_withdraw(
    const char* channel_id,
    uint64_t amount_satoshis,
    const char* to_address,
    tx_result_t* result)
{
    if (!result || !channel_id || !to_address) {
        return -1;
    }
    
    if (!g_bitcoin_initialized) {
        strcpy(result->error, "Bitcoin not initialized");
        result->success = 0;
        return -1;
    }
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../bitcoin && ";
    cmd << "BITCOIN_RPC_URL=" << g_bitcoin_rpc_url << " ";
    cmd << "python3 blockchain_rpc.py withdraw ";
    cmd << channel_id << " " << amount_satoshis << " " << to_address;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && extract_txid_from_json(output, result->txid, sizeof(result->txid)) == 0) {
        result->success = 1;
        return 0;
    } else {
        result->success = 0;
        strncpy(result->error, output.c_str(), sizeof(result->error) - 1);
        return -1;
    }
}

int bitcoin_close_channel(
    const char* channel_id,
    uint64_t user_c_amount,
    uint64_t m_tee_amount,
    const char* condition,
    tx_result_t* result)
{
    if (!result || !channel_id) {
        return -1;
    }
    
    if (!g_bitcoin_initialized) {
        strcpy(result->error, "Bitcoin not initialized");
        result->success = 0;
        return -1;
    }
    
    const char* close_condition = condition ? condition : "u_tee";
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../bitcoin && ";
    cmd << "BITCOIN_RPC_URL=" << g_bitcoin_rpc_url << " ";
    cmd << "python3 blockchain_rpc.py closeChannel ";
    cmd << channel_id << " " << user_c_amount << " " << m_tee_amount << " " << close_condition;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && extract_txid_from_json(output, result->txid, sizeof(result->txid)) == 0) {
        result->success = 1;
        return 0;
    } else {
        result->success = 0;
        strncpy(result->error, output.c_str(), sizeof(result->error) - 1);
        return -1;
    }
}

int bitcoin_send_raw_transaction(const char* hex_transaction, tx_result_t* result) {
    if (!result || !hex_transaction) {
        return -1;
    }
    
    if (!g_bitcoin_initialized) {
        strcpy(result->error, "Bitcoin not initialized");
        result->success = 0;
        return -1;
    }
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../bitcoin && ";
    cmd << "BITCOIN_RPC_URL=" << g_bitcoin_rpc_url << " ";
    cmd << "python3 blockchain_rpc.py sendRaw " << hex_transaction;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0 && extract_txid_from_json(output, result->txid, sizeof(result->txid)) == 0) {
        result->success = 1;
        return 0;
    } else {
        result->success = 0;
        strncpy(result->error, output.c_str(), sizeof(result->error) - 1);
        return -1;
    }
}

int bitcoin_get_transaction_data(
    const char* txid,
    uint8_t* data,
    size_t* data_len)
{
    if (!txid || !data || !data_len) {
        return -1;
    }
    
    if (!g_bitcoin_initialized) {
        return -1;
    }
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../bitcoin && ";
    cmd << "BITCOIN_RPC_URL=" << g_bitcoin_rpc_url << " ";
    cmd << "python3 blockchain_rpc.py getTx " << txid;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0) {
        size_t pos = output.find("\"hex\"");
        if (pos != std::string::npos) {
            pos = output.find("\"", pos + 1);
            if (pos != std::string::npos) {
                size_t start = pos + 1;
                size_t end = output.find("\"", start);
                if (end != std::string::npos) {
                    std::string hex = output.substr(start, end - start);
                    
                    hex.erase(std::remove(hex.begin(), hex.end(), '\n'), hex.end());
                    hex.erase(std::remove(hex.begin(), hex.end(), '\r'), hex.end());
                    
                    size_t hex_len = hex.length();
                    if (hex_len % 2 != 0) {
                        return -1;
                    }
                    
                    size_t data_size = hex_len / 2;
                    if (data_size > *data_len) {
                        return -1;
                    }
                    
                    for (size_t i = 0; i < data_size; i++) {
                        std::string byte_str = hex.substr(i * 2, 2);
                        data[i] = (uint8_t)strtoul(byte_str.c_str(), nullptr, 16);
                    }
                    
                    *data_len = data_size;
                    return 0;
                }
            }
        }
    }
    
    return -1;
}

int bitcoin_get_channel_info(const char* channel_id, channel_info_t* info) {
    if (!channel_id || !info) {
        return -1;
    }
    
    if (!g_bitcoin_initialized) {
        return -1;
    }
    
    std::ostringstream cmd;
    cmd << "cd " << __FILE__ << "/../../bitcoin && ";
    cmd << "BITCOIN_RPC_URL=" << g_bitcoin_rpc_url << " ";
    cmd << "python3 blockchain_rpc.py getChannelInfo " << channel_id;
    
    std::string output;
    int ret = execute_command(cmd.str(), output);
    
    if (ret == 0) {
        size_t pos = output.find("\"amount\"");
        if (pos != std::string::npos) {
            pos = output.find(":", pos);
            if (pos != std::string::npos) {
                size_t start = pos + 1;
                while (start < output.length() && (output[start] == ' ' || output[start] == '\t')) {
                    start++;
                }
                size_t end = start;
                while (end < output.length() && output[end] != ',' && output[end] != '}') {
                    end++;
                }
                std::string amount_str = output.substr(start, end - start);
                info->total_amount = strtoull(amount_str.c_str(), nullptr, 10);
                
                strncpy(info->channel_id, channel_id, sizeof(info->channel_id) - 1);
                info->is_open = 1;
                return 0;
            }
        }
    }
    
    return -1;
}
