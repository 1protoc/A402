#include "timelock.h"
#include <string.h>
#include <stdlib.h>
#include <time.h>

static timelock_request_t timelock_requests[MAX_TIMELOCK_REQUESTS];
static int timelock_count = 0;

int timelock_record_request(
    const uint8_t* cid,
    const uint8_t* rid,
    uint64_t locked_amount,
    uint32_t timeout_seconds)
{
    if (!cid || !rid) {
        return -1;
    }
    
    if (timelock_count >= MAX_TIMELOCK_REQUESTS) {
        return -1;
    }
    
    timelock_request_t* req = &timelock_requests[timelock_count++];
    memcpy(req->cid, cid, 32);
    memcpy(req->rid, rid, 32);
    req->lock_time = time(NULL);
    req->locked_amount = locked_amount;
    req->valid = 1;
    
    
    
    
    return 0;
}

int timelock_check_and_unlock_expired(void) {
    time_t now = time(NULL);
    int unlocked_count = 0;
    
    for (int i = 0; i < timelock_count; i++) {
        if (!timelock_requests[i].valid) {
            continue;
        }
        
        
        if (now - timelock_requests[i].lock_time > DEFAULT_TIMELOCK_SECONDS) {
            
            timelock_requests[i].valid = 0;
            unlocked_count++;
        }
    }
    
    return unlocked_count;
}

int timelock_is_expired(const uint8_t* cid, const uint8_t* rid) {
    if (!cid || !rid) {
        return -1;
    }
    
    time_t now = time(NULL);
    
    for (int i = 0; i < timelock_count; i++) {
        if (!timelock_requests[i].valid) {
            continue;
        }
        
        if (memcmp(timelock_requests[i].cid, cid, 32) == 0 &&
            memcmp(timelock_requests[i].rid, rid, 32) == 0) {
            
            if (now - timelock_requests[i].lock_time > DEFAULT_TIMELOCK_SECONDS) {
                return 1; 
            }
            return 0; 
        }
    }
    
    return -1; 
}

int timelock_clear_request(const uint8_t* cid, const uint8_t* rid) {
    if (!cid || !rid) {
        return -1;
    }
    
    for (int i = 0; i < timelock_count; i++) {
        if (timelock_requests[i].valid &&
            memcmp(timelock_requests[i].cid, cid, 32) == 0 &&
            memcmp(timelock_requests[i].rid, rid, 32) == 0) {
            timelock_requests[i].valid = 0;
            return 0;
        }
    }
    
    return -1;
}
