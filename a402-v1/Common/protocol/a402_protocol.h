#ifndef A402_PROTOCOL_H
#define A402_PROTOCOL_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif


#include "../mixer/receipt.h"




typedef struct {
    uint8_t pk_U[33];           
    uint8_t codehash_U[32];      
    uint8_t att_U[512];          
    size_t att_U_len;
} register_uvault_input_t;

typedef struct {
    uint8_t H_U[32];             
} register_uvault_output_t;

int register_uvault(const register_uvault_input_t* input, register_uvault_output_t* output);


typedef struct {
    uint8_t sid[32];             
    uint8_t pk_S[33];            
    uint8_t codehash_S[32];      
    uint8_t att_S[512];          
    size_t att_S_len;
} register_server_input_t;

typedef struct {
    uint8_t H_S[32];             
} register_server_output_t;

int register_server(const register_server_input_t* input, register_server_output_t* output);



typedef struct {
    uint8_t tx_lock[256];        
    size_t tx_lock_len;
    uint8_t U_address[20];        
    uint8_t S_address[20];        
    uint8_t mode;                
} create_asc_input_t;


typedef struct {
    uint8_t user_address[20];    
    uint64_t amount;             
    uint8_t S_address[20];        
} create_asc_privacy_input_t;

typedef struct {
    uint8_t cid[32];             
    uint8_t Gamma_cid[128];      
    size_t Gamma_cid_len;
    
    uint8_t accepted_interfaces[256];  
    size_t interfaces_len;
    uint64_t min_amount;         
    uint64_t max_amount;         
} create_asc_output_t;

int create_asc(const create_asc_input_t* input, create_asc_output_t* output);
int create_asc_privacy(const create_asc_privacy_input_t* input, create_asc_output_t* output);


typedef struct {
    uint8_t cid[32];             
    uint8_t req[1024];            
    size_t req_len;
    uint64_t a;                  
} send_request_input_t;

typedef struct {
    uint8_t rid[32];             
} send_request_output_t;

int send_request(const send_request_input_t* input, send_request_output_t* output);


typedef struct {
    uint8_t cid[32];             
    uint8_t rid[32];             
    uint8_t req[1024];            
    size_t req_len;
    uint64_t delta;              
} exec_request_input_t;

typedef struct {
    uint8_t T[33];               
    uint8_t hat_sigma_S[64];      
    uint8_t EncRes[4096];         
    size_t EncRes_len;
} exec_request_output_t;

int exec_request(const exec_request_input_t* input, exec_request_output_t* output);


typedef struct {
    uint8_t cid[32];             
    uint8_t rid[32];             
    uint8_t T[33];               
    uint8_t hat_sigma_S[64];      
    uint8_t EncRes[4096];         
    size_t EncRes_len;
} on_exec_reply_input_t;

typedef struct {
    uint8_t sigma_U[64];          
} on_exec_reply_output_t;

int on_exec_reply(const on_exec_reply_input_t* input, on_exec_reply_output_t* output);


typedef struct {
    uint8_t cid[32];             
    uint8_t rid[32];             
    uint8_t t[32];               
} reveal_secret_input_t;

int reveal_secret(const reveal_secret_input_t* input);


typedef struct {
    uint8_t cid[32];             
} close_asc_input_t;

typedef struct {
    uint8_t tx_unlock[256];      
    size_t tx_unlock_len;
} close_asc_output_t;

int close_asc(const close_asc_input_t* input, close_asc_output_t* output);


typedef struct {
    uint8_t vid[32];             
    uint8_t cid[32];             
    uint64_t delta;              
} bind_to_channel_input_t;

int bind_to_channel(const bind_to_channel_input_t* input);


typedef struct {
    uint8_t vid[32];             
    uint8_t cid[32];             
} unbind_from_channel_input_t;

int unbind_from_channel(const unbind_from_channel_input_t* input);


typedef struct {
    uint8_t cid[32];             
    uint64_t amount;             
} channel_deposit_input_t;

int channel_deposit(const channel_deposit_input_t* input);


typedef struct {
    uint8_t cid[32];             
    uint64_t amount;             
    uint8_t to_address[20];      
} channel_withdraw_input_t;

int channel_withdraw(const channel_withdraw_input_t* input);


typedef struct {
    uint8_t owner[20];           
} batch_settlement_input_t;

typedef struct {
    uint8_t tx_batch[512];        
    size_t tx_batch_len;
} batch_settlement_output_t;

int batch_settlement(const batch_settlement_input_t* input, batch_settlement_output_t* output);


typedef struct {
    uint8_t cid[32];             
    uint8_t rid[32];             
    uint64_t amount;             
} request_receipt_input_t;

typedef struct {
    receipt_t receipt;           
} request_receipt_output_t;

int request_receipt(const request_receipt_input_t* input, request_receipt_output_t* output);


typedef struct {
    uint8_t s_address[20];       
    uint8_t cid_list[32][100];   
    int cid_count;                
} s_batch_settlement_input_t;

typedef struct {
    uint8_t tx_batch[512];       
    size_t tx_batch_len;
} s_batch_settlement_output_t;

int s_batch_settlement(const s_batch_settlement_input_t* input, s_batch_settlement_output_t* output);

#ifdef __cplusplus
}
#endif

#endif 
