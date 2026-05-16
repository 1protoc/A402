//! Client-side cryptographic verification for the atomic exchange.
//!
//! The flow this module supports (mirroring  Algorithms 2–3):
//!
//!   1. SP returns `(asc_state_hash, T, σ̂_S, EncRes)` to `/v1/sp/request`.
//!   2. Client calls [`parse_atomic_resp`] to decode the wire bytes back
//!      into typed crypto primitives (`ProjectivePoint`, `AdaptorPreSignature`,
//!      `EncryptedResult`).
//!   3. Client calls [`verify_pre_sig`] which runs `p_verify` under the
//!      SP's advertised Schnorr public key. If it fails, abort — the SP
//!      misbehaved.
//!   4. Client ECDSA-signs `asc_state_hash` (`σ_C`) and submits it to
//!      `/v1/sp/finalize`. SP responds with `(t, σ_S)`.
//!   5. Client calls [`finalize_and_decrypt`] which:
//!        - checks `t · G == T` (witness consistency),
//!        - adapts `σ̂_S → σ_S` via `adapt(pre, t)` and runs `verify_full`,
//!        - decrypts `EncRes` under the AES key derived from `t`,
//!        - returns the plaintext service response.

use a402_shared::adaptor_sig_secp::{
    self, AdaptorPreSignature, EncryptedResult, FullSignature,
};
use k256::{ProjectivePoint, Scalar};

use crate::error::ClientError;
use crate::sp_http::{AtomicResp, FinalizeResp};

/// Parsed `AtomicResp` ready for client-side verification.
pub struct ParsedAtomicResp {
    pub asc_state_hash: [u8; 32],
    pub new_balance_c: u128,
    pub new_balance_s: u128,
    pub new_version: u64,
    pub big_t: ProjectivePoint,
    pub pre_sig: AdaptorPreSignature,
    pub enc_res: EncryptedResult,
}

pub fn parse_atomic_resp(resp: &AtomicResp) -> Result<ParsedAtomicResp, ClientError> {
    let asc_state_hash = parse_b32(&resp.asc_state_hash, "ascStateHash")?;

    let new_balance_c: u128 = resp
        .new_balance_c
        .parse()
        .map_err(|e: std::num::ParseIntError| {
            ClientError::InvalidSpResponse(format!("newBalanceC: {e}"))
        })?;
    let new_balance_s: u128 = resp
        .new_balance_s
        .parse()
        .map_err(|e: std::num::ParseIntError| {
            ClientError::InvalidSpResponse(format!("newBalanceS: {e}"))
        })?;

    let big_t_bytes = parse_b33(&resp.big_t, "bigT")?;
    let big_t = adaptor_sig_secp::decompress_point(&big_t_bytes)
        .ok_or_else(|| ClientError::InvalidSpResponse("bigT not on curve".to_string()))?;

    let r_prime = parse_b33(&resp.sig_hat_r_prime, "sigHatRPrime")?;
    let s_prime = parse_b32(&resp.sig_hat_s_prime, "sigHatSPrime")?;
    let pre_sig = AdaptorPreSignature { r_prime, s_prime };

    let iv_full = parse_hex(&resp.enc_iv, "encIv")?;
    if iv_full.len() != 12 {
        return Err(ClientError::InvalidSpResponse(format!(
            "encIv must be 12 bytes, got {}",
            iv_full.len()
        )));
    }
    let mut iv = [0u8; 12];
    iv.copy_from_slice(&iv_full);
    let ciphertext = parse_hex(&resp.enc_ciphertext, "encCiphertext")?;
    let tag_full = parse_hex(&resp.enc_tag, "encTag")?;
    if tag_full.len() != 16 {
        return Err(ClientError::InvalidSpResponse(format!(
            "encTag must be 16 bytes, got {}",
            tag_full.len()
        )));
    }
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&tag_full);
    let enc_res = EncryptedResult {
        iv,
        ciphertext,
        tag,
    };

    Ok(ParsedAtomicResp {
        asc_state_hash,
        new_balance_c,
        new_balance_s,
        new_version: resp.new_version,
        big_t,
        pre_sig,
        enc_res,
    })
}

/// Reconstruct the SP's normalized Schnorr public point from its 32-byte
/// `schnorrPx` hex (the only thing exposed on `/v1/sp/info`).
///
/// The normalization invariant — `py` even, `px < HALF_Q` — means a single
/// `0x02`-prefix decompress recovers a unique point. If the SP advertises a
/// `schnorrPx` that doesn't sit on the curve under even-y, we refuse.
pub fn reconstruct_seller_pubkey(schnorr_px_hex: &str) -> Result<ProjectivePoint, ClientError> {
    let px = parse_b32(schnorr_px_hex, "schnorrPx")?;
    let mut compressed = [0u8; 33];
    compressed[0] = 0x02; // even-y prefix
    compressed[1..].copy_from_slice(&px);
    adaptor_sig_secp::decompress_point(&compressed)
        .ok_or_else(|| ClientError::InvalidSpResponse("schnorrPx not on curve under even-y".to_string()))
}

/// Verifies the adaptor pre-signature. Wraps `adaptor_sig_secp::p_verify`.
pub fn verify_pre_sig(
    seller_pub: &ProjectivePoint,
    parsed: &ParsedAtomicResp,
) -> Result<(), ClientError> {
    if adaptor_sig_secp::p_verify(seller_pub, &parsed.asc_state_hash, &parsed.big_t, &parsed.pre_sig) {
        Ok(())
    } else {
        Err(ClientError::PreSigInvalid)
    }
}

/// Parse the SP's finalize response into a scalar `t` plus the 65-byte ECDSA
/// `σ_S` from the SP.
pub fn parse_finalize_resp(resp: &FinalizeResp) -> Result<(Scalar, [u8; 65]), ClientError> {
    let t_bytes = parse_b32(&resp.t, "t")?;
    let t = adaptor_sig_secp::scalar_from_be_bytes_strict(&t_bytes)
        .ok_or_else(|| ClientError::InvalidSpResponse("t out of curve order".to_string()))?;
    let sig_bytes = parse_hex(&resp.sig_s, "sigS")?;
    if sig_bytes.len() != 65 {
        return Err(ClientError::InvalidSpResponse(format!(
            "sigS must be 65 bytes, got {}",
            sig_bytes.len()
        )));
    }
    let mut sig_s = [0u8; 65];
    sig_s.copy_from_slice(&sig_bytes);
    Ok((t, sig_s))
}

/// Final verification + decryption. On success returns the plaintext
/// service response. Performs three checks in order:
///   1. `t · G == T`  (the witness opens the same statement)
///   2. `verify_full(pk_S, asc_state_hash, adapt(σ̂_S, t))` (σ_S is valid)
///   3. AES-GCM decryption succeeds under the key derived from `t`.
pub fn finalize_and_decrypt(
    seller_pub: &ProjectivePoint,
    parsed: &ParsedAtomicResp,
    t: &Scalar,
) -> Result<(FullSignature, Vec<u8>), ClientError> {
    let big_t_check = ProjectivePoint::GENERATOR * *t;
    if big_t_check != parsed.big_t {
        return Err(ClientError::WitnessMismatch);
    }

    let full = adaptor_sig_secp::adapt(&parsed.pre_sig, t)
        .map_err(|e| ClientError::Signature(format!("adapt: {e}")))?;
    if !adaptor_sig_secp::verify_full(seller_pub, &parsed.asc_state_hash, &full) {
        return Err(ClientError::FullSigInvalid);
    }

    let plaintext = adaptor_sig_secp::decrypt_result(&parsed.enc_res, t)
        .map_err(|_| ClientError::DecryptFailed)?;
    Ok((full, plaintext))
}

/* -------------------------------------------------------------------------- */
/*                                Hex helpers                                  */
/* -------------------------------------------------------------------------- */

fn parse_hex(input: &str, label: &str) -> Result<Vec<u8>, ClientError> {
    let stripped = input.strip_prefix("0x").unwrap_or(input);
    hex::decode(stripped).map_err(|e| ClientError::Hex(format!("{label}: {e}")))
}

fn parse_b32(input: &str, label: &str) -> Result<[u8; 32], ClientError> {
    let raw = parse_hex(input, label)?;
    if raw.len() != 32 {
        return Err(ClientError::InvalidSpResponse(format!(
            "{label} must be 32 bytes, got {}",
            raw.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

fn parse_b33(input: &str, label: &str) -> Result<[u8; 33], ClientError> {
    let raw = parse_hex(input, label)?;
    if raw.len() != 33 {
        return Err(ClientError::InvalidSpResponse(format!(
            "{label} must be 33 bytes, got {}",
            raw.len()
        )));
    }
    let mut out = [0u8; 33];
    out.copy_from_slice(&raw);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use a402_shared::adaptor_sig_secp::{
        compress_point, derive_normalized_keypair, encrypt_result, p_sign, random_witness,
        scalar_to_be_bytes,
    };
    use a402_shared::evm_chain::{asc_state_hash, Address, Bytes32};

    /// Round-trip the entire client-side verification path without a network:
    ///   - synthesize an SP keypair + `(t, T)`,
    ///   - sign a synthetic asc_state_hash + encrypt a synthetic plaintext,
    ///   - serialize to wire shapes (hex) and re-parse them,
    ///   - run verify_pre_sig → finalize_and_decrypt.
    #[test]
    fn parse_verify_decrypt_round_trip() {
        let seller_keys = derive_normalized_keypair(b"client-crate-test-seller-seed").unwrap();
        let (t_scalar, big_t) = random_witness();

        let asc_manager = Address([0xAB; 20]);
        let cid = Bytes32([0xCD; 32]);
        let new_balance_c: u128 = 99_000;
        let new_balance_s: u128 = 1_000;
        let new_version = 1u64;
        let state_hash = asc_state_hash(
            &asc_manager,
            &cid,
            new_balance_c,
            new_balance_s,
            new_version,
        );

        let pre = p_sign(&seller_keys, &state_hash.0, &big_t);
        let plaintext = b"{\"endpoint\":\"/weather\",\"temperature\":72}";
        let enc = encrypt_result(plaintext, &t_scalar);

        let wire = AtomicResp {
            cid: cid.to_hex(),
            asc_state_hash: state_hash.to_hex(),
            new_balance_c: new_balance_c.to_string(),
            new_balance_s: new_balance_s.to_string(),
            new_version,
            big_t: format!("0x{}", hex::encode(compress_point(&big_t))),
            sig_hat_r_prime: format!("0x{}", hex::encode(pre.r_prime)),
            sig_hat_s_prime: format!("0x{}", hex::encode(pre.s_prime)),
            enc_iv: format!("0x{}", hex::encode(enc.iv)),
            enc_ciphertext: format!("0x{}", hex::encode(&enc.ciphertext)),
            enc_tag: format!("0x{}", hex::encode(enc.tag)),
        };

        let parsed = parse_atomic_resp(&wire).expect("parse");
        assert_eq!(parsed.asc_state_hash, state_hash.0);
        assert_eq!(parsed.new_balance_c, new_balance_c);

        let seller_pub_recovered =
            reconstruct_seller_pubkey(&format!("0x{}", hex::encode(seller_keys.px_bytes))).unwrap();
        assert_eq!(seller_pub_recovered, seller_keys.public);

        verify_pre_sig(&seller_keys.public, &parsed).expect("σ̂ verifies");

        let (_full, recovered_plain) =
            finalize_and_decrypt(&seller_keys.public, &parsed, &t_scalar).expect("finalize");
        assert_eq!(recovered_plain, plaintext);

        // Witness mismatch path.
        let (rogue_t, _) = random_witness();
        assert!(finalize_and_decrypt(&seller_keys.public, &parsed, &rogue_t).is_err());

        // FinalizeResp parsing.
        let fake_sig_s = [0u8; 65];
        let finalize_wire = FinalizeResp {
            cid: cid.to_hex(),
            version: new_version,
            t: format!("0x{}", hex::encode(scalar_to_be_bytes(&t_scalar))),
            sig_s: format!("0x{}", hex::encode(fake_sig_s)),
        };
        let (parsed_t, parsed_sig_s) = parse_finalize_resp(&finalize_wire).unwrap();
        assert_eq!(parsed_t, t_scalar);
        assert_eq!(parsed_sig_s, fake_sig_s);
    }
}
