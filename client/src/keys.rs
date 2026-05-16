//! Client (C) keypair material. C uses a regular Ethereum-style secp256k1
//! key: `sk_C` for ECDSA `σ_C` over `ascStateHash`, plus the 20-byte
//! keccak-derived address used as the on-chain `client` field in the
//! ASCManager channel struct.
//!
//! Unlike `enclave/` (Vault U) and `service_provider/` (Service Provider S),
//! Client keys are **not** assumed to be TEE-bound. The user holds them
//! locally, the same way an externally-owned account (EOA) would.

use a402_shared::evm_chain::Address;
use k256::ecdsa::SigningKey;
use sha3::{Digest, Keccak256};

use crate::error::ClientError;

#[derive(Debug)]
pub struct ClientKeys {
    pub ecdsa: SigningKey,
    pub address: Address,
}

impl ClientKeys {
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, ClientError> {
        let ecdsa = SigningKey::from_slice(bytes)
            .map_err(|e| ClientError::Key(format!("k256 SigningKey::from_slice: {e}")))?;
        let address = derive_address(&ecdsa);
        Ok(Self { ecdsa, address })
    }

    pub fn from_hex(hex_priv: &str) -> Result<Self, ClientError> {
        let stripped = hex_priv.strip_prefix("0x").unwrap_or(hex_priv);
        let raw = hex::decode(stripped).map_err(|e| ClientError::Hex(e.to_string()))?;
        if raw.len() != 32 {
            return Err(ClientError::Key(format!(
                "client secret must be 32 bytes, got {}",
                raw.len()
            )));
        }
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&raw);
        Self::from_bytes(&buf)
    }

    pub fn address_hex(&self) -> String {
        self.address.to_hex()
    }
}

fn derive_address(sk: &SigningKey) -> Address {
    let verifying = sk.verifying_key();
    let encoded = verifying.to_encoded_point(false);
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

    /// Anvil deterministic account #2 — the one the JS demo uses for the
    /// Client. Verifies our address derivation matches the well-known value.
    #[test]
    fn anvil_account_2_address() {
        let keys = ClientKeys::from_hex(
            "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a",
        )
        .expect("hex parse");
        assert_eq!(
            keys.address_hex().to_lowercase(),
            "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc".to_lowercase()
        );
    }

    #[test]
    fn rejects_short_secret() {
        let err = ClientKeys::from_hex("0x1234").unwrap_err();
        assert!(matches!(err, ClientError::Key(_)));
    }
}
