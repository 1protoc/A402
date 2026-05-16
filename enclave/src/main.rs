#![allow(dead_code)]

mod adaptor_sig;
mod admin_auth;
mod asc_manager;
mod attestation;
mod audit;
mod batch;
mod btc_deposit_detector;
mod btc_ledger;
mod chain_adapter;
mod deposit_detector;
mod handlers_client;
mod handlers_evm;
mod error;
mod handlers;
mod interconnect;
mod kms_bootstrap;
mod multichain_settlement;
mod outbound;
mod provider_attestation;
mod raft_setup;
mod snapshot;
mod snapshot_store;
mod state;
mod tls;
mod wal;

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use deposit_detector::DepositDetector;
use handlers::AppState;
use interconnect::ParentInterconnect;
use kms_bootstrap::bootstrap_materials;
use outbound::OutboundTransport;
use snapshot::SnapshotManager;
use snapshot_store::SnapshotStoreClient;
use state::{SolanaRuntimeConfig, VaultState};
use wal::Wal;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let vault_config = read_pubkey_env("A402_VAULT_CONFIG", Pubkey::default());
    let usdc_mint = read_pubkey_env("A402_USDC_MINT", Pubkey::default());
    let attestation_policy_hash = attestation::resolve_attestation_policy_hash_from_env()
        .expect("attestation policy hash must resolve from env or Nitro measurements");
    let solana = SolanaRuntimeConfig {
        program_id: read_pubkey_env("A402_PROGRAM_ID", a402_vault::ID),
        vault_token_account: read_pubkey_env("A402_VAULT_TOKEN_ACCOUNT", Pubkey::default()),
        rpc_url: env::var("A402_SOLANA_RPC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8899".to_string()),
        ws_url: env::var("A402_SOLANA_WS_URL")
            .unwrap_or_else(|_| "ws://127.0.0.1:8900".to_string()),
    };
    let wal_path = env::var("A402_WAL_PATH").unwrap_or_else(|_| "data/wal.jsonl".to_string());
    let listen_addr =
        env::var("A402_ENCLAVE_LISTEN").unwrap_or_else(|_| "0.0.0.0:3100".to_string());
    let ingress_port = read_env_u32("A402_ENCLAVE_INGRESS_PORT", 5000);
    let parent_interconnect = ParentInterconnect::from_env();
    let outbound = OutboundTransport::from_env(parent_interconnect);
    let snapshot_store = SnapshotStoreClient::from_env(parent_interconnect);
    let bootstrap = bootstrap_materials(
        vault_config,
        attestation_policy_hash,
        snapshot_store.clone(),
    )
    .await
    .expect("runtime bootstrap must succeed");
    let vault_signer_pubkey =
        Pubkey::new_from_array(bootstrap.signing_key.verifying_key().to_bytes());
    info!(vault_signer = %vault_signer_pubkey, "Loaded vault signer keypair");

    let vault_state = Arc::new(VaultState::new(
        vault_config,
        bootstrap.signing_key,
        usdc_mint,
        attestation_policy_hash,
        solana.clone(),
    ));

    let wal = if let Some(snapshot_store) = snapshot_store.clone() {
        let wal_prefix =
            env::var("A402_WAL_PREFIX").unwrap_or_else(|_| format!("wal/{vault_config}"));
        Arc::new(
            Wal::new_with_snapshot_store(
                PathBuf::from(&wal_path),
                bootstrap.storage_key,
                snapshot_store,
                wal_prefix,
            )
            .await,
        )
    } else {
        Arc::new(Wal::new_with_key(PathBuf::from(&wal_path), bootstrap.storage_key).await)
    };
    let deposit_detector = Arc::new(DepositDetector::new(
        solana.vault_token_account,
        solana.program_id,
        solana.rpc_url.clone(),
        solana.ws_url.clone(),
        outbound,
    ));

    let watchtower_url = env::var("A402_WATCHTOWER_URL")
        .expect("A402_WATCHTOWER_URL must be set for Phase 4 receipt mirroring");
    ensure_watchtower_ready(&watchtower_url, outbound)
        .await
        .expect("watchtower health check must succeed before enclave starts serving");
    let batch_privacy = batch::BatchPrivacyConfig::from_env();
    let manifest_hash = env::var("A402_MANIFEST_HASH_HEX").ok();

    let tls_runtime = tls::TlsRuntime::from_env().expect("TLS configuration must be valid");
    if !bootstrap.attestation.is_local_dev
        && tls_runtime
            .as_ref()
            .and_then(|runtime| runtime.binding())
            .is_none()
    {
        panic!("non-local attested runtime requires enclave TLS with attested public key binding");
    }
    let provider_mtls_enabled = tls_runtime
        .as_ref()
        .map(|runtime| runtime.mtls_enabled())
        .unwrap_or(false);
    let attestation_provider = Arc::new(
        attestation::AttestationProvider::from_bootstrap_bundle(
            bootstrap.attestation.clone(),
            tls_runtime
                .as_ref()
                .and_then(|runtime| runtime.binding().cloned()),
            manifest_hash,
        )
        .expect("attestation provider must initialize"),
    );

    let app_state = Arc::new(AppState {
        vault: vault_state,
        wal,
        deposit_detector: deposit_detector.clone(),
        batch_privacy,
        attestation_provider,
        asc_ops_lock: tokio::sync::Mutex::new(()),
        persistence_lock: tokio::sync::Mutex::new(()),
        watchtower_url: Some(watchtower_url),
        attestation_document: bootstrap.attestation.document_b64,
        attestation_is_local_dev: bootstrap.attestation.is_local_dev,
        provider_mtls_enabled,
        outbound,
        btc_ledger: tokio::sync::RwLock::new(btc_ledger::BtcUtxoLedger::new()),
    });

    let snapshot_manager = snapshot_store
        .clone()
        .and_then(|client| SnapshotManager::from_env(client, bootstrap.storage_key, vault_config))
        .map(Arc::new);
    let enable_provider_registration_api =
        read_env_bool("A402_ENABLE_PROVIDER_REGISTRATION_API");
    let enable_admin_api = read_env_bool("A402_ENABLE_ADMIN_API");

    let replay_from = if let Some(manager) = snapshot_manager.as_ref() {
        manager
            .recover_latest(&app_state)
            .await
            .expect("snapshot recovery must succeed")
    } else {
        None
    };

    wal::replay_app_state_from(&app_state, replay_from)
        .await
        .expect("WAL replay must succeed on startup");

    // Spawn background tasks (batch settlement, reservation expiry)
    batch::spawn_background_tasks(app_state.clone());

    // Spawn deposit detection (monitors on-chain deposits to update client balances)
    deposit_detector::spawn_deposit_detector(app_state.clone(), deposit_detector);

    // Optional Bitcoin deposit detector (slice 3B). Skipped when
    // A402_BITCOIN_RPC_URL / A402_BITCOIN_VAULT_PRIV aren't set.
    match btc_deposit_detector::BtcDepositDetector::from_env() {
        Ok(Some(det)) => {
            btc_deposit_detector::spawn_detector(det, app_state.clone());
            info!("Bitcoin deposit detector spawned");
        }
        Ok(None) => {}
        Err(e) => panic!("BTC deposit detector env error: {e}"),
    }

    if let Some(manager) = snapshot_manager {
        manager.spawn_background_task(app_state.clone());
    }

    // Optional Raft committee bootstrap (slice 2B).
    // `A402_RAFT_PEERS` empty/unset → single-node legacy path, no change.
    // Non-empty → bring up RaftCommittee, serve raft RPCs on
    // `A402_RAFT_LISTEN`; slice 2C will route /v1/verify and /v1/settle
    // mutations through the committee before returning 200.
    let _raft_committee = match raft_setup::RaftEnv::from_env() {
        Ok(Some(env)) => match raft_setup::start_committee(env).await {
            Ok(c) => Some(c),
            Err(e) => panic!("raft committee bring-up failed: {e}"),
        },
        Ok(None) => {
            info!("A402_RAFT_PEERS empty → running Vault in single-node mode");
            None
        }
        Err(e) => panic!("raft env parse error: {e}"),
    };

    let mut app = Router::new()
        .route("/v1/attestation", get(handlers::get_attestation))
        .route("/v1/verify", post(handlers::post_verify))
        .route("/v1/settle", post(handlers::post_settle))
        .route(
            "/v1/settlement/status",
            post(handlers::post_settlement_status),
        )
        .route("/v1/cancel", post(handlers::post_cancel))
        .route("/v1/withdraw-auth", post(handlers::post_withdraw_auth))
        .route("/v1/balance", post(handlers::post_balance))
        .route("/v1/receipt", post(handlers::post_receipt))
        // Phase 3: ASC channel endpoints
        .route("/v1/channel/open", post(handlers::post_channel_open))
        .route("/v1/channel/request", post(handlers::post_channel_request))
        .route("/v1/channel/deliver", post(handlers::post_channel_deliver))
        .route(
            "/v1/channel/finalize",
            post(handlers::post_channel_finalize),
        )
        .route("/v1/channel/close", post(handlers::post_channel_close))
        // Phase C: EVM ASC channel endpoints — independent of the Solana
        // routes above; mounted unconditionally so callers can probe and
        // receive HTTP 503 when the EVM env vars aren't configured.
        .route(
            "/v1/channel/evm/open",
            post(handlers_evm::post_channel_evm_open),
        )
        .route(
            "/v1/channel/evm/close",
            post(handlers_evm::post_channel_evm_close),
        )
        .route(
            "/v1/channel/evm/:cid",
            get(handlers_evm::get_channel_evm_state),
        )
        // The Vault's contribution to a Service Provider force-close —
        // produces σ_U over an already (buyer+seller)-signed ascStateHash.
        // SP-role endpoints (register / request / finalize / force-close)
        // moved to the `service_provider/` binary in the role split.
        .route(
            "/v1/channel/evm/state-sig",
            post(handlers_evm::post_channel_evm_state_sig),
        )
        // Client-facing delegation routes. The
        // Vault acts as C's TEE-backed proxy: opens the on-chain channel,
        // forwards per-request intents to the SP, verifies σ̂_S /
        // decrypts EncRes locally, and returns the plaintext to C only
        // after the atomic exchange completes.
        .route("/v1/client/open", post(handlers_client::post_client_open))
        .route(
            "/v1/client/request",
            post(handlers_client::post_client_request),
        )
        .route("/v1/client/pay", post(handlers_client::post_client_pay))
        .route(
            "/v1/client/close",
            post(handlers_client::post_client_close),
        );
    if enable_provider_registration_api {
        let provider_registration_routes = Router::new()
            .route(
                "/v1/provider/register",
                post(handlers::post_register_provider),
            )
            .route_layer(middleware::from_fn(admin_auth::require_admin_auth));
        app = app.merge(provider_registration_routes);
    }
    if enable_admin_api {
        let admin_routes = Router::new()
            .route("/v1/admin/seed-balance", post(handlers::post_seed_balance))
            .route("/v1/admin/fire-batch", post(handlers::post_fire_batch))
            .route_layer(middleware::from_fn(admin_auth::require_admin_auth));
        app = app.merge(admin_routes);
    }
    let app = app.with_state(app_state);

    let bind_label = match parent_interconnect.mode() {
        interconnect::InterconnectMode::Tcp => listen_addr.clone(),
        interconnect::InterconnectMode::Vsock => format!("vsock:{ingress_port}"),
    };
    info!(
        addr = %bind_label,
        interconnect = parent_interconnect.mode().label(),
        vault_config = %vault_config,
        program_id = %solana.program_id,
        provider_registration_api = enable_provider_registration_api,
        admin_api = enable_admin_api,
        "Enclave facilitator starting"
    );

    let listener =
        interconnect::bind_ingress_listener(parent_interconnect.mode(), &listen_addr, ingress_port)
            .await
            .unwrap();
    tls::serve(listener, app, tls_runtime).await.unwrap();
}

async fn ensure_watchtower_ready(url: &str, outbound: OutboundTransport) -> Result<(), String> {
    let status_url = format!("{url}/v1/status");
    let (status, _) = outbound
        .get_json::<serde_json::Value>(&status_url)
        .await
        .map_err(|error| format!("failed to reach watchtower at {status_url}: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "watchtower health check returned status {}",
            status
        ));
    }
    Ok(())
}

fn read_pubkey_env(name: &str, default: Pubkey) -> Pubkey {
    env::var(name)
        .ok()
        .map(|value| {
            value
                .parse()
                .unwrap_or_else(|_| panic!("{name} must be a valid Pubkey"))
        })
        .unwrap_or(default)
}

fn read_env_u32(name: &str, default: u32) -> u32 {
    env::var(name)
        .ok()
        .map(|value| {
            value
                .parse()
                .unwrap_or_else(|_| panic!("{name} must be a valid u32"))
        })
        .unwrap_or(default)
}

fn read_env_bool(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}
