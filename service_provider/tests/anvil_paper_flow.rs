//! End-to-end paper flow test:
//!
//!   Client (`a402-client` lib)  →  Vault (in-test stub)  →  SP (`a402-service-provider` binary subprocess)
//!
//! Exercises the multi-round atomic exchange as described in the paper:
//!
//!   1. Client → Vault POST /v1/client/channel/open
//!      Vault talks to SP (/register-vault + /register), submits createASC on-chain.
//!   2. Client → Vault POST /v1/client/channel/request   (×3)
//!      Vault proxies to SP /request, locally verifies σ̂_S,
//!      returns HTTP 402 + JSON envelope { ascStateHash, newBalances, ... }.
//!   3. Client → Vault POST /v1/client/channel/pay       (×3, after signing σ_C)
//!      Vault proxies to SP /finalize, locally verifies σ_S and decrypts EncRes,
//!      returns plaintext to Client.
//!   4. Client → Vault POST /v1/client/channel/close
//!      Vault submits cooperative closeASC with the cached σ_C / σ_S.
//!
//! The Vault here is an inline test stub (~200 lines below) — the smallest
//! amount of code that proves the multi-process protocol works. Promoting
//! these routes into the production `enclave/` binary is a follow-up that
//! does not require any of the cryptographic plumbing to change.
//!
//! Prerequisites (test is `#[ignore]` otherwise):
//!   - Anvil running at `A402_EVM_RPC_URL`
//!   - `yarn evm:bootstrap` already produced `.env.evm.generated`; export it
//!     so `A402_EVM_ASC_MANAGER` + `A402_EVM_ASC_VAULT_EOA` are visible
//!   - Buyer / vault / seller deterministic Anvil keys (defaults built in)
//!
//! Run:
//!   cargo test -p a402-service-provider --test anvil_paper_flow -- \
//!       --ignored --nocapture

use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{Json as AxumJson, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpListener;

use a402_client::sigs::sign_eth_signed;
use a402_client::sp_http::AtomicResp as SpAtomicResp;
use a402_client::sp_http::FinalizeResp as SpFinalizeResp;
use a402_client::ClientKeys;
use a402_shared::adaptor_sig_secp::{decrypt_result, EncryptedResult};
use a402_shared::evm_chain::{Address, AscManagerClient, Bytes32, EvmRpcClient};
use a402_shared::evm_tx::EvmSigner;

const ANVIL_BUYER_PRIV: &str =
    "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a";
const ANVIL_VAULT_PRIV: &str =
    "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
const ANVIL_SP_PRIV: &str =
    "0x7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6";

const VAULT_UID: &str = "vault-paper-flow-1";

/* -------------------------------------------------------------------------- */
/*                          Vault stub (in-test only)                          */
/* -------------------------------------------------------------------------- */

#[derive(Clone)]
struct VaultState {
    rpc_url: String,
    asc_manager: Address,
    vault_eoa: Address,
    signer: EvmSigner,
    channels: Arc<DashMap<Bytes32, ChannelEntry>>,
    http: reqwest::Client,
}

#[derive(Clone)]
struct ChannelEntry {
    sp_url: String,
    #[allow(dead_code)]
    sp_address: Address,
    sp_schnorr_px: String,
    buyer: Address,
    balance_c: u128,
    balance_s: u128,
    version: u64,
    pending: Option<PendingForPay>,
    last_sig_c: Option<[u8; 65]>,
    last_sig_s: Option<[u8; 65]>,
}

#[derive(Clone)]
struct PendingForPay {
    version: u64,
    parsed: Arc<a402_client::atomic::ParsedAtomicResp>,
    seller_pub: k256::ProjectivePoint,
}

enum VaultError {
    NotFound(String),
    BadRequest(String),
    Chain(String),
    Upstream(String),
    PaymentRequired(Value),
}

impl IntoResponse for VaultError {
    fn into_response(self) -> Response {
        match self {
            VaultError::NotFound(m) => {
                (StatusCode::NOT_FOUND, AxumJson(json!({ "error": m }))).into_response()
            }
            VaultError::BadRequest(m) => {
                (StatusCode::BAD_REQUEST, AxumJson(json!({ "error": m }))).into_response()
            }
            VaultError::Chain(m) => {
                (StatusCode::BAD_GATEWAY, AxumJson(json!({ "error": m }))).into_response()
            }
            VaultError::Upstream(m) => {
                (StatusCode::BAD_GATEWAY, AxumJson(json!({ "error": m }))).into_response()
            }
            VaultError::PaymentRequired(env) => {
                (StatusCode::PAYMENT_REQUIRED, AxumJson(env)).into_response()
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientOpenReq {
    buyer: String,
    deposit: String,
    sp_url: String,
    sp_address: String,
    sp_schnorr_px: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientOpenResp {
    cid: String,
    tx_hash: String,
    block_number: u64,
    sp_address: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientCidReq {
    cid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientPayReq {
    cid: String,
    version: u64,
    sig_c: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientPayResp {
    cid: String,
    version: u64,
    plaintext_b64: String,
    sig_s: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientCloseResp {
    cid: String,
    tx_hash: String,
    block_number: u64,
    balance_c: String,
    balance_s: String,
    version: u64,
}

async fn vault_open(
    State(state): State<VaultState>,
    AxumJson(req): AxumJson<ClientOpenReq>,
) -> Result<AxumJson<ClientOpenResp>, VaultError> {
    let buyer = Address::parse(&req.buyer).map_err(|e| VaultError::BadRequest(e.to_string()))?;
    let sp_address =
        Address::parse(&req.sp_address).map_err(|e| VaultError::BadRequest(e.to_string()))?;
    let deposit: u128 = req
        .deposit
        .parse()
        .map_err(|e: std::num::ParseIntError| VaultError::BadRequest(format!("deposit: {e}")))?;

    // (1) Register self into the SP's Reg_U. Idempotent: a second call with
    //     the same uid+eoa returns 200.
    let sp_register_vault_url = format!("{}/v1/sp/register-vault", state.sp_url(&req.sp_url));
    let _ = state
        .http
        .post(&sp_register_vault_url)
        .json(&json!({ "uid": VAULT_UID, "vaultEoa": state.vault_eoa.to_hex() }))
        .send()
        .await
        .map_err(|e| VaultError::Upstream(format!("sp register-vault: {e}")))?;

    // (2) Sample a fresh cid + submit createASC on-chain.
    let mut cid_bytes = [0u8; 32];
    rand::Rng::fill(&mut rand::thread_rng(), &mut cid_bytes);
    let cid = Bytes32(cid_bytes);
    let rpc = EvmRpcClient::new(state.rpc_url.clone());
    let asc =
        AscManagerClient::with_signer(rpc.clone(), state.asc_manager, state.signer.clone());
    let tx_hash = asc
        .create_asc(&cid, &buyer, &sp_address, deposit)
        .await
        .map_err(|e| VaultError::Chain(format!("createASC: {e}")))?;
    let receipt = rpc
        .wait_receipt(&tx_hash, 60)
        .await
        .map_err(|e| VaultError::Chain(format!("createASC receipt: {e}")))?;

    // (3) Tell the SP that this cid exists. SP will eth_call ascs(cid) and
    //     cross-check the on-chain state before accepting.
    let sp_register_url = format!("{}/v1/sp/register", state.sp_url(&req.sp_url));
    let resp = state
        .http
        .post(&sp_register_url)
        .json(&json!({
            "cid": cid.to_hex(),
            "buyer": buyer.to_hex(),
            "totalDeposit": deposit.to_string(),
            "vaultUid": VAULT_UID,
        }))
        .send()
        .await
        .map_err(|e| VaultError::Upstream(format!("sp register: {e}")))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(VaultError::Upstream(format!("sp register: {body}")));
    }

    // (4) Bookkeep.
    state.channels.insert(
        cid,
        ChannelEntry {
            sp_url: state.sp_url(&req.sp_url),
            sp_address,
            sp_schnorr_px: req.sp_schnorr_px.clone(),
            buyer,
            balance_c: deposit,
            balance_s: 0,
            version: 0,
            pending: None,
            last_sig_c: None,
            last_sig_s: None,
        },
    );

    Ok(AxumJson(ClientOpenResp {
        cid: cid.to_hex(),
        tx_hash,
        block_number: receipt.block_number_u64(),
        sp_address: sp_address.to_hex(),
    }))
}

async fn vault_request(
    State(state): State<VaultState>,
    AxumJson(req): AxumJson<ClientCidReq>,
) -> Result<Response, VaultError> {
    let cid = Bytes32::parse(&req.cid).map_err(|e| VaultError::BadRequest(e.to_string()))?;
    let entry = state
        .channels
        .get(&cid)
        .ok_or_else(|| VaultError::NotFound("cid".into()))?
        .clone();

    let sp_resp = state
        .http
        .post(format!("{}/v1/sp/request", entry.sp_url))
        .json(&json!({ "cid": cid.to_hex(), "req": Value::Null }))
        .send()
        .await
        .map_err(|e| VaultError::Upstream(format!("sp request: {e}")))?;
    if !sp_resp.status().is_success() {
        let body = sp_resp.text().await.unwrap_or_default();
        return Err(VaultError::Upstream(format!("sp request: {body}")));
    }
    let atomic: SpAtomicResp = sp_resp
        .json()
        .await
        .map_err(|e| VaultError::Upstream(format!("sp request decode: {e}")))?;

    // Local verification of σ̂_S using the SP's pinned Schnorr Px.
    let parsed = a402_client::atomic::parse_atomic_resp(&atomic)
        .map_err(|e| VaultError::Upstream(format!("parse atomic: {e}")))?;
    let seller_pub = a402_client::atomic::reconstruct_seller_pubkey(&entry.sp_schnorr_px)
        .map_err(|e| VaultError::Upstream(format!("seller px: {e}")))?;
    a402_client::atomic::verify_pre_sig(&seller_pub, &parsed)
        .map_err(|_| VaultError::Upstream("σ̂_S failed p_verify".into()))?;

    let envelope = json!({
        "cid": atomic.cid,
        "ascStateHash": atomic.asc_state_hash,
        "newBalanceC": atomic.new_balance_c,
        "newBalanceS": atomic.new_balance_s,
        "newVersion": atomic.new_version,
        "scheme": "a402-evm-asc-v1",
        "price": format!("{}", parsed.new_balance_s.saturating_sub(entry.balance_s)),
    });

    state.channels.alter(&cid, |_, mut e| {
        e.pending = Some(PendingForPay {
            version: parsed.new_version,
            parsed: Arc::new(parsed),
            seller_pub,
        });
        e
    });

    Err(VaultError::PaymentRequired(envelope))
}

async fn vault_pay(
    State(state): State<VaultState>,
    AxumJson(req): AxumJson<ClientPayReq>,
) -> Result<AxumJson<ClientPayResp>, VaultError> {
    let cid = Bytes32::parse(&req.cid).map_err(|e| VaultError::BadRequest(e.to_string()))?;
    let entry = state
        .channels
        .get(&cid)
        .ok_or_else(|| VaultError::NotFound("cid".into()))?
        .clone();
    let pending = entry
        .pending
        .clone()
        .ok_or_else(|| VaultError::BadRequest("no pending request — call /request first".into()))?;
    if pending.version != req.version {
        return Err(VaultError::BadRequest(format!(
            "version mismatch: client says {}, pending {}",
            req.version, pending.version
        )));
    }

    let sig_c_bytes = parse_sig_65(&req.sig_c)
        .map_err(|e| VaultError::BadRequest(format!("sigC: {e}")))?;
    // Vault optionally cross-checks σ_C recovers to its own client view.
    let recovered = a402_client::sigs::recover_eth_signed(
        &pending.parsed.asc_state_hash,
        &sig_c_bytes,
    )
    .map_err(|e| VaultError::BadRequest(format!("σ_C recover: {e}")))?;
    if recovered != entry.buyer {
        return Err(VaultError::BadRequest(format!(
            "σ_C recovered to {} but channel buyer is {}",
            recovered.to_hex(),
            entry.buyer.to_hex()
        )));
    }

    // Forward to SP /finalize.
    let sp_resp = state
        .http
        .post(format!("{}/v1/sp/finalize", entry.sp_url))
        .json(&json!({
            "cid": cid.to_hex(),
            "version": req.version,
            "sigC": req.sig_c,
        }))
        .send()
        .await
        .map_err(|e| VaultError::Upstream(format!("sp finalize: {e}")))?;
    if !sp_resp.status().is_success() {
        let body = sp_resp.text().await.unwrap_or_default();
        return Err(VaultError::Upstream(format!("sp finalize: {body}")));
    }
    let finalize: SpFinalizeResp = sp_resp
        .json()
        .await
        .map_err(|e| VaultError::Upstream(format!("sp finalize decode: {e}")))?;
    let (t, sig_s) = a402_client::atomic::parse_finalize_resp(&finalize)
        .map_err(|e| VaultError::Upstream(format!("parse finalize: {e}")))?;

    // Adapt, verify_full, decrypt EncRes locally.
    let (_full, plaintext) =
        a402_client::atomic::finalize_and_decrypt(&pending.seller_pub, &pending.parsed, &t)
            .map_err(|e| VaultError::Upstream(format!("verify_full/decrypt: {e}")))?;

    // Advance channel state.
    state.channels.alter(&cid, |_, mut e| {
        e.balance_c = pending.parsed.new_balance_c;
        e.balance_s = pending.parsed.new_balance_s;
        e.version = pending.parsed.new_version;
        e.pending = None;
        e.last_sig_c = Some(sig_c_bytes);
        e.last_sig_s = Some(sig_s);
        e
    });

    use base64::Engine as _;
    let plaintext_b64 = base64::engine::general_purpose::STANDARD.encode(&plaintext);

    Ok(AxumJson(ClientPayResp {
        cid: cid.to_hex(),
        version: req.version,
        plaintext_b64,
        sig_s: format!("0x{}", hex::encode(sig_s)),
    }))
}

async fn vault_close(
    State(state): State<VaultState>,
    AxumJson(req): AxumJson<ClientCidReq>,
) -> Result<AxumJson<ClientCloseResp>, VaultError> {
    let cid = Bytes32::parse(&req.cid).map_err(|e| VaultError::BadRequest(e.to_string()))?;
    let entry = state
        .channels
        .get(&cid)
        .ok_or_else(|| VaultError::NotFound("cid".into()))?
        .clone();
    let sig_c = entry
        .last_sig_c
        .ok_or_else(|| VaultError::BadRequest("no cached σ_C; call /pay first".into()))?;
    let sig_s = entry
        .last_sig_s
        .ok_or_else(|| VaultError::BadRequest("no cached σ_S; call /pay first".into()))?;

    let rpc = EvmRpcClient::new(state.rpc_url.clone());
    let asc =
        AscManagerClient::with_signer(rpc.clone(), state.asc_manager, state.signer.clone());
    let tx_hash = asc
        .close_asc(
            &cid,
            entry.balance_c,
            entry.balance_s,
            entry.version,
            &sig_c,
            &sig_s,
        )
        .await
        .map_err(|e| VaultError::Chain(format!("closeASC: {e}")))?;
    let receipt = rpc
        .wait_receipt(&tx_hash, 60)
        .await
        .map_err(|e| VaultError::Chain(format!("closeASC receipt: {e}")))?;

    Ok(AxumJson(ClientCloseResp {
        cid: cid.to_hex(),
        tx_hash,
        block_number: receipt.block_number_u64(),
        balance_c: entry.balance_c.to_string(),
        balance_s: entry.balance_s.to_string(),
        version: entry.version,
    }))
}

impl VaultState {
    fn sp_url(&self, candidate: &str) -> String {
        candidate.trim_end_matches('/').to_string()
    }

    fn router(self) -> Router {
        Router::new()
            .route("/v1/client/channel/open", post(vault_open))
            .route("/v1/client/channel/request", post(vault_request))
            .route("/v1/client/channel/pay", post(vault_pay))
            .route("/v1/client/channel/close", post(vault_close))
            .with_state(self)
    }
}

fn parse_sig_65(hex_value: &str) -> Result<[u8; 65], String> {
    let stripped = hex_value.strip_prefix("0x").unwrap_or(hex_value);
    let raw = hex::decode(stripped).map_err(|e| e.to_string())?;
    if raw.len() != 65 {
        return Err(format!("must be 65 bytes, got {}", raw.len()));
    }
    let mut out = [0u8; 65];
    out.copy_from_slice(&raw);
    Ok(out)
}

/* -------------------------------------------------------------------------- */
/*                       SP subprocess + Vault server                          */
/* -------------------------------------------------------------------------- */

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn bind_free_port() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    (addr, listener)
}

async fn wait_http_ready(url: &str) {
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Ok(resp) = http.get(url).send().await {
            if resp.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    panic!("service at {url} never became ready");
}

/* -------------------------------------------------------------------------- */
/*                          Shared test bootstrap                              */
/* -------------------------------------------------------------------------- */

struct TestStack {
    rpc_url: String,
    asc_manager: Address,
    vault_eoa: Address,
    vault_signer: EvmSigner,
    sp_url: String,
    sp_info: a402_client::sp_http::InfoResponse,
    sp_address: Address,
    _sp_child: ChildGuard,
    vault_url: String,
    vault_task: tokio::task::JoinHandle<()>,
}

impl Drop for TestStack {
    fn drop(&mut self) {
        self.vault_task.abort();
    }
}

async fn boot_stack() -> TestStack {
    let rpc_url = std::env::var("A402_EVM_RPC_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
    let asc_manager_hex = std::env::var("A402_EVM_ASC_MANAGER")
        .expect("A402_EVM_ASC_MANAGER must be set — source .env.evm.generated");
    let vault_eoa_hex = std::env::var("A402_EVM_ASC_VAULT_EOA")
        .expect("A402_EVM_ASC_VAULT_EOA must be set");
    let asc_manager = Address::parse(&asc_manager_hex).expect("manager addr");
    let vault_eoa = Address::parse(&vault_eoa_hex).expect("vault eoa");

    let sp_priv =
        std::env::var("A402_EVM_SELLER_PRIV").unwrap_or_else(|_| ANVIL_SP_PRIV.to_string());

    let rpc = EvmRpcClient::new(rpc_url.clone());
    let chain_id = rpc.chain_id().await.expect("chain_id");
    let vault_signer = EvmSigner::from_hex(ANVIL_VAULT_PRIV, chain_id).expect("vault signer");
    assert_eq!(
        vault_signer.address().to_hex().to_lowercase(),
        vault_eoa.to_hex().to_lowercase(),
        "ANVIL_VAULT_PRIV does not match A402_EVM_ASC_VAULT_EOA",
    );

    // Spawn SP binary.
    let (sp_addr, sp_listener) = bind_free_port().await;
    drop(sp_listener);
    let sp_listen = sp_addr.to_string();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_a402-service-provider"));
    cmd.env("A402_SP_LISTEN", &sp_listen)
        .env("A402_SP_PRIV", &sp_priv)
        .env("A402_EVM_RPC_URL", &rpc_url)
        .env("A402_EVM_ASC_MANAGER", asc_manager.to_hex())
        .env("A402_EVM_VAULT_ADDR", vault_eoa.to_hex())
        .env("no_proxy", "127.0.0.1,localhost")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    let sp_child = ChildGuard(cmd.spawn().expect("spawn SP binary"));
    let sp_url = format!("http://{sp_listen}");
    wait_http_ready(&format!("{sp_url}/v1/sp/info")).await;

    let sp_client = a402_client::SpHttpClient::new(sp_url.clone()).expect("sp client");
    let sp_info = sp_client.get_info().await.expect("sp info");
    let sp_address = Address::parse(&sp_info.address).expect("sp address");

    // Spin up Vault stub.
    let vault_state = VaultState {
        rpc_url: rpc_url.clone(),
        asc_manager,
        vault_eoa,
        signer: vault_signer.clone(),
        channels: Arc::new(DashMap::new()),
        http: reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap(),
    };
    let (vault_addr, vault_listener) = bind_free_port().await;
    let std_listener = vault_listener.into_std().expect("into_std");
    std_listener.set_nonblocking(true).unwrap();
    let vault_listener = TcpListener::from_std(std_listener).unwrap();
    let vault_url = format!("http://{vault_addr}");
    let vault_router = vault_state.router();
    let vault_task = tokio::spawn(async move {
        axum::serve(vault_listener, vault_router).await.unwrap();
    });

    TestStack {
        rpc_url,
        asc_manager,
        vault_eoa,
        vault_signer,
        sp_url,
        sp_info,
        sp_address,
        _sp_child: sp_child,
        vault_url,
        vault_task,
    }
}

fn client_http() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap()
}

/// Runs one full atomic round (request + pay). Returns the asc_state_hash
/// of the settled version, the version number, the σ_C signed by the
/// Client, and the decrypted service plaintext.
struct RoundOutcome {
    asc_state_hash: [u8; 32],
    version: u64,
    #[allow(dead_code)]
    sig_c: [u8; 65],
    plaintext: String,
}

async fn run_round(
    http: &reqwest::Client,
    vault_url: &str,
    buyer: &ClientKeys,
    cid: &str,
) -> RoundOutcome {
    let resp = http
        .post(format!("{vault_url}/v1/client/channel/request"))
        .json(&json!({ "cid": cid }))
        .send()
        .await
        .expect("request send");
    assert_eq!(
        resp.status(),
        StatusCode::PAYMENT_REQUIRED,
        "Vault must return HTTP 402 (PAYMENT_REQUIRED)"
    );
    let envelope: Value = resp.json().await.expect("envelope json");
    let asc_state_hash_hex = envelope["ascStateHash"]
        .as_str()
        .expect("ascStateHash")
        .to_string();
    let version = envelope["newVersion"].as_u64().expect("newVersion");

    let state_hash = Bytes32::parse(&asc_state_hash_hex).expect("parse hash");
    let sig_c = sign_eth_signed(&buyer.ecdsa, &state_hash.0);

    let pay: ClientPayResp = http
        .post(format!("{vault_url}/v1/client/channel/pay"))
        .json(&json!({
            "cid": cid,
            "version": version,
            "sigC": format!("0x{}", hex::encode(sig_c)),
        }))
        .send()
        .await
        .expect("pay send")
        .error_for_status()
        .expect("pay status")
        .json()
        .await
        .expect("pay decode");
    use base64::Engine as _;
    let plaintext = base64::engine::general_purpose::STANDARD
        .decode(&pay.plaintext_b64)
        .expect("base64 decode");

    RoundOutcome {
        asc_state_hash: state_hash.0,
        version,
        sig_c,
        plaintext: String::from_utf8(plaintext).expect("utf8 plaintext"),
    }
}

/* -------------------------------------------------------------------------- */
/*                                  Tests                                      */
/* -------------------------------------------------------------------------- */

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn anvil_paper_flow() {
    let stack = boot_stack().await;
    let buyer_priv =
        std::env::var("A402_EVM_BUYER_PRIV").unwrap_or_else(|_| ANVIL_BUYER_PRIV.to_string());

    let buyer = ClientKeys::from_hex(&buyer_priv).expect("buyer keys");
    let http = client_http();
    let vault_url = &stack.vault_url;
    let sp_url = &stack.sp_url;
    let info = &stack.sp_info;
    let sp_address = stack.sp_address;
    let asc_manager = stack.asc_manager;
    let vault_eoa = stack.vault_eoa;
    let rpc_url = stack.rpc_url.clone();

    let deposit: u128 = 100_000;
    let open: ClientOpenResp = http
        .post(format!("{vault_url}/v1/client/channel/open"))
        .json(&json!({
            "buyer": buyer.address_hex(),
            "deposit": deposit.to_string(),
            "spUrl": sp_url,
            "spAddress": info.address,
            "spSchnorrPx": info.schnorr_px,
        }))
        .send()
        .await
        .expect("open send")
        .error_for_status()
        .expect("open status")
        .json()
        .await
        .expect("open decode");
    let cid = open.cid.clone();
    assert_eq!(
        open.sp_address.to_lowercase(),
        sp_address.to_hex().to_lowercase()
    );
    eprintln!("[open] cid={cid} tx={} block={}", open.tx_hash, open.block_number);

    // Three atomic rounds.
    for round in 1..=3u64 {
        let r = run_round(&http, vault_url, &buyer, &cid).await;
        assert_eq!(r.version, round);
        assert!(r.plaintext.contains("\"endpoint\":\"/weather\""));
        assert!(r.plaintext.contains(&format!("\"request_number\":{round}")));
        eprintln!("[round {round}] plaintext = {}", r.plaintext);
    }

    // Cross-check: Vault's cached σ_C round-trips through close, and the
    // resulting on-chain state has balance_s = 3 * PRICE_USDC_ATOMIC = 3_000.
    let close: ClientCloseResp = http
        .post(format!("{vault_url}/v1/client/channel/close"))
        .json(&json!({ "cid": cid }))
        .send()
        .await
        .expect("close send")
        .error_for_status()
        .expect("close status")
        .json()
        .await
        .expect("close decode");
    eprintln!("[close] tx={} block={}", close.tx_hash, close.block_number);
    assert_eq!(close.balance_s, "3000");
    assert_eq!(close.balance_c, (deposit - 3_000).to_string());
    assert_eq!(close.version, 3);

    // Read final on-chain state — must be CLOSED with the expected balances.
    let asc = AscManagerClient::new(EvmRpcClient::new(rpc_url.clone()), asc_manager, vault_eoa);
    let cid_bytes = Bytes32::parse(&cid).expect("parse cid");
    let final_state = asc.read_state(&cid_bytes).await.expect("read state");
    assert_eq!(final_state.balance_s, 3_000);
    assert_eq!(final_state.balance_c, deposit - 3_000);
    assert_eq!(final_state.version, 3);
    assert_eq!(final_state.status, 2, "channel must be CLOSED");

    // `stack` Drop kills the SP child and the Vault task.
    drop(stack);
}

/// Force-close path ( adversarial-buyer recovery).
///
/// Scenario: Client opens a channel and pays for ONE round. Both sides hold
/// the materials for cooperative close, but the Client (or Vault, here we
/// model the same outcome) refuses to call `/close`. The SP has σ̂_S parked
/// from `/request` and the witness `t` from `/finalize`; combined with σ_U
/// fetched out-of-band from the Vault, the SP can adapt σ̂_S into σ_S and
/// submit `ASCManager.forceClose` on its own — the on-chain Schnorr
/// verifier checks `(R, s)` so anyone observing the call can extract `t`
/// — and recover its earned funds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn anvil_force_close_sp_initiated() {
    let stack = boot_stack().await;
    let buyer_priv =
        std::env::var("A402_EVM_BUYER_PRIV").unwrap_or_else(|_| ANVIL_BUYER_PRIV.to_string());

    let buyer = ClientKeys::from_hex(&buyer_priv).expect("buyer keys");
    let http = client_http();
    let vault_url = &stack.vault_url;
    let sp_url = &stack.sp_url;
    let info = &stack.sp_info;
    let asc_manager = stack.asc_manager;
    let vault_eoa = stack.vault_eoa;
    let rpc_url = stack.rpc_url.clone();

    // (1) open
    let deposit: u128 = 100_000;
    let open: ClientOpenResp = http
        .post(format!("{vault_url}/v1/client/channel/open"))
        .json(&json!({
            "buyer": buyer.address_hex(),
            "deposit": deposit.to_string(),
            "spUrl": sp_url,
            "spAddress": info.address,
            "spSchnorrPx": info.schnorr_px,
        }))
        .send()
        .await
        .expect("open send")
        .error_for_status()
        .expect("open status")
        .json()
        .await
        .expect("open decode");
    let cid = open.cid.clone();
    eprintln!("[fc-open] cid={cid} tx={}", open.tx_hash);

    // (2) one round of request+pay so SP has last_sig_hat / last_t / last_sig_s.
    let resp = http
        .post(format!("{vault_url}/v1/client/channel/request"))
        .json(&json!({ "cid": cid }))
        .send()
        .await
        .expect("request send");
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
    let envelope: Value = resp.json().await.expect("envelope json");
    let asc_state_hash_hex = envelope["ascStateHash"]
        .as_str()
        .expect("ascStateHash")
        .to_string();
    let new_version = envelope["newVersion"].as_u64().expect("newVersion");
    let state_hash_bytes = Bytes32::parse(&asc_state_hash_hex).expect("parse hash");

    let sig_c = sign_eth_signed(&buyer.ecdsa, &state_hash_bytes.0);
    let _pay: ClientPayResp = http
        .post(format!("{vault_url}/v1/client/channel/pay"))
        .json(&json!({
            "cid": cid,
            "version": new_version,
            "sigC": format!("0x{}", hex::encode(sig_c)),
        }))
        .send()
        .await
        .expect("pay send")
        .error_for_status()
        .expect("pay status")
        .json()
        .await
        .expect("pay decode");
    eprintln!("[fc-pay] v={new_version} balance settled, NOT cooperatively closing");

    // (3) SP-initiated force-close: compute σ_U using the Vault's k256 key
    //     over the same ascStateHash from the paid round, then post to
    //     /v1/sp/force-close. (Mirrors a real deployment where the SP would
    //     have called the Vault's /channel/evm/state-sig endpoint to fetch
    //     σ_U; for the test we compute it inline.)
    let sig_u = sign_eth_signed(stack.vault_signer.signing_key(), &state_hash_bytes.0);
    let fc_resp = http
        .post(format!("{sp_url}/v1/sp/force-close"))
        .json(&json!({
            "cid": cid,
            "vaultSigU": format!("0x{}", hex::encode(sig_u)),
        }))
        .send()
        .await
        .expect("force-close send");
    let status = fc_resp.status();
    let fc_body: Value = fc_resp.json().await.expect("force-close json");
    assert!(
        status.is_success(),
        "SP /force-close failed: {status} body={fc_body}"
    );
    let fc_tx = fc_body["txHash"].as_str().expect("txHash").to_string();
    let fc_block = fc_body["blockNumber"].as_u64().expect("blockNumber");
    eprintln!("[fc-tx] tx={fc_tx} block={fc_block}");

    // (4) Read final on-chain state — must be force-closed with the
    //     post-round balances and the channel out of the OPEN status.
    let asc = AscManagerClient::new(EvmRpcClient::new(rpc_url.clone()), asc_manager, vault_eoa);
    let cid_bytes = Bytes32::parse(&cid).expect("parse cid");
    let final_state = asc.read_state(&cid_bytes).await.expect("read state");
    assert_eq!(final_state.balance_s, 1_000);
    assert_eq!(final_state.balance_c, deposit - 1_000);
    assert_eq!(final_state.version, 1);
    assert!(
        !final_state.is_open(),
        "channel must no longer be OPEN after forceClose; status={}",
        final_state.status
    );

    drop(stack);
}

/// Client-initiated force-close ( adversarial-Vault recovery).
///
/// Scenario: Client opens a channel, pays one round, then the Vault goes
/// silent (refuses to coordinate cooperative close OR to issue σ_U for the
/// SP's `forceClose` path). The Client recovers funds themselves by
/// submitting `initForceClose` with σ_C over the v=1 state hash they
/// already signed during `/pay`. After `DISPUTE_WINDOW` elapses, anyone
/// can call `finalForceClose` to lock in the payout.
///
/// Asserts:
///   - Client (acct #2) successfully submits `initForceClose` — channel
///     transitions to `Status.CLOSING` on-chain.
///   - Calling `finalForceClose` BEFORE the window expires reverts with
///     `DisputeWindowActive`.
///   - After `evm_increaseTime(DISPUTE_WINDOW + 1)` + `evm_mine`, anyone
///     can finalize (we let the Client do it). Status → `CLOSED`,
///     balances match the v=1 settlement.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn anvil_force_close_client_initiated() {
    let stack = boot_stack().await;
    let buyer_priv =
        std::env::var("A402_EVM_BUYER_PRIV").unwrap_or_else(|_| ANVIL_BUYER_PRIV.to_string());
    let buyer = ClientKeys::from_hex(&buyer_priv).expect("buyer keys");
    let http = client_http();

    // (1) Open channel + 1 round of request/pay. After this the Client holds
    //     σ_C over the v=1 ascStateHash. We DON'T call /close — the Vault
    //     has gone silent.
    let deposit: u128 = 100_000;
    let open: ClientOpenResp = http
        .post(format!("{}/v1/client/channel/open", stack.vault_url))
        .json(&json!({
            "buyer": buyer.address_hex(),
            "deposit": deposit.to_string(),
            "spUrl": &stack.sp_url,
            "spAddress": &stack.sp_info.address,
            "spSchnorrPx": &stack.sp_info.schnorr_px,
        }))
        .send()
        .await
        .expect("open send")
        .error_for_status()
        .expect("open status")
        .json()
        .await
        .expect("open decode");
    let cid = open.cid.clone();
    eprintln!("[cifc-open] cid={cid} tx={}", open.tx_hash);

    let round = run_round(&http, &stack.vault_url, &buyer, &cid).await;
    assert_eq!(round.version, 1);
    eprintln!("[cifc-round1] plaintext={}", round.plaintext);

    // (2) Client submits initForceClose themselves. msg.sender must equal
    //     the channel's `client` field, so they sign with their own k256
    //     EOA via in-enclave EIP-1559 signing (no RPC-side unlocked
    //     accounts required).
    let rpc = EvmRpcClient::new(stack.rpc_url.clone());
    let chain_id = rpc.chain_id().await.expect("chain_id");
    let buyer_signer = EvmSigner::from_hex(&buyer_priv, chain_id).expect("buyer signer");
    let buyer_asc =
        AscManagerClient::with_signer(rpc.clone(), stack.asc_manager, buyer_signer.clone());

    let cid_bytes = Bytes32::parse(&cid).expect("parse cid");
    let balance_c = deposit - 1_000;
    let balance_s = 1_000;
    let init_tx = buyer_asc
        .init_force_close(&cid_bytes, balance_c, balance_s, 1, &round.sig_c)
        .await
        .expect("initForceClose submit");
    let init_receipt = rpc
        .wait_receipt(&init_tx, 60)
        .await
        .expect("initForceClose receipt");
    eprintln!(
        "[cifc-init] tx={} block={}",
        init_tx, init_receipt.block_number_u64()
    );

    // Status must be CLOSING (1) — channel is mid-dispute.
    let state_after_init = buyer_asc.read_state(&cid_bytes).await.expect("read state");
    assert_eq!(
        state_after_init.status, 1,
        "channel must be CLOSING after initForceClose; got status={}",
        state_after_init.status
    );

    // (3) Premature finalForceClose must revert with DisputeWindowActive.
    //     `wait_receipt` returns `Reverted` cleanly.
    let early_tx = buyer_asc.final_force_close(&cid_bytes).await;
    match early_tx {
        Err(_) => eprintln!("[cifc-final-early] reverted at submission (as expected)"),
        Ok(tx) => match rpc.wait_receipt(&tx, 60).await {
            Err(_) => eprintln!("[cifc-final-early] tx mined-reverted (as expected)"),
            Ok(receipt) => {
                if receipt.status_u64() != Some(0) {
                    panic!(
                        "finalForceClose before dispute window should revert; got status={:?}",
                        receipt.status_u64()
                    );
                }
            }
        },
    }
    assert_eq!(
        buyer_asc
            .read_state(&cid_bytes)
            .await
            .expect("read state")
            .status,
        1,
        "channel must still be CLOSING after a reverted early finalize"
    );

    // (4) Advance the chain clock past DISPUTE_WINDOW = 24h, mine a block.
    rpc.evm_increase_time(24 * 3600 + 1)
        .await
        .expect("evm_increaseTime");
    rpc.evm_mine().await.expect("evm_mine");

    // (5) finalForceClose is permissionless — Client calls it.
    let final_tx = buyer_asc
        .final_force_close(&cid_bytes)
        .await
        .expect("finalForceClose submit");
    let final_receipt = rpc
        .wait_receipt(&final_tx, 60)
        .await
        .expect("finalForceClose receipt");
    eprintln!(
        "[cifc-final] tx={} block={}",
        final_tx, final_receipt.block_number_u64()
    );

    // (6) Final state: CLOSED (2) with balances from v=1 paid round.
    let final_state = buyer_asc.read_state(&cid_bytes).await.expect("read state");
    assert_eq!(final_state.status, 2, "channel must be CLOSED");
    assert_eq!(final_state.balance_c, balance_c);
    assert_eq!(final_state.balance_s, balance_s);
    assert_eq!(final_state.version, 1);

    drop(stack);
}

// Suppress dead_code warnings on fields only read inside axum handlers.
#[allow(dead_code)]
fn _force_field_use() {
    let _ = std::mem::size_of::<ChannelEntry>();
    let _ = std::mem::size_of::<PendingForPay>();
    let _ = std::mem::size_of::<VaultState>();
    let _ = std::mem::size_of::<EncryptedResult>();
    let _: Option<fn(&EncryptedResult, &k256::Scalar) -> Result<Vec<u8>, _>> = Some(decrypt_result);
}
