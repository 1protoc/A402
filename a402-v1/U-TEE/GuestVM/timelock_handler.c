

#include "utee_guest.h"
#include "../../Common/utils/timelock.h"
#include "../../Common/vault/vault.h"
#include "../../Common/utils/logger.h"
#include <string.h>

extern payment_channel_t channels[];
extern int channel_count;


int handle_timelock_timeout(void) {
    
    int unlocked_count = timelock_check_and_unlock_expired();
    
    if (unlocked_count == 0) {
        return 0;
    }
    
    LOG_INFO("Found %d expired timelock requests, unlocking assets...", unlocked_count);
    
    
    
    
    
    
    
    return unlocked_count;
}
