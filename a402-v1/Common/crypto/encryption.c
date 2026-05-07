#include "encryption.h"
#include <string.h>
#include <stdlib.h>
#include <openssl/evp.h>
#include <openssl/aes.h>
#include <openssl/rand.h>




int encrypt_data(
    const uint8_t* plaintext,
    size_t plaintext_len,
    const uint8_t* key,
    const uint8_t* iv,
    uint8_t* ciphertext,
    size_t ciphertext_max_len,
    uint8_t* tag)
{
    if (!plaintext || !key || !iv || !ciphertext || !tag) {
        return -1;
    }
    
    if (plaintext_len + 16 > ciphertext_max_len) {
        return -1;
    }
    
    
    EVP_CIPHER_CTX* ctx = EVP_CIPHER_CTX_new();
    if (!ctx) {
        return -1;
    }
    
    
    if (EVP_EncryptInit_ex(ctx, EVP_aes_256_gcm(), NULL, NULL, NULL) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        return -1;
    }
    
    
    if (EVP_EncryptInit_ex(ctx, NULL, NULL, key, iv) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        return -1;
    }
    
    
    int len;
    int ciphertext_len = 0;
    if (EVP_EncryptUpdate(ctx, ciphertext, &len, plaintext, (int)plaintext_len) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        return -1;
    }
    ciphertext_len = len;
    
    
    if (EVP_EncryptFinal_ex(ctx, ciphertext + len, &len) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        return -1;
    }
    ciphertext_len += len;
    
    
    if (EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_GET_TAG, 16, tag) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        return -1;
    }
    
    EVP_CIPHER_CTX_free(ctx);
    return ciphertext_len;
}

int decrypt_data(
    const uint8_t* ciphertext,
    size_t ciphertext_len,
    const uint8_t* key,
    const uint8_t* iv,
    const uint8_t* tag,
    uint8_t* plaintext,
    size_t plaintext_max_len)
{
    if (!ciphertext || !key || !iv || !tag || !plaintext) {
        return -1;
    }
    
    if (ciphertext_len > plaintext_max_len) {
        return -1;
    }
    
    
    EVP_CIPHER_CTX* ctx = EVP_CIPHER_CTX_new();
    if (!ctx) {
        return -1;
    }
    
    
    if (EVP_DecryptInit_ex(ctx, EVP_aes_256_gcm(), NULL, NULL, NULL) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        return -1;
    }
    
    
    if (EVP_DecryptInit_ex(ctx, NULL, NULL, key, iv) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        return -1;
    }
    
    
    int len;
    int plaintext_len = 0;
    if (EVP_DecryptUpdate(ctx, plaintext, &len, ciphertext, (int)ciphertext_len) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        return -1;
    }
    plaintext_len = len;
    
    
    if (EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_TAG, 16, (void*)tag) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        return -1;
    }
    
    
    if (EVP_DecryptFinal_ex(ctx, plaintext + len, &len) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        return -1; 
    }
    plaintext_len += len;
    
    EVP_CIPHER_CTX_free(ctx);
    return plaintext_len;
}

int encrypt_with_shared_key(
    const uint8_t* plaintext,
    size_t plaintext_len,
    const uint8_t* shared_key_sk_m,
    uint8_t* ciphertext,
    size_t ciphertext_max_len,
    uint8_t* tag)
{
    
    uint8_t iv[AES_IV_SIZE];
    if (RAND_bytes(iv, AES_IV_SIZE) != 1) {
        return -1;
    }
    
    
    
    return encrypt_data(plaintext, plaintext_len, shared_key_sk_m, iv, ciphertext, ciphertext_max_len, tag);
}

int decrypt_with_shared_key(
    const uint8_t* ciphertext,
    size_t ciphertext_len,
    const uint8_t* shared_key_sk_m,
    const uint8_t* tag,
    uint8_t* plaintext,
    size_t plaintext_max_len)
{
    
    uint8_t iv[AES_IV_SIZE] = {0};
    
    return decrypt_data(ciphertext, ciphertext_len, shared_key_sk_m, iv, tag, plaintext, plaintext_max_len);
}

