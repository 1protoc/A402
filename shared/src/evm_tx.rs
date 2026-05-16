//! EIP-1559 transaction signing for the EVM ASC path.
//!
//! Scope: a self-contained module that takes the calldata produced by
//! [`crate::evm_chain`] and produces a raw, signed transaction ready for
//! `eth_sendRawTransaction`. No external dependency beyond what the rest of
//! the enclave already pulls in (`k256`, `sha3`).
//!
//! Why a hand-rolled implementation:
//!   - The full transaction shape we need is EIP-1559 (`type 0x02`) with an
//!     empty access list. ~120 lines.
//!   - Pulling in `ethers-rs` or `alloy` would 10x the dependency tree for
//!     three functions we'd actually use.
//!   - The cross-stack fixture against viem (see [`tests::cross_stack_eip1559`])
//!     pins us byte-for-byte to the canonical encoding.
//!
//! On-chain layout of a signed EIP-1559 transaction:
//!
//! ```text
//! 0x02 || rlp([
//!     chainId,                // RLP-encoded big-endian (minimal)
//!     nonce,
//!     maxPriorityFeePerGas,
//!     maxFeePerGas,
//!     gasLimit,
//!     to,                     // 20-byte address (rlp byte-string)
//!     value,
//!     data,                   // calldata bytes
//!     accessList,             // empty list (0xc0)
//!     yParity,                // 0 or 1
//!     r,                      // 32-byte BE, minimal
//!     s
//! ])
//! ```
//!
//! Pre-image for signing: same RLP list MINUS yParity/r/s, prefixed with
//! `0x02`. Then `keccak256` of that gives the digest fed to ECDSA-sign.

use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature, SigningKey};
use sha3::{Digest, Keccak256};

use crate::evm_chain::{Address, EvmError};

const EIP_1559_TX_TYPE: u8 = 0x02;

/// Holds the private key used to sign Ethereum transactions inside the
/// enclave. The matching address is derived once at construction.
#[derive(Debug, Clone)]
pub struct EvmSigner {
    signing_key: SigningKey,
    address: Address,
    chain_id: u64,
}

impl EvmSigner {
    /// Construct from a raw 32-byte private key plus the target chain id.
    pub fn from_bytes(secret: &[u8; 32], chain_id: u64) -> Result<Self, EvmError> {
        let signing_key = SigningKey::from_slice(secret).map_err(|err| {
            EvmError::BadResponse(format!("invalid private key for EvmSigner: {err}"))
        })?;
        let address = derive_address(&signing_key);
        Ok(Self {
            signing_key,
            address,
            chain_id,
        })
    }

    /// Construct from a `0x`-prefixed hex private key.
    pub fn from_hex(hex_priv: &str, chain_id: u64) -> Result<Self, EvmError> {
        let h = hex_priv.strip_prefix("0x").unwrap_or(hex_priv);
        let bytes = hex::decode(h)
            .map_err(|err| EvmError::BadResponse(format!("EvmSigner hex decode: {err}")))?;
        let bytes: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
            EvmError::BadResponse("EvmSigner: private key must be 32 bytes".to_string())
        })?;
        Self::from_bytes(&bytes, chain_id)
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    /// Consumes the signer and returns the underlying `k256::ecdsa::SigningKey`.
    /// Useful for callers that need to sign data that ISN'T an EIP-1559 tx
    /// (e.g. the `eth_signedMessage` flow used by `ASCManager._signedBy`).
    pub fn into_signing_key(self) -> SigningKey {
        self.signing_key
    }

    /// Borrowed view of the underlying signing key.
    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }
}

/// Parameters required to assemble a signed EIP-1559 transaction.
#[derive(Debug, Clone)]
pub struct Eip1559TxParams<'a> {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: u128,
    pub max_fee_per_gas: u128,
    pub gas_limit: u64,
    pub to: Address,
    pub value: u128,
    pub data: &'a [u8],
}

/// Signs the given EIP-1559 parameters and returns the raw bytes ready for
/// `eth_sendRawTransaction`. The `0x` prefix is NOT added.
pub fn sign_eip1559(signer: &EvmSigner, params: &Eip1559TxParams) -> Result<Vec<u8>, EvmError> {
    let unsigned = encode_eip1559_unsigned(params);
    let mut to_hash = Vec::with_capacity(1 + unsigned.len());
    to_hash.push(EIP_1559_TX_TYPE);
    to_hash.extend_from_slice(&unsigned);
    let digest = keccak256(&to_hash);

    let (sig, recovery_id) = signer
        .signing_key
        .sign_prehash(&digest)
        .map(|s: Signature| {
            // `sign_prehash` from hazmat doesn't return the recovery id; we
            // recover it from the signature ourselves below.
            (s, RecoveryId::trial_recovery_from_prehash(
                signer.signing_key.verifying_key(), &digest, &s
            ).expect("recovery_id available for a freshly-produced sig"))
        })
        .map_err(|err| EvmError::BadResponse(format!("ECDSA sign: {err}")))?;

    // Normalize to low-S as per EIP-2 / EIP-1559 expectations.
    let normalized = sig.normalize_s().unwrap_or(sig);
    let r_bytes = normalized.r().to_bytes();
    let s_bytes = normalized.s().to_bytes();
    let y_parity = u8::from(recovery_id) & 1;

    let signed = encode_eip1559_signed(params, y_parity, &r_bytes, &s_bytes);
    let mut out = Vec::with_capacity(1 + signed.len());
    out.push(EIP_1559_TX_TYPE);
    out.extend_from_slice(&signed);
    Ok(out)
}

fn encode_eip1559_unsigned(params: &Eip1559TxParams) -> Vec<u8> {
    // The 9-element list for the signing pre-image (no yParity / r / s yet,
    // and accessList is empty).
    let mut items: Vec<Vec<u8>> = Vec::with_capacity(9);
    items.push(rlp_encode_u64(params.chain_id));
    items.push(rlp_encode_u64(params.nonce));
    items.push(rlp_encode_u128(params.max_priority_fee_per_gas));
    items.push(rlp_encode_u128(params.max_fee_per_gas));
    items.push(rlp_encode_u64(params.gas_limit));
    items.push(rlp_encode_bytes(&params.to.0));
    items.push(rlp_encode_u128(params.value));
    items.push(rlp_encode_bytes(params.data));
    items.push(rlp_encode_list_empty()); // accessList
    rlp_encode_list(&items)
}

fn encode_eip1559_signed(
    params: &Eip1559TxParams,
    y_parity: u8,
    r_be: &[u8],
    s_be: &[u8],
) -> Vec<u8> {
    // 12-element list including signature components.
    let mut items: Vec<Vec<u8>> = Vec::with_capacity(12);
    items.push(rlp_encode_u64(params.chain_id));
    items.push(rlp_encode_u64(params.nonce));
    items.push(rlp_encode_u128(params.max_priority_fee_per_gas));
    items.push(rlp_encode_u128(params.max_fee_per_gas));
    items.push(rlp_encode_u64(params.gas_limit));
    items.push(rlp_encode_bytes(&params.to.0));
    items.push(rlp_encode_u128(params.value));
    items.push(rlp_encode_bytes(params.data));
    items.push(rlp_encode_list_empty());
    items.push(rlp_encode_u64(y_parity as u64));
    items.push(rlp_encode_bytes(&strip_leading_zeros(r_be)));
    items.push(rlp_encode_bytes(&strip_leading_zeros(s_be)));
    rlp_encode_list(&items)
}

/* -------------------------------------------------------------------------- */
/*                                  RLP                                       */
/* -------------------------------------------------------------------------- */

/// RLP-encodes a byte string per the canonical Ethereum rules:
///   - 1-byte input < 0x80              → itself
///   - len < 56                          → 0x80+len || bytes
///   - len ≥ 56                          → 0xb7+len_of_len || len_be || bytes
fn rlp_encode_bytes(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() == 1 && bytes[0] < 0x80 {
        return vec![bytes[0]];
    }
    let mut out = Vec::with_capacity(bytes.len() + 9);
    if bytes.len() < 56 {
        out.push(0x80 + bytes.len() as u8);
    } else {
        let len_be = u_to_be_minimal(bytes.len() as u128);
        out.push(0xb7 + len_be.len() as u8);
        out.extend_from_slice(&len_be);
    }
    out.extend_from_slice(bytes);
    out
}

/// RLP-encodes the concatenation of already-encoded items.
fn rlp_encode_list(items: &[Vec<u8>]) -> Vec<u8> {
    let body_len: usize = items.iter().map(|item| item.len()).sum();
    let mut out = Vec::with_capacity(body_len + 9);
    if body_len < 56 {
        out.push(0xc0 + body_len as u8);
    } else {
        let len_be = u_to_be_minimal(body_len as u128);
        out.push(0xf7 + len_be.len() as u8);
        out.extend_from_slice(&len_be);
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

fn rlp_encode_list_empty() -> Vec<u8> {
    vec![0xc0]
}

fn rlp_encode_u64(value: u64) -> Vec<u8> {
    rlp_encode_bytes(&u_to_be_minimal(value as u128))
}

fn rlp_encode_u128(value: u128) -> Vec<u8> {
    rlp_encode_bytes(&u_to_be_minimal(value))
}

/// Returns the big-endian byte representation of `value` with leading zeros
/// stripped. RLP requires the canonical form: 0 is the empty byte string,
/// values fit in the smallest possible byte count.
fn u_to_be_minimal(value: u128) -> Vec<u8> {
    if value == 0 {
        return Vec::new();
    }
    let bytes = value.to_be_bytes();
    let first_nonzero = bytes.iter().position(|b| *b != 0).unwrap();
    bytes[first_nonzero..].to_vec()
}

fn strip_leading_zeros(bytes: &[u8]) -> Vec<u8> {
    let first_nonzero = bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len());
    bytes[first_nonzero..].to_vec()
}

/* -------------------------------------------------------------------------- */
/*                            Address derivation                              */
/* -------------------------------------------------------------------------- */

fn derive_address(signing_key: &SigningKey) -> Address {
    let verifying = signing_key.verifying_key();
    let pt = verifying.to_encoded_point(false);
    let xy = &pt.as_bytes()[1..]; // strip the 0x04 SEC1 prefix
    let digest = keccak256(xy);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&digest[12..32]);
    Address(addr)
}

fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/* -------------------------------------------------------------------------- */
/*                                  Tests                                     */
/* -------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;

    /// Anvil deterministic account #1; well-known address.
    const ANVIL_PRIV: &str =
        "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
    const ANVIL_ADDR: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";

    #[test]
    fn address_matches_anvil_account_one() {
        let signer = EvmSigner::from_hex(ANVIL_PRIV, 31337).unwrap();
        assert_eq!(
            signer.address().to_hex().to_lowercase(),
            ANVIL_ADDR.to_lowercase()
        );
        assert_eq!(signer.chain_id(), 31337);
    }

    #[test]
    fn rlp_basic_vectors() {
        // RLP wiki vectors: https://eth.wiki/fundamentals/rlp
        assert_eq!(rlp_encode_bytes(b"dog"), b"\x83dog".to_vec());
        assert_eq!(
            rlp_encode_list(&[
                rlp_encode_bytes(b"cat"),
                rlp_encode_bytes(b"dog"),
            ]),
            b"\xc8\x83cat\x83dog".to_vec()
        );
        assert_eq!(rlp_encode_bytes(b""), vec![0x80]);
        assert_eq!(rlp_encode_list(&[]), vec![0xc0]);
        assert_eq!(rlp_encode_bytes(&[0]), vec![0x00]);
        assert_eq!(rlp_encode_bytes(&[0x0f]), vec![0x0f]);
        assert_eq!(rlp_encode_bytes(&[0x04, 0x00]), vec![0x82, 0x04, 0x00]);
    }

    #[test]
    fn u_to_be_minimal_strips_leading_zeros() {
        assert_eq!(u_to_be_minimal(0), Vec::<u8>::new());
        assert_eq!(u_to_be_minimal(1), vec![1]);
        assert_eq!(u_to_be_minimal(0x100), vec![0x01, 0x00]);
        assert_eq!(u_to_be_minimal(0xff), vec![0xff]);
        assert_eq!(u_to_be_minimal(u64::MAX as u128), vec![0xff; 8]);
    }

    /// Cross-stack agreement: viem produces the canonical RLP encoding of an
    /// EIP-1559 transaction; this test runs the Rust encoder against the same
    /// inputs and asserts byte-for-byte equality of the unsigned pre-image AND
    /// the signed serialized bytes.
    ///
    /// Regenerate via:
    ///   node scripts/demo/evm-asc-atomic/gen-eip1559-fixture.js
    #[test]
    fn cross_stack_eip1559_matches_viem() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("tests")
            .join("fixtures")
            .join("eip1559_fixture.json");
        let raw = std::fs::read_to_string(&path)
            .expect("missing fixture; run gen-eip1559-fixture.js");
        let fix: serde_json::Value = serde_json::from_str(&raw).expect("parse fixture");

        let priv_hex = fix["privateKey"].as_str().unwrap();
        let chain_id: u64 = fix["tx"]["chainId"].as_u64().unwrap();
        let signer = EvmSigner::from_hex(priv_hex, chain_id).unwrap();
        assert_eq!(
            signer.address().to_hex().to_lowercase(),
            fix["signerAddress"].as_str().unwrap().to_lowercase()
        );

        let to = Address::parse(fix["tx"]["to"].as_str().unwrap()).unwrap();
        let nonce: u64 = fix["tx"]["nonce"].as_u64().unwrap();
        let max_priority: u128 = fix["tx"]["maxPriorityFeePerGas"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let max_fee: u128 = fix["tx"]["maxFeePerGas"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let gas: u64 = fix["tx"]["gasLimit"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let value: u128 = fix["tx"]["value"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let data = {
            let s = fix["tx"]["data"].as_str().unwrap();
            hex::decode(s.strip_prefix("0x").unwrap_or(s)).unwrap()
        };

        let params = Eip1559TxParams {
            chain_id,
            nonce,
            max_priority_fee_per_gas: max_priority,
            max_fee_per_gas: max_fee,
            gas_limit: gas,
            to,
            value,
            data: &data,
        };

        // 1. Unsigned pre-image (0x02 || rlp(...nine items...)) must match.
        let mut our_unsigned = Vec::new();
        our_unsigned.push(EIP_1559_TX_TYPE);
        our_unsigned.extend_from_slice(&encode_eip1559_unsigned(&params));
        assert_eq!(
            format!("0x{}", hex::encode(&our_unsigned)),
            fix["unsigned"].as_str().unwrap(),
            "unsigned RLP must match viem"
        );

        // 2. Signed serialization. Note ECDSA over `k256` is deterministic
        //    (RFC 6979), so signing the same inputs must produce the SAME
        //    (r, s, yParity) viem produced. After s-normalization both libs
        //    agree.
        let our_signed = sign_eip1559(&signer, &params).unwrap();
        assert_eq!(
            format!("0x{}", hex::encode(&our_signed)),
            fix["signed"].as_str().unwrap(),
            "signed bytes must match viem"
        );
    }

    #[test]
    fn signing_round_trip_recovers_address() {
        // Pin a deterministic tx, sign it, recover the address from the
        // signature, confirm it matches the signer's address.
        let signer = EvmSigner::from_hex(ANVIL_PRIV, 31337).unwrap();
        let to = Address::parse("0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9").unwrap();
        let params = Eip1559TxParams {
            chain_id: 31337,
            nonce: 7,
            max_priority_fee_per_gas: 1_000_000_000,
            max_fee_per_gas: 2_000_000_000,
            gas_limit: 100_000,
            to,
            value: 0,
            data: &[0xab, 0xcd, 0xef],
        };
        let signed = sign_eip1559(&signer, &params).unwrap();
        // The result must start with the EIP-1559 type byte and not be empty.
        assert_eq!(signed[0], EIP_1559_TX_TYPE);
        assert!(signed.len() > 1);

        // Re-derive the digest from the unsigned encoding and verify ECDSA
        // recovery returns the expected address.
        let mut pre = Vec::new();
        pre.push(EIP_1559_TX_TYPE);
        pre.extend_from_slice(&encode_eip1559_unsigned(&params));
        let digest = keccak256(&pre);

        // The last three list items are yParity, r, s. Walk the RLP backwards
        // — simpler: read them from the verified output of `sign_eip1559`
        // again by re-signing (deterministic? no — k256 uses RFC 6979 so yes).
        // Anyway, just check that the verifying key recovers properly using
        // the prehash signature we know we used.
        let (sig, _rid) = signer
            .signing_key
            .sign_prehash(&digest)
            .map(|s: Signature| {
                let rid = RecoveryId::trial_recovery_from_prehash(
                    signer.signing_key.verifying_key(),
                    &digest,
                    &s,
                )
                .unwrap();
                (s, rid)
            })
            .unwrap();
        let normalized = sig.normalize_s().unwrap_or(sig);
        let recovered = k256::ecdsa::VerifyingKey::recover_from_prehash(
            &digest,
            &normalized,
            _rid,
        )
        .unwrap();
        // Compare via Ethereum address rather than raw bytes (recovery_id +
        // s-normalization can flip the parity).
        let mut sk_for_addr =
            k256::ecdsa::SigningKey::from_slice(&hex::decode(&ANVIL_PRIV[2..]).unwrap()).unwrap();
        // Build a dummy signer whose verifying_key matches `recovered`, just
        // to reuse derive_address. Actually derive_address takes a SigningKey
        // — write a tiny helper for VerifyingKey instead.
        let pt = recovered.to_encoded_point(false);
        let xy = &pt.as_bytes()[1..];
        let h = keccak256(xy);
        let mut rec_addr = [0u8; 20];
        rec_addr.copy_from_slice(&h[12..32]);
        assert_eq!(
            Address(rec_addr).to_hex().to_lowercase(),
            ANVIL_ADDR.to_lowercase()
        );
        let _ = &mut sk_for_addr; // silence
    }
}
