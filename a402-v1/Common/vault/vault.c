#include "vault.h"
#include <string.h>
#include <stdlib.h>

static vault_balance_t vaults[MAX_VAULTS];
static int vault_count = 0;

vault_balance_t* get_or_create_vault(const uint8_t* owner) {
    if (!owner) {
        return NULL;
    }
    
    
    for (int i = 0; i < vault_count; i++) {
        if (vaults[i].valid && memcmp(vaults[i].owner, owner, ADDRESS_SIZE) == 0) {
            return &vaults[i];
        }
    }
    
    
    if (vault_count >= MAX_VAULTS) {
        return NULL;
    }
    
    vault_balance_t* vault = &vaults[vault_count++];
    memcpy(vault->owner, owner, ADDRESS_SIZE);
    vault->free_balance = 0;
    vault->locked_balance = 0;
    vault->total_deposited = 0;
    vault->valid = 1;
    
    return vault;
}

int vault_deposit(const uint8_t* owner, uint64_t amount) {
    vault_balance_t* vault = get_or_create_vault(owner);
    if (!vault) {
        return -1;
    }
    
    vault->free_balance += amount;
    vault->total_deposited += amount;
    
    return 0;
}

int vault_bind_to_channel(const uint8_t* owner, uint64_t amount) {
    vault_balance_t* vault = get_or_create_vault(owner);
    if (!vault) {
        return -1;
    }
    
    if (vault->free_balance < amount) {
        return -1; 
    }
    
    vault->free_balance -= amount;
    vault->locked_balance += amount;
    
    return 0;
}

int vault_unbind_from_channel(const uint8_t* owner, uint64_t amount) {
    vault_balance_t* vault = get_or_create_vault(owner);
    if (!vault) {
        return -1;
    }
    
    if (vault->locked_balance < amount) {
        return -1;
    }
    
    vault->locked_balance -= amount;
    vault->free_balance += amount;
    
    return 0;
}

int vault_pay_from_locked(const uint8_t* owner, uint64_t amount) {
    vault_balance_t* vault = get_or_create_vault(owner);
    if (!vault) {
        return -1;
    }
    
    if (vault->locked_balance < amount) {
        return -1;
    }
    
    vault->locked_balance -= amount;
    
    
    return 0;
}

int vault_get_balance(const uint8_t* owner, uint64_t* free, uint64_t* locked) {
    vault_balance_t* vault = get_or_create_vault(owner);
    if (!vault || !free || !locked) {
        return -1;
    }
    
    *free = vault->free_balance;
    *locked = vault->locked_balance;
    
    return 0;
}

int vault_release(const uint8_t* owner, uint64_t amount) {
    vault_balance_t* vault = get_or_create_vault(owner);
    if (!vault) {
        return -1;
    }
    
    if (vault->free_balance < amount) {
        return -1;
    }
    
    vault->free_balance -= amount;
    
    return 0;
}
