//! Ethereum-prefixed ECDSA signing / recovery helpers, matching exactly
//! what `ASCManager._signedBy(...)` accepts on chain and what the SP's
//! `recover_eth_signed` expects in [service_provider/src/handlers.rs].
//!
//! Layout: `keccak256("\x19Ethereum Signed Message:\n32" || digest32)` →
//! prehash → secp256k1 ECDSA → 65-byte `r || s || v` with `v ∈ {27, 28}`.

use a402_shared::evm_chain::Address;
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use sha3::{Digest, Keccak256};

use crate::error::ClientError;

const ETH_PREFIX: &[u8] = b"\x19Ethereum Signed Message:\n32";

pub fn eth_signed_digest(digest32: &[u8; 32]) -> [u8; 32] {
    let mut prefixed = Vec::with_capacity(ETH_PREFIX.len() + 32);
    prefixed.extend_from_slice(ETH_PREFIX);
    prefixed.extend_from_slice(digest32);
    let mut hasher = Keccak256::new();
    hasher.update(&prefixed);
    let out = hasher.finalize();
    let mut buf = [0u8; 32];
    buf.copy_from_slice(out.as_slice());
    buf
}

pub fn sign_eth_signed(sk: &SigningKey, digest32: &[u8; 32]) -> [u8; 65] {
    let eth_digest = eth_signed_digest(digest32);
    let (sig, rid) = sk
        .sign_prehash_recoverable(&eth_digest)
        .expect("k256 sign_prehash_recoverable");
    let mut out = [0u8; 65];
    out[..32].copy_from_slice(&sig.r().to_bytes());
    out[32..64].copy_from_slice(&sig.s().to_bytes());
    out[64] = 27 + u8::from(rid);
    out
}

pub fn recover_eth_signed(digest32: &[u8; 32], sig65: &[u8]) -> Result<Address, ClientError> {
    if sig65.len() != 65 {
        return Err(ClientError::Signature(format!(
            "signature must be 65 bytes, got {}",
            sig65.len()
        )));
    }
    let mut rs = [0u8; 64];
    rs.copy_from_slice(&sig65[..64]);
    let v = sig65[64];
    let recovery = match v {
        27 | 0 => 0u8,
        28 | 1 => 1u8,
        _ => return Err(ClientError::Signature(format!("invalid recovery v={v}"))),
    };
    let sig = Signature::from_slice(&rs)
        .map_err(|e| ClientError::Signature(format!("Signature::from_slice: {e}")))?;
    let rid = RecoveryId::try_from(recovery)
        .map_err(|e| ClientError::Signature(format!("RecoveryId::try_from: {e}")))?;
    let eth_digest = eth_signed_digest(digest32);
    let recovered = VerifyingKey::recover_from_prehash(&eth_digest, &sig, rid)
        .map_err(|e| ClientError::Signature(format!("recover_from_prehash: {e}")))?;
    Ok(verifying_key_to_address(&recovered))
}

fn verifying_key_to_address(vk: &VerifyingKey) -> Address {
    let encoded = vk.to_encoded_point(false);
    let xy = &encoded.as_bytes()[1..];
    let mut hasher = Keccak256::new();
    hasher.update(xy);
    let digest = hasher.finalize();
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&digest[12..32]);
    Address(addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::ClientKeys;

    #[test]
    fn sign_recover_round_trip() {
        let keys = ClientKeys::from_hex(
            "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a",
        )
        .unwrap();
        let mut digest = [0u8; 32];
        for (i, b) in digest.iter_mut().enumerate() {
            *b = i as u8;
        }
        let sig = sign_eth_signed(&keys.ecdsa, &digest);
        let recovered = recover_eth_signed(&digest, &sig).unwrap();
        assert_eq!(recovered, keys.address);
    }
}
