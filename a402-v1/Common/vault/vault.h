#ifndef VAULT_H
#define VAULT_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define ADDRESS_SIZE 20
#define MAX_VAULTS 1000


typedef struct {
    uint8_t owner[ADDRESS_SIZE];  
    uint64_t free_balance;        
    uint64_t locked_balance;      
    uint64_t total_deposited;     
    int valid;
} vault_balance_t;


vault_balance_t* get_or_create_vault(const uint8_t* owner);


int vault_deposit(const uint8_t* owner, uint64_t amount);


int vault_bind_to_channel(const uint8_t* owner, uint64_t amount);


int vault_unbind_from_channel(const uint8_t* owner, uint64_t amount);


int vault_pay_from_locked(const uint8_t* owner, uint64_t amount);


int vault_get_balance(const uint8_t* owner, uint64_t* free, uint64_t* locked);


int vault_release(const uint8_t* owner, uint64_t amount);

#ifdef __cplusplus
}
#endif

#endif 
