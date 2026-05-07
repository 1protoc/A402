#ifndef HOST_VM_MEDIATOR_H
#define HOST_VM_MEDIATOR_H

#include <stdint.h>
#include <stddef.h>




typedef struct {
    uint32_t vm_id;
    void* vm_handle;        
    uint8_t vm_cert[2048];  
    bool is_active;
} guest_vm_info_t;


int host_vm_mediator_init(void);


int host_vm_mediator_register_vm(uint32_t vm_id, void* vm_handle);


int host_vm_mediator_forward_message(uint32_t src_vm_id,
                                     uint32_t dst_vm_id,
                                     const uint8_t* data,
                                     size_t len);


int host_vm_mediator_broadcast(uint32_t src_vm_id,
                               const uint8_t* data,
                               size_t len);


int host_vm_mediator_receive_from_vm(uint32_t vm_id,
                                      uint8_t* buffer,
                                      size_t* buffer_len);


void host_vm_mediator_close(void);

#endif 


