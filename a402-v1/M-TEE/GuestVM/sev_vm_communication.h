#ifndef SEV_VM_COMMUNICATION_H
#define SEV_VM_COMMUNICATION_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif


typedef enum {
    SEV_COMM_CHANNEL_HOST_MEDIATED = 0,  
    SEV_COMM_CHANNEL_SHARED_MEMORY = 1,   
    SEV_COMM_CHANNEL_VIRTUAL_NET = 2      
} sev_comm_channel_type_t;


int sev_vm_comm_init(uint32_t vm_id, sev_comm_channel_type_t channel_type);


int sev_vm_comm_send(uint32_t dst_vm_id, const uint8_t* data, size_t len);


int sev_vm_comm_receive(uint8_t* buffer, size_t* buffer_len, uint32_t* src_vm_id);


typedef void (*sev_vm_message_callback_t)(uint32_t src_vm_id, 
                                           const uint8_t* data, 
                                           size_t len);
int sev_vm_comm_register_callback(sev_vm_message_callback_t callback);


void sev_vm_comm_close(void);

#ifdef __cplusplus
}
#endif

#endif 


