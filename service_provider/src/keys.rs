//! Service Provider key material — `sk_S` (ECDSA) + Schnorr keypair derived
//! deterministically from the same secret. Per  this is
//! INDEPENDENT of the Vault's secret and never leaves the SP TEE.

use a402_shared::adaptor_sig_secp::{self, NormalizedKeypair};
use a402_shared::evm_chain::Address;
use k256::ecdsa::SigningKey;
use sha3::{Digest, Keccak256};

#[derive(Debug)]
pub struct SellerKeys {
    /// ECDSA secret for `σ_S` over `ascStateHash` and for signing outgoing
    /// EIP-1559 transactions (e.g. `forceClose`).
    pub ecdsa: SigningKey,
    /// Ethereum address derived from the ECDSA verifying key. Must equal the
    /// `provider` field of every channel the SP serves.
    pub address: Address,
    /// Normalised Schnorr keypair (`even-y`, `px < HALF_Q`) derived from the
    /// SAME secret via `keccak256("a402-sp-schnorr-v1" || sk)`. Holding both
    /// in the same SellerKeys is fine because we control both inside the same
    /// SP TEE; they're conceptually one identity ("S's signing material").
    pub schnorr: NormalizedKeypair,
}

impl SellerKeys {
    pub fn from_hex(hex_priv: &str) -> Result<Self, String> {
        let h = hex_priv.strip_prefix("0x").unwrap_or(hex_priv);
        let bytes = hex::decode(h).map_err(|e| format!("hex decode: {e}"))?;
        if bytes.len() != 32 {
            return Err(format!("expected 32 bytes, got {}", bytes.len()));
        }
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(secret: &[u8]) -> Result<Self, String> {
        let ecdsa =
            SigningKey::from_slice(secret).map_err(|e| format!("k256 SigningKey: {e}"))?;
        let address = derive_address(&ecdsa);

        // Derive a Schnorr keypair from the same secret. Using a tagged
        // keccak as the seed keeps the two key derivations cryptographically
        // independent (changing the tag won't accidentally collide with any
        // other key in the system).
        let mut hasher = Keccak256::new();
        hasher.update(b"a402-sp-schnorr-v1");
        hasher.update(secret);
        let seed = hasher.finalize();
        let schnorr = adaptor_sig_secp::derive_normalized_keypair(&seed)
            .map_err(|e| format!("schnorr derive: {e}"))?;

        Ok(Self {
            ecdsa,
            address,
            schnorr,
        })
    }

    pub fn address_hex(&self) -> String {
        self.address.to_hex()
    }

    pub fn schnorr_px_hex(&self) -> String {
        format!("0x{}", hex::encode(self.schnorr.px_bytes))
    }
}

/// `keccak256(uncompressed_pubkey_xy)[12..32]` — standard Ethereum address.
fn derive_address(signing_key: &SigningKey) -> Address {
    use k256::elliptic_curve::sec1::ToEncodedPoint;
    let pt = signing_key.verifying_key().to_encoded_point(false);
    let xy = &pt.as_bytes()[1..]; // strip the 0x04 SEC1 prefix
    let mut hasher = Keccak256::new();
    hasher.update(xy);
    let digest = hasher.finalize();
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&digest[12..32]);
    Address(addr)
}
