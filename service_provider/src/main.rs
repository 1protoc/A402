//! A402 Service Provider (S) binary.
//!
//! Per , S is an INDEPENDENT TEE role from the Vault (U). It:
//!
//!   - holds its own (sk_S, pk_S) keypair, derived in-enclave from a
//!     KMS-bound seed (here read from env in dev, real KMS in prod);
//!   - has its own code hash h_code,S and Nitro/SEV-SNP attestation;
//!   - maintains `Reg_U`, a local registry of Vault TEEs it has verified
//!     the attestation of;
//!   - exposes the atomic-flow endpoints (`register / request /
//!     finalize / force-close`) that previously lived inside `enclave/`.
//!
//! The Vault and Service Provider never share keys. After mutual attestation
//! registration each side stores only the other's `(pk, h_code, att)` and
//! authenticates per-message over an mTLS-bound channel.
//!
//! ## Env
//!
//!   A402_SP_LISTEN              — bind address (default 127.0.0.1:3700)
//!   A402_SP_PRIV                — 32-byte hex secp256k1 private key
//!   A402_EVM_RPC_URL            — chain RPC (default http://127.0.0.1:8545)
//!   A402_EVM_ASC_MANAGER        — ASCManager.sol address
//!   A402_EVM_VAULT_ADDR         — pinned Vault EOA we will accept channels for
//!
//! The deliberately-narrower env set vs the Vault makes the role
//! separation explicit: S never sees `A402_EVM_VAULT_PRIV`.

mod handlers;
mod keys;
mod registry;
mod service;

use axum::routing::{get, post};
use axum::Router;
use std::env;
use std::sync::Arc;
use tracing::info;

use keys::SellerKeys;
use registry::VaultRegistry;

pub struct AppState {
    pub keys: Arc<SellerKeys>,
    pub vault_registry: Arc<VaultRegistry>,
    pub rpc_url: String,
    pub asc_manager: String,
    pub vault_addr: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let listen = env::var("A402_SP_LISTEN").unwrap_or_else(|_| "127.0.0.1:3700".to_string());
    let priv_hex = env::var("A402_SP_PRIV")
        .expect("A402_SP_PRIV must be set (32-byte hex secp256k1 secret)");
    let rpc_url =
        env::var("A402_EVM_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
    let asc_manager =
        env::var("A402_EVM_ASC_MANAGER").expect("A402_EVM_ASC_MANAGER must be set");
    let vault_addr =
        env::var("A402_EVM_VAULT_ADDR").expect("A402_EVM_VAULT_ADDR must be set");

    let keys = Arc::new(SellerKeys::from_hex(&priv_hex).expect("invalid A402_SP_PRIV"));
    info!(
        sp_address = %keys.address_hex(),
        sp_schnorr_px = %keys.schnorr_px_hex(),
        "Service Provider keys loaded"
    );

    let vault_registry = Arc::new(VaultRegistry::new());

    let state = Arc::new(AppState {
        keys,
        vault_registry,
        rpc_url,
        asc_manager,
        vault_addr,
    });

    let app = Router::new()
        .route("/v1/sp/info", get(handlers::get_info))
        .route("/v1/sp/register-vault", post(handlers::post_register_vault))
        .route("/v1/sp/register", post(handlers::post_register))
        .route("/v1/sp/request", post(handlers::post_request))
        .route("/v1/sp/finalize", post(handlers::post_finalize))
        .route("/v1/sp/force-close", post(handlers::post_force_close))
        .with_state(state);

    let addr: std::net::SocketAddr = listen.parse().expect("A402_SP_LISTEN must be a socket address");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    info!(%addr, "service provider listening");
    axum::serve(listener, app).await.expect("serve");
}
