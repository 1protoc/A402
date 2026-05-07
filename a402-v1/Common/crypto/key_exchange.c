#include "key_exchange.h"
#include <openssl/ec.h>
#include <openssl/ecdh.h>
#include <openssl/evp.h>
#include <openssl/bn.h>
#include <openssl/obj_mac.h>
#include <string.h>
#include <stdlib.h>

int generate_keypair(uint8_t* privkey, uint8_t* pubkey) {
    if (!privkey || !pubkey) {
        return -1;
    }
    
    
    EC_KEY* ec_key = EC_KEY_new_by_curve_name(NID_secp256k1);
    if (!ec_key) {
        return -1;
    }
    
    
    if (EC_KEY_generate_key(ec_key) != 1) {
        EC_KEY_free(ec_key);
        return -1;
    }
    
    
    const BIGNUM* priv_bn = EC_KEY_get0_private_key(ec_key);
    if (!priv_bn || BN_bn2binpad(priv_bn, privkey, 32) != 32) {
        EC_KEY_free(ec_key);
        return -1;
    }
    
    
    const EC_GROUP* group = EC_KEY_get0_group(ec_key);
    const EC_POINT* pub_point = EC_KEY_get0_public_key(ec_key);
    size_t pub_len = EC_POINT_point2oct(group, pub_point, 
                                         POINT_CONVERSION_COMPRESSED,
                                         pubkey, 33, NULL);
    if (pub_len != 33) {
        EC_KEY_free(ec_key);
        return -1;
    }
    
    EC_KEY_free(ec_key);
    return 0;
}

int compute_shared_secret(
    const uint8_t* my_privkey,
    const uint8_t* peer_pubkey,
    uint8_t* shared_secret)
{
    if (!my_privkey || !peer_pubkey || !shared_secret) {
        return -1;
    }
    
    
    EC_KEY* my_key = EC_KEY_new_by_curve_name(NID_secp256k1);
    if (!my_key) {
        return -1;
    }
    
    
    BIGNUM* priv_bn = BN_bin2bn(my_privkey, 32, NULL);
    if (!priv_bn || EC_KEY_set_private_key(my_key, priv_bn) != 1) {
        BN_free(priv_bn);
        EC_KEY_free(my_key);
        return -1;
    }
    
    
    const EC_GROUP* group = EC_KEY_get0_group(my_key);
    EC_POINT* pub_point = EC_POINT_new(group);
    if (!pub_point) {
        BN_free(priv_bn);
        EC_KEY_free(my_key);
        return -1;
    }
    
    if (EC_POINT_mul(group, pub_point, priv_bn, NULL, NULL, NULL) != 1) {
        EC_POINT_free(pub_point);
        BN_free(priv_bn);
        EC_KEY_free(my_key);
        return -1;
    }
    
    EC_KEY_set_public_key(my_key, pub_point);
    
    
    EC_POINT* peer_point = EC_POINT_new(group);
    if (!peer_point) {
        EC_POINT_free(pub_point);
        BN_free(priv_bn);
        EC_KEY_free(my_key);
        return -1;
    }
    
    if (EC_POINT_oct2point(group, peer_point, peer_pubkey, 33, NULL) != 1) {
        EC_POINT_free(peer_point);
        EC_POINT_free(pub_point);
        BN_free(priv_bn);
        EC_KEY_free(my_key);
        return -1;
    }
    
    
    uint8_t shared_secret_buf[32];
    int field_size = EC_GROUP_get_degree(group);
    int secret_len = (field_size + 7) / 8;
    
    if (ECDH_compute_key(shared_secret_buf, secret_len, peer_point, my_key, NULL) != secret_len) {
        EC_POINT_free(peer_point);
        EC_POINT_free(pub_point);
        BN_free(priv_bn);
        EC_KEY_free(my_key);
        return -1;
    }
    
    
    memset(shared_secret, 0, 32);
    if (secret_len >= 32) {
        memcpy(shared_secret, shared_secret_buf, 32);
    } else {
        memcpy(shared_secret, shared_secret_buf, secret_len);
    }
    
    
    EC_POINT_free(peer_point);
    EC_POINT_free(pub_point);
    BN_free(priv_bn);
    EC_KEY_free(my_key);
    
    return 0;
}


