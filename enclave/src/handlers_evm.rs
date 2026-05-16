//! HTTP handlers for the EVM ASC channel lifecycle — VAULT (U) role only.
//!
//! After the role separation refactor, this module implements only the
//! Vault's HTTP surface:
//!
//!   POST /v1/channel/evm/open       — submit createASC (vault EOA tx)
//!   POST /v1/channel/evm/close      — submit closeASC (vault EOA tx)
//!   GET  /v1/channel/evm/:cid       — eth_call ascs(cid) decoded
//!   POST /v1/channel/evm/state-sig  — vault ECDSA over ascStateHash,
//!                                      consumed by the SP's force-close
//!                                      path (`ASCManager.forceClose` wants
//!                                      a vault sig as `σ_U`)
//!
//! The Service-Provider-role endpoints (`register / request / finalize /
//! force-close`) now live in the `service_provider/` binary. Per 
//! U and S are independent TEEs; mixing their handlers in one process
//! collapses their attack surfaces.
//!
//! ## Configuration (env)
//!
//!   A402_EVM_RPC_URL          (default http://127.0.0.1:8545)
//!   A402_EVM_ASC_MANAGER      required — deployed ASCManager.sol address
//!   A402_EVM_ASC_VAULT_EOA    required — the vault role address
//!   A402_EVM_VAULT_PRIV       optional — when set, build a signed-mode
//!                                         client (EIP-1559 raw txs, in-enclave
//!                                         k256 signing). State-sig REQUIRES
//!                                         signed mode because it needs the
//!                                         private key for `σ_U`.

use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha3::{Digest, Keccak256};

use a402_shared::evm_chain::{
    self, Address, AscManagerClient, AscState, Bytes32, EvmError, EvmRpcClient,
    TransactionReceipt,
};
use a402_shared::evm_channel_store::{store as channel_store, EvmChannelRecord};
use k256::ecdsa::{RecoveryId, Signature, SigningKey as EcdsaSigningKey, VerifyingKey};
use k256::elliptic_curve::sec1::ToEncodedPoint;

const DEFAULT_RPC_URL: &str = "http://127.0.0.1:8545";
pub const RECEIPT_MAX_POLLS: u32 = 60;

/// Loads the Vault EVM context from environment. Returns `None` if
/// `A402_EVM_ASC_MANAGER` is unset (HTTP 503), `Err` on invalid values.
pub async fn try_load_evm_context() -> Result<Option<AscManagerClient>, String> {
    let manager_hex = match std::env::var("A402_EVM_ASC_MANAGER") {
        Ok(v) if !v.is_empty() => v,
        _ => return Ok(None),
    };
    let vault_hex = std::env::var("A402_EVM_ASC_VAULT_EOA").map_err(|_| {
        "A402_EVM_ASC_VAULT_EOA must be set when A402_EVM_ASC_MANAGER is".to_string()
    })?;
    let rpc_url =
        std::env::var("A402_EVM_RPC_URL").unwrap_or_else(|_| DEFAULT_RPC_URL.to_string());

    let manager =
        Address::parse(&manager_hex).map_err(|e| format!("A402_EVM_ASC_MANAGER: {e}"))?;
    let vault =
        Address::parse(&vault_hex).map_err(|e| format!("A402_EVM_ASC_VAULT_EOA: {e}"))?;
    let rpc = EvmRpcClient::new(rpc_url);

    if let Ok(priv_hex) = std::env::var("A402_EVM_VAULT_PRIV") {
        if !priv_hex.is_empty() {
            let chain_id = rpc
                .chain_id()
                .await
                .map_err(|e| format!("eth_chainId probe failed: {e}"))?;
            let signer = a402_shared::evm_tx::EvmSigner::from_hex(&priv_hex, chain_id)
                .map_err(|e| format!("A402_EVM_VAULT_PRIV: {e}"))?;
            if signer.address() != vault {
                return Err(format!(
                    "A402_EVM_VAULT_PRIV derives {}, but A402_EVM_ASC_VAULT_EOA says {}",
                    signer.address().to_hex(),
                    vault.to_hex()
                ));
            }
            return Ok(Some(AscManagerClient::with_signer(rpc, manager, signer)));
        }
    }
    Ok(Some(AscManagerClient::new(rpc, manager, vault)))
}

/* -------------------------------------------------------------------------- */
/*                                  Errors                                     */
/* -------------------------------------------------------------------------- */

pub enum EvmHandlerError {
    NotConfigured,
    BadRequest(String),
    Chain(EvmError),
}

impl IntoResponse for EvmHandlerError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            EvmHandlerError::NotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "EVM context not configured — set A402_EVM_ASC_MANAGER + A402_EVM_ASC_VAULT_EOA"
                    .to_string(),
            ),
            EvmHandlerError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            EvmHandlerError::Chain(EvmError::Reverted(tx)) => (
                StatusCode::BAD_GATEWAY,
                format!("on-chain tx reverted: {tx}"),
            ),
            EvmHandlerError::Chain(e) => (StatusCode::BAD_GATEWAY, e.to_string()),
        };
        (status, Json(json!({"error": msg}))).into_response()
    }
}

/* -------------------------------------------------------------------------- */
/*                                  Wire types                                 */
/* -------------------------------------------------------------------------- */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenEvmChannelRequest {
    pub buyer: String,
    pub seller: String,
    pub deposit: String,
    pub cid: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenEvmChannelResponse {
    pub cid: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub asc_manager: String,
    pub vault_eoa: String,
    pub deposit: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseEvmChannelRequest {
    pub cid: String,
    pub balance_c: String,
    pub balance_s: String,
    pub version: u64,
    pub sig_c: String,
    pub sig_s: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseEvmChannelResponse {
    pub cid: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub balance_c: String,
    pub balance_s: String,
    pub version: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvmChannelStateResponse {
    pub cid: String,
    pub client: String,
    pub provider: String,
    pub balance_c: String,
    pub balance_s: String,
    pub version: u64,
    pub status: u8,
    pub status_name: &'static str,
    pub created_at: u64,
    pub total_deposit: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSigRequest {
    pub cid: String,
    pub balance_c: String,
    pub balance_s: String,
    pub version: u64,
    pub sig_c: String,
    pub sig_s: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSigResponse {
    pub asc_state_hash: String,
    pub sig_u: String,
}

/* -------------------------------------------------------------------------- */
/*                                Handlers                                     */
/* -------------------------------------------------------------------------- */

pub async fn post_channel_evm_open(
    Json(req): Json<OpenEvmChannelRequest>,
) -> Result<Json<OpenEvmChannelResponse>, EvmHandlerError> {
    let asc = try_load_evm_context()
        .await
        .map_err(EvmHandlerError::BadRequest)?
        .ok_or(EvmHandlerError::NotConfigured)?;

    let buyer = Address::parse(&req.buyer).map_err(|e| EvmHandlerError::BadRequest(e.to_string()))?;
    let seller =
        Address::parse(&req.seller).map_err(|e| EvmHandlerError::BadRequest(e.to_string()))?;
    let deposit: u128 = req.deposit.parse().map_err(|e: std::num::ParseIntError| {
        EvmHandlerError::BadRequest(format!("deposit: {e}"))
    })?;
    if deposit == 0 {
        return Err(EvmHandlerError::BadRequest("deposit must be > 0".to_string()));
    }

    let cid = match req.cid {
        Some(hex) => Bytes32::parse(&hex).map_err(|e| EvmHandlerError::BadRequest(e.to_string()))?,
        None => {
            let mut buf = [0u8; 32];
            rand::thread_rng().fill(&mut buf);
            Bytes32(buf)
        }
    };

    let tx_hash = asc
        .create_asc(&cid, &buyer, &seller, deposit)
        .await
        .map_err(EvmHandlerError::Chain)?;
    let receipt: TransactionReceipt = asc
        .rpc
        .wait_receipt(&tx_hash, RECEIPT_MAX_POLLS)
        .await
        .map_err(EvmHandlerError::Chain)?;

    // Mirror the channel into the local store so state-sig can verify
    // balances against the channel's total deposit later.
    channel_store().insert_channel(
        &cid,
        EvmChannelRecord::new(buyer, seller, deposit),
    );

    Ok(Json(OpenEvmChannelResponse {
        cid: cid.to_hex(),
        tx_hash,
        block_number: receipt.block_number_u64(),
        asc_manager: asc.address.to_hex(),
        vault_eoa: asc.vault_eoa.to_hex(),
        deposit: deposit.to_string(),
    }))
}

pub async fn post_channel_evm_close(
    Json(req): Json<CloseEvmChannelRequest>,
) -> Result<Json<CloseEvmChannelResponse>, EvmHandlerError> {
    let asc = try_load_evm_context()
        .await
        .map_err(EvmHandlerError::BadRequest)?
        .ok_or(EvmHandlerError::NotConfigured)?;

    let cid =
        Bytes32::parse(&req.cid).map_err(|e| EvmHandlerError::BadRequest(e.to_string()))?;
    let balance_c: u128 = req.balance_c.parse().map_err(|e: std::num::ParseIntError| {
        EvmHandlerError::BadRequest(format!("balanceC: {e}"))
    })?;
    let balance_s: u128 = req.balance_s.parse().map_err(|e: std::num::ParseIntError| {
        EvmHandlerError::BadRequest(format!("balanceS: {e}"))
    })?;
    let sig_c = decode_sig(&req.sig_c, "sigC")?;
    let sig_s = decode_sig(&req.sig_s, "sigS")?;

    let tx_hash = asc
        .close_asc(&cid, balance_c, balance_s, req.version, &sig_c, &sig_s)
        .await
        .map_err(EvmHandlerError::Chain)?;
    let receipt = asc
        .rpc
        .wait_receipt(&tx_hash, RECEIPT_MAX_POLLS)
        .await
        .map_err(EvmHandlerError::Chain)?;

    Ok(Json(CloseEvmChannelResponse {
        cid: cid.to_hex(),
        tx_hash,
        block_number: receipt.block_number_u64(),
        balance_c: balance_c.to_string(),
        balance_s: balance_s.to_string(),
        version: req.version,
    }))
}

pub async fn get_channel_evm_state(
    Path(cid_hex): Path<String>,
) -> Result<Json<EvmChannelStateResponse>, EvmHandlerError> {
    let asc = try_load_evm_context()
        .await
        .map_err(EvmHandlerError::BadRequest)?
        .ok_or(EvmHandlerError::NotConfigured)?;

    let cid =
        Bytes32::parse(&cid_hex).map_err(|e| EvmHandlerError::BadRequest(e.to_string()))?;
    let state: AscState = asc.read_state(&cid).await.map_err(EvmHandlerError::Chain)?;

    Ok(Json(EvmChannelStateResponse {
        cid: cid.to_hex(),
        client: state.client.to_hex(),
        provider: state.provider.to_hex(),
        balance_c: state.balance_c.to_string(),
        balance_s: state.balance_s.to_string(),
        version: state.version,
        status: state.status,
        status_name: status_name(state.status),
        created_at: state.created_at,
        total_deposit: state.total_deposit.to_string(),
    }))
}

/// Vault function: given a state already signed by buyer + seller, produce
/// the vault's ECDSA over the same `ascStateHash`. Consumed by the SP's
/// `forceClose` path (`ASCManager.forceClose` requires `sigU` from the
/// vault role). Requires signed-mode (`A402_EVM_VAULT_PRIV` set), since
/// we need the private key to mint `σ_U`.
pub async fn post_channel_evm_state_sig(
    Json(req): Json<StateSigRequest>,
) -> Result<Json<StateSigResponse>, EvmHandlerError> {
    let asc = try_load_evm_context()
        .await
        .map_err(EvmHandlerError::BadRequest)?
        .ok_or(EvmHandlerError::NotConfigured)?;
    let signer = asc
        .signer
        .as_ref()
        .ok_or_else(|| {
            EvmHandlerError::BadRequest(
                "vault state-sig requires signed mode (set A402_EVM_VAULT_PRIV)".to_string(),
            )
        })?
        .clone();

    let cid =
        Bytes32::parse(&req.cid).map_err(|e| EvmHandlerError::BadRequest(e.to_string()))?;
    let balance_c: u128 = req.balance_c.parse().map_err(|e: std::num::ParseIntError| {
        EvmHandlerError::BadRequest(format!("balanceC: {e}"))
    })?;
    let balance_s: u128 = req.balance_s.parse().map_err(|e: std::num::ParseIntError| {
        EvmHandlerError::BadRequest(format!("balanceS: {e}"))
    })?;

    let entry = channel_store()
        .get_channel(&cid)
        .ok_or_else(|| EvmHandlerError::BadRequest("unknown cid".to_string()))?;
    if balance_c + balance_s != entry.total_deposit {
        return Err(EvmHandlerError::BadRequest(
            "balance not conserved".to_string(),
        ));
    }

    let sig_c = decode_sig(&req.sig_c, "sigC")?;
    let sig_s = decode_sig(&req.sig_s, "sigS")?;
    let state_hash =
        evm_chain::asc_state_hash(&asc.address, &cid, balance_c, balance_s, req.version);
    let recovered_c = ecdsa_recover_eth_message(&state_hash, &sig_c)?;
    let recovered_s = ecdsa_recover_eth_message(&state_hash, &sig_s)?;
    if recovered_c != entry.buyer {
        return Err(EvmHandlerError::BadRequest(format!(
            "sigC recovers to {}, expected buyer {}",
            recovered_c.to_hex(),
            entry.buyer.to_hex()
        )));
    }
    if recovered_s != entry.seller {
        return Err(EvmHandlerError::BadRequest(format!(
            "sigS recovers to {}, expected seller {}",
            recovered_s.to_hex(),
            entry.seller.to_hex()
        )));
    }

    let vault_sk: EcdsaSigningKey = signer.into_signing_key();
    let sig_u = ecdsa_sign_eth_message(&vault_sk, &state_hash);

    Ok(Json(StateSigResponse {
        asc_state_hash: state_hash.to_hex(),
        sig_u: format!("0x{}", hex::encode(&sig_u)),
    }))
}

/* -------------------------------------------------------------------------- */
/*                                Helpers                                      */
/* -------------------------------------------------------------------------- */

fn decode_sig(hex_value: &str, label: &str) -> Result<Vec<u8>, EvmHandlerError> {
    let h = hex_value.strip_prefix("0x").unwrap_or(hex_value);
    let bytes = hex::decode(h).map_err(|e| EvmHandlerError::BadRequest(format!("{label}: {e}")))?;
    if bytes.len() != 65 {
        return Err(EvmHandlerError::BadRequest(format!(
            "{label} must be 65 bytes (r || s || v), got {}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

fn ecdsa_recover_eth_message(digest32: &Bytes32, sig65: &[u8]) -> Result<Address, EvmHandlerError> {
    if sig65.len() != 65 {
        return Err(EvmHandlerError::BadRequest("sig must be 65 bytes".to_string()));
    }
    let r = &sig65[0..32];
    let s = &sig65[32..64];
    let v = sig65[64];
    let recovery = match v {
        27 | 0 => 0u8,
        28 | 1 => 1u8,
        _ => {
            return Err(EvmHandlerError::BadRequest(format!(
                "invalid recovery v={v}"
            )))
        }
    };
    let mut rs = [0u8; 64];
    rs[..32].copy_from_slice(r);
    rs[32..].copy_from_slice(s);
    let sig = Signature::from_slice(&rs)
        .map_err(|e| EvmHandlerError::BadRequest(format!("signature: {e}")))?;
    let rid = RecoveryId::try_from(recovery)
        .map_err(|e| EvmHandlerError::BadRequest(format!("recovery: {e}")))?;

    let mut prefixed = Vec::with_capacity(28 + 32);
    prefixed.extend_from_slice(b"\x19Ethereum Signed Message:\n32");
    prefixed.extend_from_slice(&digest32.0);
    let mut hasher = Keccak256::new();
    hasher.update(&prefixed);
    let eth_digest = hasher.finalize();
    let mut digest_arr = [0u8; 32];
    digest_arr.copy_from_slice(&eth_digest);

    let recovered = VerifyingKey::recover_from_prehash(&digest_arr, &sig, rid)
        .map_err(|e| EvmHandlerError::BadRequest(format!("recover: {e}")))?;
    let pt = recovered.to_encoded_point(false);
    let xy = &pt.as_bytes()[1..];
    let mut h = Keccak256::new();
    h.update(xy);
    let d = h.finalize();
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&d[12..32]);
    Ok(Address(addr))
}

fn ecdsa_sign_eth_message(sk: &EcdsaSigningKey, digest32: &Bytes32) -> Vec<u8> {
    let mut prefixed = Vec::with_capacity(28 + 32);
    prefixed.extend_from_slice(b"\x19Ethereum Signed Message:\n32");
    prefixed.extend_from_slice(&digest32.0);
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

fn status_name(status: u8) -> &'static str {
    match status {
        0 => "OPEN",
        1 => "CLOSING",
        2 => "CLOSED",
        _ => "UNKNOWN",
    }
}
