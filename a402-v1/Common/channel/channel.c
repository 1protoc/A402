#include "channel.h"
#include <string.h>
#include <stdlib.h>

int create_channel(
    const uint8_t* u_tee_address,
    const uint8_t* m_tee_address,
    uint64_t amount,
    payment_channel_t* channel)
{
    if (!u_tee_address || !m_tee_address || !channel) {
        return -1;
    }
    
    
    for (int i = 0; i < CHANNEL_ID_SIZE; i++) {
        channel->channel_id[i] = (uint8_t)(rand() % 256);
    }
    
    memcpy(channel->u_tee_address, u_tee_address, ADDRESS_SIZE);
    memcpy(channel->m_tee_address, m_tee_address, ADDRESS_SIZE);
    channel->total_amount = amount;
    channel->locked_amount = 0;
    channel->paid_amount = 0;
    channel->state = CHANNEL_STATE_OPEN;
    channel->nonce = 0;
    
    return 0;
}

int lock_assets(
    payment_channel_t* channel,
    uint64_t amount)
{
    if (!channel) {
        return -1;
    }
    
    if (channel->state != CHANNEL_STATE_OPEN && channel->state != CHANNEL_STATE_LOCKED) {
        return -1;
    }
    
    if (channel->total_amount - channel->locked_amount < amount) {
        return -1;
    }
    
    channel->locked_amount += amount;
    channel->state = CHANNEL_STATE_LOCKED;
    
    return 0;
}

int update_channel_payment(
    payment_channel_t* channel,
    uint64_t payment_amount)
{
    if (!channel) {
        return -1;
    }
    
    if (channel->locked_amount < payment_amount) {
        return -1;
    }
    
    channel->locked_amount -= payment_amount;
    channel->paid_amount += payment_amount;
    
    if (channel->locked_amount == 0) {
        channel->state = CHANNEL_STATE_OPEN;
    }
    
    return 0;
}

int close_channel(
    payment_channel_t* channel)
{
    if (!channel) {
        return -1;
    }
    
    channel->state = CHANNEL_STATE_CLOSED;
    
    return 0;
}

