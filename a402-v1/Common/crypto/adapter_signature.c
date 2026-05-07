#include "adapter_signature.h"
#include <string.h>
#include <stdlib.h>
#include <openssl/ec.h>
#include <openssl/ecdsa.h>
#include <openssl/bn.h>
#include <openssl/obj_mac.h>
#include <openssl/evp.h>
#include <openssl/sha.h>




int generate_adapter_signature(
    const uint8_t* privkey,
    const uint8_t* message,
    size_t message_len,
    adapter_signature_ctx_t* ctx)
{
    if (!privkey || !message || !ctx) {
        return -1;
    }

    
    
    FILE* urandom = fopen("/dev/urandom", "r");
    if (!urandom) {
        return -1;
    }
    if (fread(ctx->secret_t, 1, 32, urandom) != 32) {
        fclose(urandom);
        return -1;
    }
    fclose(urandom);
    
    
    EC_KEY* ec_key = EC_KEY_new_by_curve_name(NID_secp256k1);
    if (!ec_key) {
        return -1;
    }
    
    BIGNUM* t_bn = BN_bin2bn(ctx->secret_t, 32, NULL);
    if (!t_bn) {
        EC_KEY_free(ec_key);
        return -1;
    }
    
    const EC_GROUP* group = EC_KEY_get0_group(ec_key);
    EC_POINT* T_point = EC_POINT_new(group);
    if (!T_point) {
        BN_free(t_bn);
        EC_KEY_free(ec_key);
        return -1;
    }
    
    
    if (EC_POINT_mul(group, T_point, t_bn, NULL, NULL, NULL) != 1) {
        EC_POINT_free(T_point);
        BN_free(t_bn);
        EC_KEY_free(ec_key);
        return -1;
    }
    
    
    size_t T_len = EC_POINT_point2oct(group, T_point, 
                                       POINT_CONVERSION_COMPRESSED,
                                       ctx->adapter_point_T, 33, NULL);
    if (T_len != 33) {
        EC_POINT_free(T_point);
        BN_free(t_bn);
        EC_KEY_free(ec_key);
        return -1;
    }
    
    
    uint8_t extended_message[1024 + 33];
    if (message_len + 33 > sizeof(extended_message)) {
        EC_POINT_free(T_point);
        BN_free(t_bn);
        EC_KEY_free(ec_key);
        return -1;
    }
    
    memcpy(extended_message, message, message_len);
    memcpy(extended_message + message_len, ctx->adapter_point_T, 33);
    
    
    BIGNUM* priv_bn = BN_bin2bn(privkey, 32, NULL);
    if (!priv_bn || EC_KEY_set_private_key(ec_key, priv_bn) != 1) {
        EC_POINT_free(T_point);
        BN_free(t_bn);
        BN_free(priv_bn);
        EC_KEY_free(ec_key);
        return -1;
    }
    
    
    EC_POINT* pub_point = EC_POINT_new(group);
    if (!pub_point || EC_POINT_mul(group, pub_point, priv_bn, NULL, NULL, NULL) != 1) {
        EC_POINT_free(T_point);
        BN_free(t_bn);
        BN_free(priv_bn);
        EC_KEY_free(ec_key);
        return -1;
    }
    EC_KEY_set_public_key(ec_key, pub_point);
    
    
    ECDSA_SIG* sig = ECDSA_do_sign(extended_message, message_len + 33, ec_key);
    if (!sig) {
        EC_POINT_free(pub_point);
        EC_POINT_free(T_point);
        BN_free(t_bn);
        BN_free(priv_bn);
        EC_KEY_free(ec_key);
        return -1;
    }
    
    
    const BIGNUM* r = ECDSA_SIG_get0_r(sig);
    const BIGNUM* s = ECDSA_SIG_get0_s(sig);
    
    
    BN_bn2binpad(r, ctx->signature, 32);
    BN_bn2binpad(s, ctx->signature + 32, 32);
    
    
    ECDSA_SIG_free(sig);
    EC_POINT_free(pub_point);
    EC_POINT_free(T_point);
    BN_free(t_bn);
    BN_free(priv_bn);
    EC_KEY_free(ec_key);
    
    return 0;
}

int verify_adapter_signature(
    const uint8_t* pubkey,
    const uint8_t* message,
    size_t message_len,
    const uint8_t* signature,
    const uint8_t* adapter_point_T)
{
    if (!pubkey || !message || !signature || !adapter_point_T) {
        return -1;
    }

    
    
    return 0;
}

int extract_secret_from_tx(
    const uint8_t* tx_data,
    size_t tx_len,
    const uint8_t* adapter_point_T,
    uint8_t* secret_t)
{
    if (!tx_data || !adapter_point_T || !secret_t) {
        return -1;
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    if (tx_len >= 32) {
        
        memcpy(secret_t, tx_data + tx_len - 32, 32);
        return 0;
    }
    
    
    return -1;
}

