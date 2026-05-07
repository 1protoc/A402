#ifndef CHANNEL_H
#define CHANNEL_H

#include <stdint.h>
#include <stddef.h>

#define CHANNEL_ID_SIZE 32
#define ADDRESS_SIZE 20


typedef enum {
    CHANNEL_STATE_INIT,      
    CHANNEL_STATE_OPEN,      
    CHANNEL_STATE_LOCKED,    
    CHANNEL_STATE_CLOSING,   
    CHANNEL_STATE_CLOSED     
} channel_state_t;


typedef struct {
    uint8_t channel_id[CHANNEL_ID_SIZE];  
    uint8_t u_tee_address[ADDRESS_SIZE];  
    uint8_t m_tee_address[ADDRESS_SIZE]; 
    uint64_t total_amount;                
    uint64_t locked_amount;                
    uint64_t paid_amount;                  
    channel_state_t state;                 
    uint64_t nonce;                        
} payment_channel_t;



int create_channel(
    const uint8_t* u_tee_address,
    const uint8_t* m_tee_address,
    uint64_t amount,
    payment_channel_t* channel
);



int lock_assets(
    payment_channel_t* channel,
    uint64_t amount
);



int update_channel_payment(
    payment_channel_t* channel,
    uint64_t payment_amount
);



int close_channel(
    payment_channel_t* channel
);

#endif 

