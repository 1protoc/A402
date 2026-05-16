//! Service Provider HTTP endpoints ( atomic exchange).
//!
//! These are the routes a Vault calls after mutual TEE registration. The
//! SP holds its OWN `sk_S` here and signs `σ̂_S` / `σ_S_ecdsa` itself —
//! key material never leaves this process. The Vault learns only
//! `(pk_S, h_code,S)` from `register-vault` and the per-request commitments
//! the SP returns.

use std::sync::Arc;

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;

use a402_shared::adaptor_sig_secp::{self, AdaptorPreSignature};
use a402_shared::evm_chain::{self, Address, AscManagerClient, Bytes32, EvmRpcClient};
use a402_shared::evm_channel_store::{store as channel_store, EvmChannelRecord, PendingRequest};
use a402_shared::evm_tx::{Eip1559TxParams, EvmSigner};
use sha3::{Digest, Keccak256};

use crate::registry::VaultRecord;
use crate::service;
use crate::AppState;

const PRICE_USDC_ATOMIC: u128 = 1_000;
const RECEIPT_MAX_POLLS: u32 = 60;

/* -------------------------------------------------------------------------- */
/*                                  Errors                                     */
/* -------------------------------------------------------------------------- */

pub enum SpError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Chain(String),
    Internal(String),
}

impl IntoResponse for SpError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            SpError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            SpError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            SpError::Conflict(m) => (StatusCode::CONFLICT, m),
            SpError::Chain(m) => (StatusCode::BAD_GATEWAY, m),
            SpError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(json!({"error": msg}))).into_response()
    }
}

/* -------------------------------------------------------------------------- */
/*                               GET /v1/sp/info                                */
/* -------------------------------------------------------------------------- */

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoResponse {
    pub address: String,
    pub schnorr_px: String,
    pub asc_manager: String,
    pub vault_addr: String,
    pub registered_vaults: usize,
}

pub async fn get_info(State(state): State<Arc<AppState>>) -> Json<InfoResponse> {
    Json(InfoResponse {
        address: state.keys.address_hex(),
        schnorr_px: state.keys.schnorr_px_hex(),
        asc_manager: state.asc_manager.clone(),
        vault_addr: state.vault_addr.clone(),
        registered_vaults: state.vault_registry.len(),
    })
}

/* -------------------------------------------------------------------------- */
/*                       POST /v1/sp/register-vault                            */
/* -------------------------------------------------------------------------- */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterVaultRequest {
    pub uid: String,
    pub vault_eoa: String,
    /// Optional code hash from the Vault's attestation document.
    pub code_hash: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterVaultResponse {
    pub ok: bool,
    pub uid: String,
    pub vault_eoa: String,
}

pub async fn post_register_vault(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterVaultRequest>,
) -> Result<Json<RegisterVaultResponse>, SpError> {
    let vault_eoa = Address::parse(&req.vault_eoa)
        .map_err(|e| SpError::BadRequest(format!("vaultEoa: {e}")))?;
    let pinned = Address::parse(&state.vault_addr)
        .map_err(|e| SpError::Internal(format!("A402_EVM_VAULT_ADDR: {e}")))?;
    if vault_eoa != pinned {
        return Err(SpError::BadRequest(format!(
            "vaultEoa {} does not match the pinned A402_EVM_VAULT_ADDR {}",
            vault_eoa.to_hex(),
            pinned.to_hex()
        )));
    }
    let code_hash = req
        .code_hash
        .as_deref()
        .map(|h| -> Result<[u8; 32], SpError> {
            let bytes = hex::decode(h.strip_prefix("0x").unwrap_or(h))
                .map_err(|e| SpError::BadRequest(format!("codeHash: {e}")))?;
            if bytes.len() != 32 {
                return Err(SpError::BadRequest("codeHash must be 32 bytes".to_string()));
            }
            let mut out = [0u8; 32];
            out.copy_from_slice(&bytes);
            Ok(out)
        })
        .transpose()?;

    let record = VaultRecord {
        uid: req.uid.clone(),
        vault_eoa,
        code_hash,
    };
    state
        .vault_registry
        .register(record)
        .map_err(SpError::Conflict)?;
    Ok(Json(RegisterVaultResponse {
        ok: true,
        uid: req.uid,
        vault_eoa: vault_eoa.to_hex(),
    }))
}

/* -------------------------------------------------------------------------- */
/*                           POST /v1/sp/register                              */
/* -------------------------------------------------------------------------- */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    pub cid: String,
    pub buyer: String,
    pub total_deposit: String,
    pub vault_uid: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResponse {
    pub ok: bool,
    pub cid: String,
    pub seller_address: String,
    pub seller_schnorr_px: String,
}

pub async fn post_register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, SpError> {
    state
        .vault_registry
        .get(&req.vault_uid)
        .ok_or_else(|| SpError::BadRequest(format!("vaultUid '{}' not in Reg_U", req.vault_uid)))?;

    let cid = Bytes32::parse(&req.cid).map_err(|e| SpError::BadRequest(e.to_string()))?;
    let buyer = Address::parse(&req.buyer).map_err(|e| SpError::BadRequest(e.to_string()))?;
    let total_deposit: u128 = req
        .total_deposit
        .parse()
        .map_err(|e: std::num::ParseIntError| SpError::BadRequest(format!("totalDeposit: {e}")))?;

    let rpc = EvmRpcClient::new(state.rpc_url.clone());
    let asc_manager = Address::parse(&state.asc_manager)
        .map_err(|e| SpError::Internal(format!("asc_manager: {e}")))?;
    let vault_addr = Address::parse(&state.vault_addr)
        .map_err(|e| SpError::Internal(format!("vault_addr: {e}")))?;
    let client = AscManagerClient::new(rpc, asc_manager, vault_addr);
    let onchain = client
        .read_state(&cid)
        .await
        .map_err(|e| SpError::Chain(e.to_string()))?;

    if onchain.client != buyer
        || onchain.provider != state.keys.address
        || onchain.total_deposit != total_deposit
        || onchain.status != 0
    {
        return Err(SpError::BadRequest(format!(
            "on-chain state mismatch (client={}, provider={}, deposit={}, status={})",
            onchain.client.to_hex(),
            onchain.provider.to_hex(),
            onchain.total_deposit,
            onchain.status
        )));
    }

    channel_store().insert_channel(
        &cid,
        EvmChannelRecord::new(buyer, state.keys.address, total_deposit),
    );

    Ok(Json(RegisterResponse {
        ok: true,
        cid: cid.to_hex(),
        seller_address: state.keys.address_hex(),
        seller_schnorr_px: state.keys.schnorr_px_hex(),
    }))
}

/* -------------------------------------------------------------------------- */
/*                           POST /v1/sp/request                               */
/* -------------------------------------------------------------------------- */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtomicReq {
    pub cid: String,
    pub req: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtomicResp {
    pub cid: String,
    pub asc_state_hash: String,
    pub new_balance_c: String,
    pub new_balance_s: String,
    pub new_version: u64,
    pub big_t: String,
    pub sig_hat_r_prime: String,
    pub sig_hat_s_prime: String,
    pub enc_iv: String,
    pub enc_ciphertext: String,
    pub enc_tag: String,
}

pub async fn post_request(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AtomicReq>,
) -> Result<Json<AtomicResp>, SpError> {
    let cid = Bytes32::parse(&req.cid).map_err(|e| SpError::BadRequest(e.to_string()))?;
    let record = channel_store()
        .get_channel(&cid)
        .ok_or_else(|| SpError::NotFound("channel not registered with this SP".to_string()))?;
    if record.force_closed {
        return Err(SpError::Conflict("channel force-closed".to_string()));
    }
    if record.balance_c < PRICE_USDC_ATOMIC {
        return Err(SpError::BadRequest("channel exhausted".to_string()));
    }

    let asc_manager = Address::parse(&state.asc_manager)
        .map_err(|e| SpError::Internal(format!("asc_manager: {e}")))?;

    // Execute the service. (Currently a fixed weather payload; future
    // commits will dispatch on req.req.endpoint to a real upstream API.)
    let plaintext = service::execute_weather(record.served_requests as u64 + 1);

    let (t_scalar, big_t) = adaptor_sig_secp::random_witness();
    let enc = adaptor_sig_secp::encrypt_result(&plaintext, &t_scalar);

    let new_balance_c = record.balance_c - PRICE_USDC_ATOMIC;
    let new_balance_s = record.balance_s + PRICE_USDC_ATOMIC;
    let new_version = record.version + 1;
    let state_hash = evm_chain::asc_state_hash(
        &asc_manager,
        &cid,
        new_balance_c,
        new_balance_s,
        new_version,
    );

    let sig_hat =
        adaptor_sig_secp::p_sign(&state.keys.schnorr, &state_hash.0, &big_t);
    let big_t_compressed = adaptor_sig_secp::compress_point(&big_t);

    channel_store().park_pending(PendingRequest {
        cid,
        new_version,
        new_balance_c,
        new_balance_s,
        t: t_scalar,
        sig_hat: sig_hat.clone(),
        enc_res: enc.clone(),
        asc_state_hash: state_hash,
    });

    Ok(Json(AtomicResp {
        cid: cid.to_hex(),
        asc_state_hash: state_hash.to_hex(),
        new_balance_c: new_balance_c.to_string(),
        new_balance_s: new_balance_s.to_string(),
        new_version,
        big_t: format!("0x{}", hex::encode(big_t_compressed)),
        sig_hat_r_prime: format!("0x{}", hex::encode(sig_hat.r_prime)),
        sig_hat_s_prime: format!("0x{}", hex::encode(sig_hat.s_prime)),
        enc_iv: format!("0x{}", hex::encode(enc.iv)),
        enc_ciphertext: format!("0x{}", hex::encode(&enc.ciphertext)),
        enc_tag: format!("0x{}", hex::encode(enc.tag)),
    }))
}

/* -------------------------------------------------------------------------- */
/*                          POST /v1/sp/finalize                               */
/* -------------------------------------------------------------------------- */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeReq {
    pub cid: String,
    pub version: u64,
    pub sig_c: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeResp {
    pub cid: String,
    pub version: u64,
    pub t: String,
    pub sig_s: String,
}

pub async fn post_finalize(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FinalizeReq>,
) -> Result<Json<FinalizeResp>, SpError> {
    let cid = Bytes32::parse(&req.cid).map_err(|e| SpError::BadRequest(e.to_string()))?;
    let pending = channel_store()
        .take_pending(&cid, req.version)
        .ok_or_else(|| SpError::NotFound("no pending request".to_string()))?;
    let record = channel_store()
        .get_channel(&cid)
        .ok_or_else(|| SpError::NotFound("channel disappeared".to_string()))?;

    let sig_c_bytes = parse_sig(&req.sig_c, "sigC")?;
    let recovered = recover_eth_signed(&pending.asc_state_hash.0, &sig_c_bytes)
        .map_err(SpError::BadRequest)?;
    if recovered != record.buyer {
        return Err(SpError::BadRequest(format!(
            "sigC recovered {} but channel buyer is {}",
            recovered.to_hex(),
            record.buyer.to_hex()
        )));
    }

    let sig_s = sign_eth_signed(&state.keys.ecdsa, &pending.asc_state_hash.0);

    channel_store().mutate_channel(&cid, |r| {
        r.balance_c = pending.new_balance_c;
        r.balance_s = pending.new_balance_s;
        r.version = pending.new_version;
        r.last_sig_c = Some(sig_c_bytes.clone());
        r.last_sig_s = Some(sig_s.clone());
        r.last_sig_hat = Some(pending.sig_hat.clone());
        r.last_t = Some(pending.t);
        r.last_state_hash = Some(pending.asc_state_hash);
        r.served_requests += 1;
    });

    let t_hex = format!(
        "0x{}",
        hex::encode(adaptor_sig_secp::scalar_to_be_bytes(&pending.t))
    );

    Ok(Json(FinalizeResp {
        cid: cid.to_hex(),
        version: pending.new_version,
        t: t_hex,
        sig_s: format!("0x{}", hex::encode(&sig_s)),
    }))
}

/* -------------------------------------------------------------------------- */
/*                        POST /v1/sp/force-close                              */
/* -------------------------------------------------------------------------- */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForceCloseReq {
    pub cid: String,
    /// The Vault's σ_U over `ascStateHash`, fetched out-of-band by the SP
    /// from the Vault's `/v1/channel/evm/state-sig` endpoint.
    pub vault_sig_u: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForceCloseResp {
    pub cid: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub schnorr_px: String,
    pub schnorr_e: String,
    pub schnorr_s: String,
}

pub async fn post_force_close(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ForceCloseReq>,
) -> Result<Json<ForceCloseResp>, SpError> {
    let cid = Bytes32::parse(&req.cid).map_err(|e| SpError::BadRequest(e.to_string()))?;
    let entry = channel_store()
        .get_channel(&cid)
        .ok_or_else(|| SpError::NotFound("unknown cid".to_string()))?;
    if entry.force_closed {
        return Err(SpError::Conflict("already force-closed".to_string()));
    }
    let sig_hat = entry
        .last_sig_hat
        .clone()
        .ok_or_else(|| SpError::BadRequest("no Schnorr pre-sig to adapt".to_string()))?;
    let t = entry
        .last_t
        .ok_or_else(|| SpError::BadRequest("no t to adapt with".to_string()))?;
    let state_hash = entry
        .last_state_hash
        .ok_or_else(|| SpError::BadRequest("no last state hash".to_string()))?;
    let sig_s_ecdsa = entry
        .last_sig_s
        .clone()
        .ok_or_else(|| SpError::BadRequest("no last σ_S ECDSA".to_string()))?;

    let vault_sig_u = parse_sig(&req.vault_sig_u, "vaultSigU")?;

    // Adapt + pack on-chain proof. (px/e/s consumed by SchnorrVerifier.sol.)
    let full = adaptor_sig_secp::adapt(&sig_hat, &t)
        .map_err(|e| SpError::Internal(format!("adapt: {e}")))?;
    let proof = adaptor_sig_secp::build_onchain_proof(&state.keys.schnorr.public, &state_hash.0, &full)
        .map_err(|e| SpError::Internal(format!("build_onchain_proof: {e}")))?;

    let asc_manager_addr = Address::parse(&state.asc_manager)
        .map_err(|e| SpError::Internal(format!("asc_manager: {e}")))?;
    let calldata = evm_chain::encode_force_close(
        &cid,
        entry.balance_c,
        entry.balance_s,
        entry.version,
        &vault_sig_u,
        &sig_s_ecdsa,
        proof.px,
        proof.e,
        proof.s,
    );

    // SP signs and submits the tx from its OWN ECDSA key (the on-chain
    // `forceClose` requires `msg.sender == provider`, which is exactly the
    // SP's address).
    let rpc = EvmRpcClient::new(state.rpc_url.clone());
    let chain_id = rpc
        .chain_id()
        .await
        .map_err(|e| SpError::Chain(e.to_string()))?;
    let signer_secret = state.keys.ecdsa.to_bytes();
    let mut secret = [0u8; 32];
    secret.copy_from_slice(signer_secret.as_slice());
    let signer = EvmSigner::from_bytes(&secret, chain_id)
        .map_err(|e| SpError::Internal(format!("EvmSigner: {e}")))?;

    let nonce = rpc
        .pending_nonce(&state.keys.address)
        .await
        .map_err(|e| SpError::Chain(e.to_string()))?;
    let base = rpc.gas_price().await.map_err(|e| SpError::Chain(e.to_string()))?;
    let priority = rpc.max_priority_fee_per_gas().await.unwrap_or(base);
    let max_fee = base.saturating_mul(2).max(priority.saturating_mul(2));

    let data = hex::decode(calldata.trim_start_matches("0x"))
        .map_err(|e| SpError::Internal(format!("calldata decode: {e}")))?;
    let params = Eip1559TxParams {
        chain_id,
        nonce,
        max_priority_fee_per_gas: priority,
        max_fee_per_gas: max_fee,
        gas_limit: 600_000,
        to: asc_manager_addr,
        value: 0,
        data: &data,
    };
    let raw = a402_shared::evm_tx::sign_eip1559(&signer, &params)
        .map_err(|e| SpError::Chain(e.to_string()))?;
    let tx_hash = rpc
        .send_raw_transaction(&raw)
        .await
        .map_err(|e| SpError::Chain(e.to_string()))?;
    let receipt = rpc
        .wait_receipt(&tx_hash, RECEIPT_MAX_POLLS)
        .await
        .map_err(|e| SpError::Chain(e.to_string()))?;

    channel_store().mutate_channel(&cid, |r| r.force_closed = true);

    Ok(Json(ForceCloseResp {
        cid: cid.to_hex(),
        tx_hash,
        block_number: receipt.block_number_u64(),
        schnorr_px: format!("0x{}", hex::encode(proof.px)),
        schnorr_e: format!("0x{}", hex::encode(proof.e)),
        schnorr_s: format!("0x{}", hex::encode(proof.s)),
    }))
}

/* -------------------------------------------------------------------------- */
/*                                Helpers                                      */
/* -------------------------------------------------------------------------- */

fn parse_sig(hex_value: &str, label: &str) -> Result<Vec<u8>, SpError> {
    let h = hex_value.strip_prefix("0x").unwrap_or(hex_value);
    let bytes = hex::decode(h).map_err(|e| SpError::BadRequest(format!("{label}: {e}")))?;
    if bytes.len() != 65 {
        return Err(SpError::BadRequest(format!(
            "{label} must be 65 bytes (r||s||v), got {}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

fn recover_eth_signed(digest32: &[u8; 32], sig65: &[u8]) -> Result<Address, String> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    if sig65.len() != 65 {
        return Err("signature must be 65 bytes".to_string());
    }
    let r = &sig65[0..32];
    let s = &sig65[32..64];
    let v = sig65[64];
    let recovery = match v {
        27 | 0 => 0u8,
        28 | 1 => 1u8,
        _ => return Err(format!("invalid recovery v={v}")),
    };
    let mut rs = [0u8; 64];
    rs[..32].copy_from_slice(r);
    rs[32..].copy_from_slice(s);
    let sig = Signature::from_slice(&rs).map_err(|e| format!("signature: {e}"))?;
    let rid = RecoveryId::try_from(recovery).map_err(|e| format!("recovery: {e}"))?;

    let mut prefixed = Vec::with_capacity(28 + 32);
    prefixed.extend_from_slice(b"\x19Ethereum Signed Message:\n32");
    prefixed.extend_from_slice(digest32);
    let mut hasher = Keccak256::new();
    hasher.update(&prefixed);
    let eth_digest = hasher.finalize();
    let mut digest_arr = [0u8; 32];
    digest_arr.copy_from_slice(&eth_digest);

    let recovered =
        VerifyingKey::recover_from_prehash(&digest_arr, &sig, rid).map_err(|e| format!("recover: {e}"))?;
    use k256::elliptic_curve::sec1::ToEncodedPoint;
    let pt = recovered.to_encoded_point(false);
    let xy = &pt.as_bytes()[1..];
    let mut h = Keccak256::new();
    h.update(xy);
    let d = h.finalize();
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&d[12..32]);
    Ok(Address(addr))
}

fn sign_eth_signed(sk: &k256::ecdsa::SigningKey, digest32: &[u8; 32]) -> Vec<u8> {
    let mut prefixed = Vec::with_capacity(28 + 32);
    prefixed.extend_from_slice(b"\x19Ethereum Signed Message:\n32");
    prefixed.extend_from_slice(digest32);
    let mut hasher = Keccak256::new();
    hasher.update(&prefixed);
    let eth_digest = hasher.finalize();
    let (sig, rid) = sk
        .sign_prehash_recoverable(eth_digest.as_slice())
        .expect("sign");
    let mut out = Vec::with_capacity(65);
    out.extend_from_slice(&sig.r().to_bytes());
    out.extend_from_slice(&sig.s().to_bytes());
    out.push(27 + u8::from(rid));
    out
}
