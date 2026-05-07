#include "platform_sev.h"
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <openssl/sha.h>
#include <openssl/ec.h>
#include <openssl/ecdsa.h>
#include <openssl/evp.h>
#include <openssl/bn.h>
#include <openssl/obj_mac.h>



int platform_sev_init(void) {
    
    printf("[Platform SEV] Initialized\n");
    return 0;
}

void platform_sha256(const uint8_t* data, size_t len, uint8_t* hash) {
    SHA256_CTX ctx;
    SHA256_Init(&ctx);
    SHA256_Update(&ctx, data, len);
    SHA256_Final(hash, &ctx);
}

void* platform_malloc(size_t size) {
    return malloc(size);
}

void platform_free(void* ptr) {
    free(ptr);
}

int platform_get_random(uint8_t* buffer, size_t len) {
    
    FILE* urandom = fopen("/dev/urandom", "r");
    if (!urandom) {
        return -1;
    }
    size_t read = fread(buffer, 1, len, urandom);
    fclose(urandom);
    return (read == len) ? 0 : -1;
}

int platform_sign_message(const uint8_t* message, size_t message_len,
                          const uint8_t* privkey, uint8_t* signature) {
    
    
    if (!message || !privkey || !signature) {
        return -1;
    }
    
    EVP_MD_CTX* md_ctx = EVP_MD_CTX_new();
    if (!md_ctx) {
        return -1;
    }
    
    BIGNUM* bn = BN_bin2bn(privkey, 32, NULL);
    if (!bn) {
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    EC_KEY* ec_key = EC_KEY_new_by_curve_name(NID_secp256k1);
    if (!ec_key) {
        BN_free(bn);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    if (EC_KEY_set_private_key(ec_key, bn) != 1) {
        EC_KEY_free(ec_key);
        BN_free(bn);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    const EC_GROUP* group = EC_KEY_get0_group(ec_key);
    EC_POINT* pub_key = EC_POINT_new(group);
    if (!pub_key) {
        EC_KEY_free(ec_key);
        BN_free(bn);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    if (EC_POINT_mul(group, pub_key, bn, NULL, NULL, NULL) != 1) {
        EC_POINT_free(pub_key);
        EC_KEY_free(ec_key);
        BN_free(bn);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    EC_KEY_set_public_key(ec_key, pub_key);
    
    EVP_PKEY* pkey = EVP_PKEY_new();
    if (!pkey || EVP_PKEY_set1_EC_KEY(pkey, ec_key) != 1) {
        EC_POINT_free(pub_key);
        EC_KEY_free(ec_key);
        BN_free(bn);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    if (EVP_DigestSignInit(md_ctx, NULL, EVP_sha256(), NULL, pkey) != 1 ||
        EVP_DigestSignUpdate(md_ctx, message, message_len) != 1) {
        EVP_PKEY_free(pkey);
        EC_POINT_free(pub_key);
        EC_KEY_free(ec_key);
        BN_free(bn);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    size_t sig_len = 64;
    if (EVP_DigestSignFinal(md_ctx, signature, &sig_len) != 1 || sig_len != 64) {
        EVP_PKEY_free(pkey);
        EC_POINT_free(pub_key);
        EC_KEY_free(ec_key);
        BN_free(bn);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    EVP_PKEY_free(pkey);
    EC_POINT_free(pub_key);
    EC_KEY_free(ec_key);
    BN_free(bn);
    EVP_MD_CTX_free(md_ctx);
    
    return 0;
}

int platform_verify_signature(const uint8_t* message, size_t message_len,
                              const uint8_t* pubkey, const uint8_t* signature) {
    
    if (!message || !pubkey || !signature) {
        return -1;
    }
    
    EVP_MD_CTX* md_ctx = EVP_MD_CTX_new();
    if (!md_ctx) {
        return -1;
    }
    
    EC_KEY* ec_key = EC_KEY_new_by_curve_name(NID_secp256k1);
    if (!ec_key) {
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    const EC_GROUP* group = EC_KEY_get0_group(ec_key);
    EC_POINT* pub_point = EC_POINT_new(group);
    if (!pub_point) {
        EC_KEY_free(ec_key);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    if (EC_POINT_oct2point(group, pub_point, pubkey, 33, NULL) != 1) {
        EC_POINT_free(pub_point);
        EC_KEY_free(ec_key);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    EC_KEY_set_public_key(ec_key, pub_point);
    
    EVP_PKEY* pkey = EVP_PKEY_new();
    if (!pkey || EVP_PKEY_set1_EC_KEY(pkey, ec_key) != 1) {
        EC_POINT_free(pub_point);
        EC_KEY_free(ec_key);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    if (EVP_DigestVerifyInit(md_ctx, NULL, EVP_sha256(), NULL, pkey) != 1 ||
        EVP_DigestVerifyUpdate(md_ctx, message, message_len) != 1) {
        EVP_PKEY_free(pkey);
        EC_POINT_free(pub_point);
        EC_KEY_free(ec_key);
        EVP_MD_CTX_free(md_ctx);
        return -1;
    }
    
    int ret = EVP_DigestVerifyFinal(md_ctx, signature, 64);
    
    EVP_PKEY_free(pkey);
    EC_POINT_free(pub_point);
    EC_KEY_free(ec_key);
    EVP_MD_CTX_free(md_ctx);
    
    return (ret == 1) ? 0 : -1;
}

int platform_verify_page(void* page_addr) {
    return 0;
}

int platform_get_certificate(uint8_t* cert, size_t* cert_len) {
    return 0;
}

