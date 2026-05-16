//! Secp256k1 adaptor signatures for the EVM ASC path.
//!
//! Mirror of [`crate::adaptor_sig`] but on secp256k1 with the noot-style
//! ecrecover-trick challenge format. The same signatures verify on chain
//! through [`chains/ethereum/src/SchnorrVerifier.sol`] and off chain in JS via
//! [`scripts/demo/evm-asc-atomic/adaptor.js`].
//!
//! ## Math (matching the JS implementation byte-for-byte)
//!
//! Constraints on the signer's pubkey: `py` even (parity byte = `0x1b`) and
//! `px < HALF_Q`. [`derive_normalized_keypair`] always returns a key matching
//! these constraints, re-hashing the seed if necessary.
//!
//! ```text
//! pSign(x, m, T):
//!   r' ← Z_q  (random)
//!   R' ← r' · G
//!   R  ← R' + T
//!   R_addr ← address(uncompressed R)                 // last 20 bytes of keccak(x||y)
//!   e  ← keccak256(R_addr || 0x1b || px || m) mod q
//!   s' ← r' + e · x  (mod q)
//!   return (R', s')
//!
//! pVerify(pk, m, T, (R', s')):
//!   R ← R' + T
//!   e ← keccak256(R_addr || 0x1b || px || m) mod q
//!   check s'·G == R' + e · pk
//!
//! adapt((R', s'), t):
//!   R ← R' + t·G
//!   s ← s' + t  (mod q)
//!   return (R, s)
//!
//! extract((R', s'), (R, s)):
//!   t ← s - s'  (mod q)
//!
//! verify_full(pk, m, (R, s)):
//!   e ← keccak256(R_addr || 0x1b || px || m) mod q
//!   check s·G == R + e · pk
//! ```
//!
//! ## Why a separate module vs `adaptor_sig`
//!
//! - Different curve (secp256k1 vs Ed25519).
//! - Different challenge form (noot ecrecover trick vs Ed25519 SHA-512).
//! - Different parity / range restriction on the signer pubkey.
//!
//! Sharing trait abstractions over both was considered and rejected — the
//! Ed25519 module has no parity / x-range constraints, and abstracting
//! over the two cleanly costs more code than the duplication saves.

use k256::elliptic_curve::ops::Reduce;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::elliptic_curve::PrimeField;
use k256::{AffinePoint, ProjectivePoint, Scalar, U256};
use rand::rngs::OsRng;
use rand::RngCore;
use sha3::{Digest, Keccak256};

/// Parity byte for an even-`y` public key. Matches the on-chain
/// `SchnorrVerifier` which always passes `27` as the ecrecover `v`.
pub const PARITY_EVEN_Y: u8 = 27;

/// Tag prepended to the seed when deriving the AES key from a witness `t`.
const SYM_KEY_TAG: &[u8] = b"a402-atomic-result-v1";

/// Adaptor pre-signature: (R', s') where R' is the unadapted nonce point.
///
/// Note: Serde derives are intentionally omitted — `[u8; 33]` exceeds the
/// builtin Serde array size limit. Internal callers use the struct directly;
/// wire encoders should pack into hex/base64 manually if needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdaptorPreSignature {
    /// R' = r' · G — encoded as a 33-byte SEC1 compressed point.
    pub r_prime: [u8; 33],
    /// s' = r' + e · sk (mod q), big-endian 32 bytes.
    pub s_prime: [u8; 32],
}

/// A completed Schnorr signature `(R, s)` produced by adapting a pre-sig with
/// the witness scalar `t`. Verifiable off-chain by [`verify_full`] and on-chain
/// by `SchnorrVerifier.sol`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullSignature {
    /// R = R' + t · G — 33-byte SEC1 compressed.
    pub r: [u8; 33],
    /// s = s' + t (mod q), big-endian 32 bytes.
    pub s: [u8; 32],
}

/// Normalized signer keypair satisfying `py` even and `px < HALF_Q`.
#[derive(Debug, Clone)]
pub struct NormalizedKeypair {
    /// The (possibly negated) private scalar.
    pub secret: Scalar,
    /// Serialized big-endian secret, for ergonomic FFI.
    pub secret_bytes: [u8; 32],
    /// The public point `secret · G`.
    pub public: ProjectivePoint,
    /// Affine `x` of the public point (big-endian 32-byte form).
    pub px_bytes: [u8; 32],
    /// Affine `y` of the public point (big-endian 32-byte form).
    pub py_bytes: [u8; 32],
    /// Number of hash iterations [`derive_normalized_keypair`] took to find a
    /// valid key. Surfaced for test debugging.
    pub iterations: u32,
}

/// On-chain proof tuple consumed by `SchnorrVerifier.verifySignature(px, e, s, message)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnChainProof {
    pub px: [u8; 32],
    pub e: [u8; 32],
    pub s: [u8; 32],
}

/// Errors raised by the adaptor signature operations.
#[derive(Debug, thiserror::Error)]
pub enum AdaptorError {
    #[error("scalar out of range (0 < x < q required)")]
    ScalarOutOfRange,
    #[error("public key violates SchnorrVerifier constraint (px < HALF_Q, py even)")]
    PublicKeyConstraint,
    #[error("invalid SEC1-encoded point")]
    InvalidPoint,
    #[error("derive_normalized_keypair exhausted 256 attempts")]
    KeypairExhausted,
    #[error("invalid signature")]
    InvalidSignature,
}

/// Treats `seed` as input material and deterministically searches for a
/// private key whose public point satisfies the verifier constraints. The
/// function re-keccaks the seed if the candidate fails. Always succeeds
/// for any honestly-distributed 32-byte seed within ~10 iterations.
pub fn derive_normalized_keypair(seed: &[u8]) -> Result<NormalizedKeypair, AdaptorError> {
    let mut bytes = seed.to_vec();
    for iteration in 0..256u32 {
        let candidate = scalar_from_bytes_mod_order(&bytes);
        if candidate != Scalar::ZERO {
            let pub_point = ProjectivePoint::GENERATOR * candidate;
            let aff = pub_point.to_affine();
            let (px, py_is_odd) = affine_xy(&aff);

            // First normalize parity by potentially negating the secret.
            let (secret, public, px_bytes, py_bytes, py_odd) = if py_is_odd {
                let neg = -candidate;
                let neg_pub = ProjectivePoint::GENERATOR * neg;
                let aff2 = neg_pub.to_affine();
                let (px2, py_odd2) = affine_xy(&aff2);
                (
                    neg,
                    neg_pub,
                    bigint_to_be_bytes(&px2),
                    affine_y_be_bytes(&aff2),
                    py_odd2,
                )
            } else {
                (
                    candidate,
                    pub_point,
                    bigint_to_be_bytes(&px),
                    affine_y_be_bytes(&aff),
                    py_is_odd,
                )
            };

            // Then check that px is in [0, HALF_Q). If not, rehash and retry.
            if !py_odd && px_lt_half_q(&px_bytes) {
                let secret_bytes = scalar_to_be_bytes(&secret);
                return Ok(NormalizedKeypair {
                    secret,
                    secret_bytes,
                    public,
                    px_bytes,
                    py_bytes,
                    iterations: iteration,
                });
            }
        }
        // Keccak-rehash to try a fresh candidate.
        let mut hasher = Keccak256::new();
        hasher.update(&bytes);
        bytes = hasher.finalize().to_vec();
    }
    Err(AdaptorError::KeypairExhausted)
}

/// Generates a fresh adaptor witness scalar `t` along with its public point
/// `T = t · G`. `t` is uniform in `[1, q)`.
pub fn random_witness() -> (Scalar, ProjectivePoint) {
    loop {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let candidate = scalar_from_bytes_mod_order(&bytes);
        if candidate != Scalar::ZERO {
            let public = ProjectivePoint::GENERATOR * candidate;
            return (candidate, public);
        }
    }
}

/// Produces the Schnorr adaptor pre-signature `σ̂` over `message` under
/// statement `T = t · G`.
///
/// `secret` and `public` must be a [`NormalizedKeypair`] (parity + range
/// constraints enforced). `T` is the public form of the witness; the signer
/// does not need to know `t` itself.
pub fn p_sign(
    keypair: &NormalizedKeypair,
    message: &[u8; 32],
    big_t: &ProjectivePoint,
) -> AdaptorPreSignature {
    loop {
        let r_prime_scalar = {
            let mut b = [0u8; 32];
            OsRng.fill_bytes(&mut b);
            scalar_from_bytes_mod_order(&b)
        };
        if r_prime_scalar == Scalar::ZERO {
            continue;
        }
        let r_prime = ProjectivePoint::GENERATOR * r_prime_scalar;
        let r = r_prime + *big_t;
        if r == ProjectivePoint::IDENTITY {
            continue;
        }

        let e = match challenge(&r, &keypair.px_bytes, message) {
            Some(e) => e,
            None => continue,
        };
        let s_prime = r_prime_scalar + e * keypair.secret;
        if s_prime == Scalar::ZERO {
            continue;
        }
        return AdaptorPreSignature {
            r_prime: compress_point(&r_prime),
            s_prime: scalar_to_be_bytes(&s_prime),
        };
    }
}

/// Verifies the adaptor pre-signature `σ̂` is well-formed under `pk`, `T`, and
/// `message`. Success means: given the witness `t` (where `T = t·G`), the
/// pre-sig can be adapted into a Schnorr signature accepted by [`verify_full`]
/// and by the on-chain `SchnorrVerifier`.
pub fn p_verify(
    public: &ProjectivePoint,
    message: &[u8; 32],
    big_t: &ProjectivePoint,
    pre: &AdaptorPreSignature,
) -> bool {
    let aff = public.to_affine();
    let (px, py_odd) = affine_xy(&aff);
    let px_bytes = bigint_to_be_bytes(&px);
    if py_odd || !px_lt_half_q(&px_bytes) {
        return false;
    }
    let s_prime = match scalar_from_be_bytes_strict(&pre.s_prime) {
        Some(s) => s,
        None => return false,
    };
    if s_prime == Scalar::ZERO {
        return false;
    }
    let r_prime = match decompress_point(&pre.r_prime) {
        Some(p) => p,
        None => return false,
    };
    let r = r_prime + *big_t;
    if r == ProjectivePoint::IDENTITY {
        return false;
    }
    let e = match challenge(&r, &px_bytes, message) {
        Some(e) => e,
        None => return false,
    };
    let lhs = ProjectivePoint::GENERATOR * s_prime;
    let rhs = r_prime + (*public) * e;
    lhs == rhs
}

/// Adapts a pre-signature into a full Schnorr signature using witness `t`.
pub fn adapt(pre: &AdaptorPreSignature, t: &Scalar) -> Result<FullSignature, AdaptorError> {
    let s_prime = scalar_from_be_bytes_strict(&pre.s_prime).ok_or(AdaptorError::InvalidSignature)?;
    let r_prime = decompress_point(&pre.r_prime).ok_or(AdaptorError::InvalidPoint)?;
    let r = r_prime + (ProjectivePoint::GENERATOR * *t);
    let s = s_prime + *t;
    Ok(FullSignature {
        r: compress_point(&r),
        s: scalar_to_be_bytes(&s),
    })
}

/// Recovers the witness scalar `t` from a pre-signature and its adapted
/// counterpart, exploiting `s = s' + t  (mod q)`.
pub fn extract(pre: &AdaptorPreSignature, full: &FullSignature) -> Result<Scalar, AdaptorError> {
    let s_prime = scalar_from_be_bytes_strict(&pre.s_prime).ok_or(AdaptorError::InvalidSignature)?;
    let s = scalar_from_be_bytes_strict(&full.s).ok_or(AdaptorError::InvalidSignature)?;
    Ok(s - s_prime)
}

/// Verifies a completed Schnorr signature `(R, s)` under the same constraints
/// as the on-chain verifier.
pub fn verify_full(public: &ProjectivePoint, message: &[u8; 32], full: &FullSignature) -> bool {
    let aff = public.to_affine();
    let (px, py_odd) = affine_xy(&aff);
    let px_bytes = bigint_to_be_bytes(&px);
    if py_odd || !px_lt_half_q(&px_bytes) {
        return false;
    }
    let s = match scalar_from_be_bytes_strict(&full.s) {
        Some(s) => s,
        None => return false,
    };
    if s == Scalar::ZERO {
        return false;
    }
    let r = match decompress_point(&full.r) {
        Some(p) => p,
        None => return false,
    };
    let e = match challenge(&r, &px_bytes, message) {
        Some(e) => e,
        None => return false,
    };
    let lhs = ProjectivePoint::GENERATOR * s;
    let rhs = r + (*public) * e;
    lhs == rhs
}

/// Packs a completed Schnorr signature into the tuple `SchnorrVerifier.sol`
/// consumes: `(px, e, s)` as 32-byte big-endian integers.
pub fn build_onchain_proof(
    public: &ProjectivePoint,
    message: &[u8; 32],
    full: &FullSignature,
) -> Result<OnChainProof, AdaptorError> {
    let aff = public.to_affine();
    let (px, py_odd) = affine_xy(&aff);
    let px_bytes = bigint_to_be_bytes(&px);
    if py_odd || !px_lt_half_q(&px_bytes) {
        return Err(AdaptorError::PublicKeyConstraint);
    }
    let r = decompress_point(&full.r).ok_or(AdaptorError::InvalidPoint)?;
    let e = challenge(&r, &px_bytes, message).ok_or(AdaptorError::InvalidSignature)?;
    Ok(OnChainProof {
        px: px_bytes,
        e: scalar_to_be_bytes(&e),
        s: full.s,
    })
}

/* -------------------------------------------------------------------------- */
/*                                Helpers                                     */
/* -------------------------------------------------------------------------- */

/// Recomputes the noot challenge `e = keccak256(R_addr || 0x1b || px || msg) mod q`.
fn challenge(r: &ProjectivePoint, px_bytes: &[u8; 32], message: &[u8; 32]) -> Option<Scalar> {
    let r_aff = r.to_affine();
    let r_addr = point_to_eth_address(&r_aff);
    let mut buf = Vec::with_capacity(20 + 1 + 32 + 32);
    buf.extend_from_slice(&r_addr);
    buf.push(PARITY_EVEN_Y);
    buf.extend_from_slice(px_bytes);
    buf.extend_from_slice(message);
    let mut hasher = Keccak256::new();
    hasher.update(&buf);
    let digest = hasher.finalize();
    let mut be = [0u8; 32];
    be.copy_from_slice(digest.as_slice());
    let e = scalar_from_bytes_mod_order(&be);
    if e == Scalar::ZERO {
        None
    } else {
        Some(e)
    }
}

/// Ethereum-address encoding of a curve point: `keccak256(x || y)[12..]`.
fn point_to_eth_address(aff: &AffinePoint) -> [u8; 20] {
    let xy_be = affine_xy_be_bytes(aff);
    let mut hasher = Keccak256::new();
    hasher.update(&xy_be);
    let digest = hasher.finalize();
    let mut out = [0u8; 20];
    out.copy_from_slice(&digest[12..]);
    out
}

/// Returns the affine `(x, y)` as 64 big-endian bytes (no SEC1 prefix).
fn affine_xy_be_bytes(aff: &AffinePoint) -> [u8; 64] {
    let encoded = aff.to_encoded_point(false);
    let bytes = encoded.as_bytes();
    debug_assert_eq!(bytes.len(), 65);
    debug_assert_eq!(bytes[0], 0x04);
    let mut out = [0u8; 64];
    out.copy_from_slice(&bytes[1..]);
    out
}

/// Returns `(x_bigint, y_is_odd)` for an affine point.
fn affine_xy(aff: &AffinePoint) -> (U256, bool) {
    // SEC1 compressed encoding's first byte carries the parity (0x02=even, 0x03=odd).
    let encoded = aff.to_encoded_point(true);
    let bytes = encoded.as_bytes();
    debug_assert_eq!(bytes.len(), 33);
    let y_odd = bytes[0] == 0x03;
    let x_bytes: [u8; 32] = bytes[1..33].try_into().expect("x is 32 bytes");
    let x = U256::from_be_slice(&x_bytes);
    (x, y_odd)
}

fn affine_y_be_bytes(aff: &AffinePoint) -> [u8; 32] {
    let xy = affine_xy_be_bytes(aff);
    let mut out = [0u8; 32];
    out.copy_from_slice(&xy[32..]);
    out
}

pub fn bigint_to_be_bytes(u: &U256) -> [u8; 32] {
    // `Encoding::to_be_bytes` returns the type's `Repr`, which for U256 is
    // `[u8; 32]`.
    use k256::elliptic_curve::bigint::Encoding;
    u.to_be_bytes()
}

pub fn scalar_to_be_bytes(s: &Scalar) -> [u8; 32] {
    let arr = s.to_bytes();
    arr.into()
}

fn scalar_from_bytes_mod_order(bytes: &[u8]) -> Scalar {
    // 32-byte reduce: if input is shorter, pad. k256's Scalar::reduce treats
    // the 256-bit input modulo q.
    let mut be = [0u8; 32];
    let start = if bytes.len() >= 32 { bytes.len() - 32 } else { 0 };
    let copy_len = bytes.len().min(32);
    be[32 - copy_len..].copy_from_slice(&bytes[start..start + copy_len]);
    Scalar::reduce(U256::from_be_slice(&be))
}

pub fn scalar_from_be_bytes_strict(bytes: &[u8; 32]) -> Option<Scalar> {
    use k256::FieldBytes;
    let fb = FieldBytes::clone_from_slice(bytes);
    let ct = Scalar::from_repr(fb);
    // `Scalar::from_repr` returns `CtOption<Scalar>` rejecting values >= q.
    if bool::from(ct.is_some()) {
        Some(ct.unwrap())
    } else {
        None
    }
}

/// Returns the SEC1-compressed encoding of `p` (33 bytes).
pub fn compress_point(p: &ProjectivePoint) -> [u8; 33] {
    let encoded = p.to_affine().to_encoded_point(true);
    let bytes = encoded.as_bytes();
    debug_assert_eq!(bytes.len(), 33);
    let mut out = [0u8; 33];
    out.copy_from_slice(bytes);
    out
}

pub fn decompress_point(bytes: &[u8; 33]) -> Option<ProjectivePoint> {
    use k256::EncodedPoint;
    let encoded = EncodedPoint::from_bytes(bytes).ok()?;
    let aff = AffinePoint::try_from(&encoded).ok()?;
    Some(aff.into())
}

/// Returns true iff `px < HALF_Q` where `HALF_Q = (q >> 1) + 1` matches
/// `SchnorrVerifier.sol`'s constant.
fn px_lt_half_q(px_be: &[u8; 32]) -> bool {
    // HALF_Q = (Q + 1) / 2 = 0x7FFFFFFF...A6177D5BAF572419AE57B36800B6F4F31C32A0DE2EA68B0CE671CD11A2 ...
    // For correctness, encode the constant explicitly from the curve order.
    use k256::elliptic_curve::Curve as _;
    let q: U256 = k256::Secp256k1::ORDER;
    let one = U256::ONE;
    let half_q = q.shr(1).wrapping_add(&one);
    let px = U256::from_be_slice(px_be);
    px < half_q
}

/* -------------------------------------------------------------------------- */
/*                       AES-GCM result encryption (EncRes)                    */
/* -------------------------------------------------------------------------- */

/// Encrypted result envelope produced by [`encrypt_result`]. The AES-GCM key
/// is `keccak256("a402-atomic-result-v1" || t_be)` so any party that learns
/// the witness `t` can decrypt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedResult {
    pub iv: [u8; 12],
    pub ciphertext: Vec<u8>,
    pub tag: [u8; 16],
}

/// Derives the AES-256-GCM key from the witness scalar.
pub fn derive_sym_key(t: &Scalar) -> [u8; 32] {
    let t_be = scalar_to_be_bytes(t);
    let mut hasher = Keccak256::new();
    hasher.update(SYM_KEY_TAG);
    hasher.update(t_be);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

pub fn encrypt_result(plaintext: &[u8], t: &Scalar) -> EncryptedResult {
    use aes_gcm::aead::{Aead, KeyInit, OsRng as AesOsRng};
    use aes_gcm::{AeadCore, Aes256Gcm, Nonce};
    let key_bytes = derive_sym_key(t);
    let cipher = Aes256Gcm::new_from_slice(&key_bytes).expect("256-bit key");
    let nonce_bytes = Aes256Gcm::generate_nonce(&mut AesOsRng);
    let mut combined = cipher
        .encrypt(&nonce_bytes, plaintext)
        .expect("AES-GCM encrypt");
    // aes_gcm appends the 16-byte tag at the end of the ciphertext.
    let tag_start = combined.len() - 16;
    let tag_vec = combined.split_off(tag_start);
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&tag_vec);
    let mut iv = [0u8; 12];
    iv.copy_from_slice(Nonce::<<Aes256Gcm as AeadCore>::NonceSize>::from_slice(
        &nonce_bytes,
    ));
    EncryptedResult {
        iv,
        ciphertext: combined,
        tag,
    }
}

pub fn decrypt_result(encrypted: &EncryptedResult, t: &Scalar) -> Result<Vec<u8>, AdaptorError> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let key_bytes = derive_sym_key(t);
    let cipher = Aes256Gcm::new_from_slice(&key_bytes).expect("256-bit key");
    let mut ct = encrypted.ciphertext.clone();
    ct.extend_from_slice(&encrypted.tag);
    let nonce = Nonce::<<Aes256Gcm as aes_gcm::AeadCore>::NonceSize>::from_slice(&encrypted.iv);
    cipher
        .decrypt(nonce, ct.as_slice())
        .map_err(|_| AdaptorError::InvalidSignature)
}

/* -------------------------------------------------------------------------- */
/*                                  Tests                                     */
/* -------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;

    fn rand_seed(seed: u64) -> [u8; 32] {
        let mut out = [0u8; 32];
        let mut hasher = Keccak256::new();
        hasher.update(seed.to_be_bytes());
        out.copy_from_slice(hasher.finalize().as_slice());
        out
    }

    fn rand_message(label: &str) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(label.as_bytes());
        let d = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&d);
        out
    }

    #[test]
    fn normalize_returns_even_y_and_px_below_half_q() {
        for i in 0..10u64 {
            let kp = derive_normalized_keypair(&rand_seed(i)).expect("derive");
            // even y: parity-byte check via SEC1 compressed encoding
            let aff = kp.public.to_affine();
            let enc = aff.to_encoded_point(true);
            assert_eq!(enc.as_bytes()[0], 0x02, "py must be even after normalize");
            assert!(px_lt_half_q(&kp.px_bytes), "px must be < HALF_Q");
        }
    }

    #[test]
    fn p_sign_then_p_verify_accepts_honest_presig() {
        let kp = derive_normalized_keypair(&rand_seed(42)).unwrap();
        let (t, big_t) = random_witness();
        let msg = rand_message("hello");
        let pre = p_sign(&kp, &msg, &big_t);
        assert!(p_verify(&kp.public, &msg, &big_t, &pre));
        let _ = t; // silence unused
    }

    #[test]
    fn p_verify_rejects_wrong_t() {
        let kp = derive_normalized_keypair(&rand_seed(43)).unwrap();
        let (_t, big_t) = random_witness();
        let (_t_bad, big_t_bad) = random_witness();
        let msg = rand_message("rejects-wrong-T");
        let pre = p_sign(&kp, &msg, &big_t);
        assert!(!p_verify(&kp.public, &msg, &big_t_bad, &pre));
    }

    #[test]
    fn adapt_produces_signature_verify_full_accepts() {
        let kp = derive_normalized_keypair(&rand_seed(44)).unwrap();
        let (t, big_t) = random_witness();
        let msg = rand_message("adapt-then-verify");
        let pre = p_sign(&kp, &msg, &big_t);
        let full = adapt(&pre, &t).expect("adapt");
        assert!(verify_full(&kp.public, &msg, &full));
    }

    #[test]
    fn extract_recovers_exact_t() {
        let kp = derive_normalized_keypair(&rand_seed(45)).unwrap();
        let (t, big_t) = random_witness();
        let msg = rand_message("extract-back");
        let pre = p_sign(&kp, &msg, &big_t);
        let full = adapt(&pre, &t).unwrap();
        let recovered = extract(&pre, &full).unwrap();
        assert_eq!(recovered, t);
    }

    #[test]
    fn onchain_proof_recomputes_consistent_challenge() {
        let kp = derive_normalized_keypair(&rand_seed(46)).unwrap();
        let (t, big_t) = random_witness();
        let msg = rand_message("onchain-pack");
        let pre = p_sign(&kp, &msg, &big_t);
        let full = adapt(&pre, &t).unwrap();
        let proof = build_onchain_proof(&kp.public, &msg, &full).unwrap();
        // The on-chain check is: e == keccak256(R || 0x1b || px || msg) mod q.
        let r = decompress_point(&full.r).unwrap();
        let recomputed = challenge(&r, &proof.px, &msg).unwrap();
        assert_eq!(proof.e, scalar_to_be_bytes(&recomputed));
    }

    #[test]
    fn aes_gcm_round_trip() {
        let (t, _) = random_witness();
        let plaintext = b"{\"temperature\":72,\"conditions\":\"clear\"}";
        let ct = encrypt_result(plaintext, &t);
        let pt = decrypt_result(&ct, &t).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn aes_gcm_wrong_key_fails() {
        let (t_good, _) = random_witness();
        let (t_bad, _) = random_witness();
        let ct = encrypt_result(b"secret", &t_good);
        assert!(decrypt_result(&ct, &t_bad).is_err());
    }

    /// Cross-stack agreement test: consumes the deterministic JSON fixture
    /// produced by `scripts/demo/evm-asc-atomic/gen-fixture.js` and verifies
    /// that the Rust implementation reaches byte-identical conclusions about
    /// every derived value (σ_S, extracted t, on-chain proof).
    ///
    /// If this passes, the same Schnorr proof that JS produces will verify
    /// against `SchnorrVerifier.sol` no matter which language generated it —
    /// which is the whole point of having a Rust enclave alongside the JS
    /// demo.
    ///
    /// To regenerate the fixture: `node scripts/demo/evm-asc-atomic/gen-fixture.js`
    #[test]
    fn cross_stack_fixture_matches_js() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("tests")
            .join("fixtures")
            .join("adaptor_sig_secp_fixture.json");

        let raw = std::fs::read_to_string(&fixture_path)
            .expect("missing fixture; run `node scripts/demo/evm-asc-atomic/gen-fixture.js`");
        let fixture: serde_json::Value = serde_json::from_str(&raw).expect("parse fixture");

        let parse_hex32 = |s: &str| -> [u8; 32] {
            let h = s.strip_prefix("0x").unwrap_or(s);
            let mut out = [0u8; 32];
            let bytes = hex::decode(h).expect("hex32 decode");
            assert_eq!(bytes.len(), 32);
            out.copy_from_slice(&bytes);
            out
        };
        let parse_hex33 = |s: &str| -> [u8; 33] {
            let h = s.strip_prefix("0x").unwrap_or(s);
            let mut out = [0u8; 33];
            let bytes = hex::decode(h).expect("hex33 decode");
            assert_eq!(bytes.len(), 33);
            out.copy_from_slice(&bytes);
            out
        };

        // Inputs from the fixture
        let seed = {
            let h = fixture["seed"].as_str().unwrap();
            let bytes = hex::decode(h.strip_prefix("0x").unwrap()).unwrap();
            bytes
        };
        let message = parse_hex32(fixture["message"].as_str().unwrap());
        let t_bytes = parse_hex32(fixture["witness"]["t"].as_str().unwrap());
        let t_scalar = scalar_from_be_bytes_strict(&t_bytes).expect("t in [0,q)");

        // Reconstruct the same normalized keypair the JS produced.
        let kp = derive_normalized_keypair(&seed).expect("derive normalized keypair");
        assert_eq!(
            kp.px_bytes,
            parse_hex32(fixture["signer"]["px"].as_str().unwrap()),
            "px must match across stacks"
        );
        assert_eq!(
            kp.secret_bytes,
            parse_hex32(fixture["signer"]["secret"].as_str().unwrap()),
            "normalized secret must match across stacks"
        );

        // Witness commitment must match.
        let big_t = ProjectivePoint::GENERATOR * t_scalar;
        let big_t_compressed = compress_point(&big_t);
        let expected_t = parse_hex33(fixture["witness"]["T_compressed"].as_str().unwrap());
        assert_eq!(big_t_compressed, expected_t, "T = t·G must match across stacks");

        // Consume the JS-produced pre-signature; do not regenerate (p_sign is
        // randomized, byte-exact match would require pinning r' too).
        let pre = AdaptorPreSignature {
            r_prime: parse_hex33(fixture["pre_signature"]["r_prime_compressed"].as_str().unwrap()),
            s_prime: parse_hex32(fixture["pre_signature"]["s_prime"].as_str().unwrap()),
        };

        // 1. Rust's p_verify accepts the JS-produced σ̂_S.
        assert!(
            p_verify(&kp.public, &message, &big_t, &pre),
            "Rust p_verify must accept the JS-produced σ̂_S"
        );

        // 2. adapt(σ̂_S, t) must reproduce the same full Schnorr sig byte-for-byte.
        let full = adapt(&pre, &t_scalar).expect("adapt");
        assert_eq!(
            full.r,
            parse_hex33(fixture["full_signature"]["r_compressed"].as_str().unwrap()),
            "adapted R must match across stacks"
        );
        assert_eq!(
            full.s,
            parse_hex32(fixture["full_signature"]["s"].as_str().unwrap()),
            "adapted s must match across stacks"
        );

        // 3. verify_full accepts; extract recovers t.
        assert!(verify_full(&kp.public, &message, &full));
        let recovered = extract(&pre, &full).unwrap();
        assert_eq!(recovered, t_scalar, "extracted t must equal original");

        // 4. on-chain proof tuple matches the JS-produced (px, e, s).
        let proof = build_onchain_proof(&kp.public, &message, &full).unwrap();
        assert_eq!(
            proof.px,
            parse_hex32(fixture["onchain_proof"]["px"].as_str().unwrap())
        );
        assert_eq!(
            proof.e,
            parse_hex32(fixture["onchain_proof"]["e"].as_str().unwrap())
        );
        assert_eq!(
            proof.s,
            parse_hex32(fixture["onchain_proof"]["s"].as_str().unwrap())
        );
    }
}
