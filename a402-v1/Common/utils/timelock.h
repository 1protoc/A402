#ifndef TIMELOCK_H
#define TIMELOCK_H

#include <stdint.h>
#include <stddef.h>
#include <time.h>

#ifdef __cplusplus
extern "C" {
#endif


typedef struct {
    uint8_t cid[32];             
    uint8_t rid[32];             
    time_t lock_time;            
    uint64_t locked_amount;      
    int valid;
} timelock_request_t;

#define MAX_TIMELOCK_REQUESTS 1000
#define DEFAULT_TIMELOCK_SECONDS 300  


int timelock_record_request(
    const uint8_t* cid,
    const uint8_t* rid,
    uint64_t locked_amount,
    uint32_t timeout_seconds
);


int timelock_check_and_unlock_expired(void);


int timelock_is_expired(const uint8_t* cid, const uint8_t* rid);


int timelock_clear_request(const uint8_t* cid, const uint8_t* rid);

#ifdef __cplusplus
}
#endif

#endif 
