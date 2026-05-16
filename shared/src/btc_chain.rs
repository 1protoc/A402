//! Minimal JSON-RPC client for `bitcoind`, used by the Vault batch
//! settlement submitter (and by integration tests against `bitcoind -regtest`).
//!
//! Why not `bitcoincore-rpc` crate? It pulls in `tokio-blocking` style
//! sync APIs and forces a specific HTTP stack. We already depend on
//! `reqwest`, so a 200-line async wrapper is cheaper and lets us match
//! the EVM-side [`crate::evm_chain::EvmRpcClient`] shape exactly.
//!
//! The methods covered here are the bare minimum for the batch-settlement
//! flow:
//!   - `getblockchaininfo` — health probe
//!   - `getblockcount`     — for confirmation polling
//!   - `getrawtransaction` — fetch a prior tx (currently used only by
//!                           tests; the production submitter passes UTXO
//!                           metadata in directly from the WAL)
//!   - `listunspent`       — pick UTXOs (dev path; production should track
//!                           Vault-owned UTXOs in the WAL instead of
//!                           round-tripping bitcoind)
//!   - `sendrawtransaction`— broadcast our signed tx
//!   - `generatetoaddress` — regtest only; lets tests mine blocks

use std::time::Duration;

use bitcoin::consensus::encode::serialize_hex;
use bitcoin::{Transaction, Txid};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BtcRpcError {
    #[error("http: {0}")]
    Http(String),
    #[error("json: {0}")]
    Json(String),
    #[error("rpc {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("invalid response: {0}")]
    Invalid(String),
}

impl From<reqwest::Error> for BtcRpcError {
    fn from(e: reqwest::Error) -> Self {
        BtcRpcError::Http(e.to_string())
    }
}

impl From<serde_json::Error> for BtcRpcError {
    fn from(e: serde_json::Error) -> Self {
        BtcRpcError::Json(e.to_string())
    }
}

#[derive(Clone)]
pub struct BtcRpcClient {
    url: String,
    user: String,
    pass: String,
    http: HttpClient,
    wallet: Option<String>,
}

impl BtcRpcClient {
    /// Build a client. `url` is the bitcoind RPC endpoint without a
    /// trailing wallet path (e.g. `http://127.0.0.1:18443`).
    pub fn new(url: impl Into<String>, user: impl Into<String>, pass: impl Into<String>) -> Self {
        let http = HttpClient::builder()
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Self {
            url: url.into().trim_end_matches('/').to_string(),
            user: user.into(),
            pass: pass.into(),
            http,
            wallet: None,
        }
    }

    /// Set the wallet path used for wallet-scoped RPCs (createwallet,
    /// listunspent, sendtoaddress, …). Falls back to the daemon-level
    /// endpoint when `None`.
    pub fn with_wallet(mut self, wallet: impl Into<String>) -> Self {
        self.wallet = Some(wallet.into());
        self
    }

    fn endpoint(&self) -> String {
        match &self.wallet {
            Some(w) => format!("{}/wallet/{w}", self.url),
            None => self.url.clone(),
        }
    }

    async fn call<R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<R, BtcRpcError> {
        #[derive(Serialize)]
        struct Req<'a> {
            jsonrpc: &'a str,
            id: &'a str,
            method: &'a str,
            params: Value,
        }
        let body = Req {
            jsonrpc: "1.0",
            id: "a402",
            method,
            params,
        };

        let resp = self
            .http
            .post(&self.endpoint())
            .basic_auth(&self.user, Some(&self.pass))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        let parsed: Value = serde_json::from_slice(&bytes).map_err(|e| {
            BtcRpcError::Invalid(format!(
                "decode response (status={status}): {e} body={}",
                String::from_utf8_lossy(&bytes)
            ))
        })?;

        if let Some(err) = parsed.get("error").filter(|v| !v.is_null()) {
            let code = err.get("code").and_then(Value::as_i64).unwrap_or(-1);
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("(no message)")
                .to_string();
            return Err(BtcRpcError::Rpc { code, message });
        }
        let result = parsed
            .get("result")
            .ok_or_else(|| BtcRpcError::Invalid("missing 'result' field".to_string()))?;
        serde_json::from_value(result.clone()).map_err(Into::into)
    }

    pub async fn getblockcount(&self) -> Result<u64, BtcRpcError> {
        self.call("getblockcount", json!([])).await
    }

    pub async fn getblockchaininfo(&self) -> Result<Value, BtcRpcError> {
        self.call("getblockchaininfo", json!([])).await
    }

    pub async fn sendrawtransaction(&self, tx: &Transaction) -> Result<Txid, BtcRpcError> {
        let hex = serialize_hex(tx);
        let txid_str: String = self.call("sendrawtransaction", json!([hex])).await?;
        txid_str
            .parse::<Txid>()
            .map_err(|e| BtcRpcError::Invalid(format!("Txid parse: {e}")))
    }

    pub async fn getrawtransaction_hex(&self, txid: &Txid) -> Result<String, BtcRpcError> {
        self.call("getrawtransaction", json!([txid.to_string(), false]))
            .await
    }

    /// Decode a tx with bitcoind so we can inspect vouts / scriptPubKey
    /// without re-implementing the Bitcoin script address decoder. The
    /// returned `Value` mirrors `bitcoin-cli getrawtransaction <txid> true`.
    pub async fn getrawtransaction_verbose(&self, txid: &Txid) -> Result<Value, BtcRpcError> {
        self.call("getrawtransaction", json!([txid.to_string(), true]))
            .await
    }

    /// Tells bitcoind to broadcast then mine `n_blocks` blocks to `address`.
    /// regtest only.
    pub async fn generate_to_address(
        &self,
        n_blocks: u32,
        address: &str,
    ) -> Result<Vec<String>, BtcRpcError> {
        self.call("generatetoaddress", json!([n_blocks, address])).await
    }

    pub async fn listunspent(
        &self,
        minconf: u32,
        address_filter: Option<&[String]>,
    ) -> Result<Value, BtcRpcError> {
        let params = match address_filter {
            Some(addrs) => json!([minconf, 9999999, addrs]),
            None => json!([minconf]),
        };
        self.call("listunspent", params).await
    }

    pub async fn createwallet(&self, name: &str) -> Result<Value, BtcRpcError> {
        // disable_private_keys=false (we don't use bitcoind's wallet to sign
        // anything; it's just funding scratch space for the test)
        self.call("createwallet", json!([name])).await
    }

    pub async fn loadwallet(&self, name: &str) -> Result<Value, BtcRpcError> {
        self.call("loadwallet", json!([name])).await
    }

    pub async fn getnewaddress(&self) -> Result<String, BtcRpcError> {
        self.call("getnewaddress", json!([])).await
    }

    pub async fn sendtoaddress(
        &self,
        address: &str,
        amount_btc: f64,
    ) -> Result<String, BtcRpcError> {
        self.call("sendtoaddress", json!([address, amount_btc])).await
    }

    /// Import a watch-only address so listunspent surfaces UTXOs paid to it.
    pub async fn importaddress(&self, address: &str, label: &str) -> Result<Value, BtcRpcError> {
        self.call("importaddress", json!([address, label, false])).await
    }

    pub async fn getrawmempool(&self) -> Result<Vec<String>, BtcRpcError> {
        self.call("getrawmempool", json!([])).await
    }

    /// `scantxoutset start [<descriptor>]` — snapshot the UTXO set matching
    /// a descriptor (here always `addr(<vault_address>)`). Works on any
    /// wallet mode (including v25+ descriptor wallets which dropped
    /// `importaddress`), so the deposit detector can poll without
    /// requiring a wallet-side import.
    ///
    /// Returns the raw RPC `Value`; `unspents` is the field of interest.
    pub async fn scantxoutset_addr(&self, address: &str) -> Result<Value, BtcRpcError> {
        self.call(
            "scantxoutset",
            json!(["start", [{ "desc": format!("addr({address})") }]]),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc_asc_channel::{BtcAscChannel, ChannelOutput};
    use crate::btc_asc_script::AscScriptTree;
    use crate::btc_tx::{build_settlement_tx, build_settlement_tx_p2tr, BtcKeys, Payout, VaultUtxo};
    use bitcoin::key::Keypair;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use bitcoin::Network;

    /// Run with:
    ///   bitcoind -regtest -rpcuser=a402 -rpcpassword=a402 -rpcport=18443 -fallbackfee=0.0002 -daemon
    ///   cargo test -p a402-shared --lib btc_chain::tests::regtest_send_settlement -- --ignored --nocapture
    ///
    /// What this test asserts end-to-end:
    ///   1. Vault owns a P2WPKH address derived from its in-process k1 key.
    ///   2. bitcoind funds that address with 1 BTC, mines 100 blocks to mature it.
    ///   3. Our `build_settlement_tx` produces a fully signed transaction with
    ///      one OP_RETURN commitment + two payouts + change.
    ///   4. bitcoind accepts it via `sendrawtransaction`.
    ///   5. After mining one more block, the payout outputs are visible on chain.
    #[tokio::test]
    #[ignore]
    async fn regtest_send_settlement() {
        let rpc_url = std::env::var("A402_BITCOIN_RPC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:18443".to_string());
        let rpc_user =
            std::env::var("A402_BITCOIN_RPC_USER").unwrap_or_else(|_| "a402".to_string());
        let rpc_pass =
            std::env::var("A402_BITCOIN_RPC_PASSWORD").unwrap_or_else(|_| "a402".to_string());

        let daemon = BtcRpcClient::new(&rpc_url, &rpc_user, &rpc_pass);
        // health check
        let info = daemon.getblockchaininfo().await.expect("getblockchaininfo");
        assert_eq!(info["chain"].as_str().unwrap(), "regtest");

        // Use a deterministic test wallet so re-runs don't accumulate state.
        // Bitcoin Core v25+ defaults to descriptor wallets — we just need a
        // wallet to fund a P2WPKH address; we don't ask bitcoind to import
        // or watch the vault address (descriptor wallets reject
        // `importaddress`), we just parse the funding tx ourselves.
        let wallet_name = "a402-regtest";
        let _ = daemon.createwallet(wallet_name).await; // ignore "already exists"
        let _ = daemon.loadwallet(wallet_name).await; // ignore "already loaded"
        let wallet = daemon.clone().with_wallet(wallet_name.to_string());

        // -- vault key + address --
        let vault_priv_hex =
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let keys = BtcKeys::from_hex(vault_priv_hex, Network::Regtest).expect("vault keys");
        let vault_addr = keys.p2wpkh_address();
        let vault_addr_str = vault_addr.to_string();

        // Fund the vault address with 1 BTC, then mine 101 blocks to a fresh
        // address so the funding is mature.
        let miner_addr = wallet.getnewaddress().await.expect("getnewaddress");
        let _ = wallet.generate_to_address(101, &miner_addr).await;
        let funding_txid_str = wallet
            .sendtoaddress(&vault_addr_str, 1.0)
            .await
            .expect("sendtoaddress");
        let _ = wallet.generate_to_address(1, &miner_addr).await;
        let funding_txid: Txid = funding_txid_str.parse().expect("txid parse");

        // Inspect the funding tx ourselves to find which vout went to the
        // vault address. This matches the production path: the Vault tracks
        // its own UTXOs in WAL, never relies on bitcoind's wallet.
        let funding_tx = daemon
            .getrawtransaction_verbose(&funding_txid)
            .await
            .expect("getrawtransaction_verbose funding");
        let vouts = funding_tx["vout"].as_array().expect("vout array");
        let (vout_idx, vout_obj) = vouts
            .iter()
            .enumerate()
            .find(|(_, v)| {
                v["scriptPubKey"]["address"].as_str() == Some(vault_addr_str.as_str())
            })
            .expect("vault vout in funding tx");
        let value_btc = vout_obj["value"].as_f64().expect("vout value");
        let value_sat = (value_btc * 100_000_000.0).round() as u64;
        let utxo = VaultUtxo {
            txid: funding_txid,
            vout: vout_idx as u32,
            value_sat,
        };

        // -- providers --
        let provider_addrs: Vec<bitcoin::Address> = (0..2u8)
            .map(|i| {
                let mut seed = [0u8; 32];
                seed[31] = 0x10 + i;
                BtcKeys::from_bytes(&seed, Network::Regtest)
                    .unwrap()
                    .p2wpkh_address()
            })
            .collect();
        let payouts: Vec<Payout> = provider_addrs
            .iter()
            .map(|a| Payout {
                address: a.clone(),
                amount_sat: 30_000,
            })
            .collect();

        // -- build + send --
        let commit_hash = {
            use bitcoin::hashes::{sha256d, Hash as _};
            let mut buf = Vec::new();
            for p in &payouts {
                buf.extend_from_slice(p.address.script_pubkey().as_bytes());
                buf.extend_from_slice(&p.amount_sat.to_le_bytes());
            }
            *sha256d::Hash::hash(&buf).as_byte_array()
        };
        let fee_sat = 500;
        let tx = build_settlement_tx(&commit_hash, &payouts, &[utxo], &vault_addr, fee_sat, &keys)
            .expect("build settlement tx");

        let sent_txid = daemon
            .sendrawtransaction(&tx)
            .await
            .expect("sendrawtransaction");
        eprintln!("[btc] settlement tx broadcast: {sent_txid}");

        // Mine one more block, then assert payout outputs are visible.
        let _ = wallet.generate_to_address(1, &miner_addr).await;

        // Re-fetch the settlement tx and assert provider 0's address
        // appears in one of the vouts at the expected amount. No reliance
        // on bitcoind's wallet — same path the production receipt watcher
        // will take.
        let settle_tx = daemon
            .getrawtransaction_verbose(&sent_txid)
            .await
            .expect("getrawtransaction_verbose settlement");
        let settle_vouts = settle_tx["vout"].as_array().expect("vout array");
        let p0_str = provider_addrs[0].to_string();
        let provider_vout = settle_vouts.iter().find(|v| {
            v["scriptPubKey"]["address"].as_str() == Some(p0_str.as_str())
        });
        let provider_vout = provider_vout.unwrap_or_else(|| {
            panic!("provider 0 ({p0_str}) not found in settlement tx vouts: {settle_vouts:?}")
        });
        let provider_value_btc = provider_vout["value"].as_f64().expect("provider value");
        let provider_value_sat = (provider_value_btc * 100_000_000.0).round() as u64;
        assert_eq!(provider_value_sat, 30_000);
        eprintln!(
            "[btc] provider 0 received {provider_value_sat} sats from settlement tx ✓"
        );
    }

    /// End-to-end Taproot path (Bitcoin Slice 4A): fund a vault P2TR
    /// address, build a settlement spending it via BIP-341 key-spend,
    /// broadcast, mine, then assert a P2TR payout landed at the expected
    /// amount. Run with the same env vars as `regtest_send_settlement`.
    #[tokio::test]
    #[ignore]
    async fn regtest_send_settlement_p2tr() {
        let rpc_url = std::env::var("A402_BITCOIN_RPC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:18443".to_string());
        let rpc_user =
            std::env::var("A402_BITCOIN_RPC_USER").unwrap_or_else(|_| "a402".to_string());
        let rpc_pass =
            std::env::var("A402_BITCOIN_RPC_PASSWORD").unwrap_or_else(|_| "a402".to_string());

        let daemon = BtcRpcClient::new(&rpc_url, &rpc_user, &rpc_pass);
        let info = daemon.getblockchaininfo().await.expect("getblockchaininfo");
        assert_eq!(info["chain"].as_str().unwrap(), "regtest");

        let wallet_name = "a402-regtest-p2tr";
        let _ = daemon.createwallet(wallet_name).await;
        let _ = daemon.loadwallet(wallet_name).await;
        let wallet = daemon.clone().with_wallet(wallet_name.to_string());

        // Use a different seed than the P2WPKH test so the two runs can
        // coexist without UTXO collisions on the same regtest datadir.
        let vault_priv_hex =
            "0x4f3edf983ac636a65a842ce7c78d9aa706d3b113bce9c46f30d7d21715b23b1d";
        let keys = BtcKeys::from_hex(vault_priv_hex, Network::Regtest).expect("vault keys");
        let vault_addr = keys.p2tr_address();
        let vault_addr_str = vault_addr.to_string();
        assert!(
            vault_addr_str.starts_with("bcrt1p"),
            "expected P2TR bech32m, got {vault_addr_str}"
        );

        // Fund + mature.
        let miner_addr = wallet.getnewaddress().await.expect("getnewaddress");
        let _ = wallet.generate_to_address(101, &miner_addr).await;
        let funding_txid_str = wallet
            .sendtoaddress(&vault_addr_str, 1.0)
            .await
            .expect("sendtoaddress");
        let _ = wallet.generate_to_address(1, &miner_addr).await;
        let funding_txid: Txid = funding_txid_str.parse().expect("txid parse");

        // Find the vault vout.
        let funding_tx = daemon
            .getrawtransaction_verbose(&funding_txid)
            .await
            .expect("getrawtransaction_verbose funding");
        let vouts = funding_tx["vout"].as_array().expect("vout array");
        let (vout_idx, vout_obj) = vouts
            .iter()
            .enumerate()
            .find(|(_, v)| {
                v["scriptPubKey"]["address"].as_str() == Some(vault_addr_str.as_str())
            })
            .expect("vault vout in funding tx");
        let value_btc = vout_obj["value"].as_f64().expect("vout value");
        let value_sat = (value_btc * 100_000_000.0).round() as u64;
        let utxo = VaultUtxo {
            txid: funding_txid,
            vout: vout_idx as u32,
            value_sat,
        };

        // Provider 0 is a P2TR address (proves cross-type payouts work);
        // provider 1 is P2WPKH.
        let mut p0_seed = [0u8; 32];
        p0_seed[31] = 0x50;
        let p0_keys = BtcKeys::from_bytes(&p0_seed, Network::Regtest).unwrap();
        let p0_addr = p0_keys.p2tr_address();
        let mut p1_seed = [0u8; 32];
        p1_seed[31] = 0x51;
        let p1_keys = BtcKeys::from_bytes(&p1_seed, Network::Regtest).unwrap();
        let p1_addr = p1_keys.p2wpkh_address();
        let payouts: Vec<Payout> = vec![
            Payout {
                address: p0_addr.clone(),
                amount_sat: 25_000,
            },
            Payout {
                address: p1_addr.clone(),
                amount_sat: 25_000,
            },
        ];

        // Build + send via Taproot key-spend.
        let commit_hash = {
            use bitcoin::hashes::{sha256d, Hash as _};
            let mut buf = Vec::new();
            for p in &payouts {
                buf.extend_from_slice(p.address.script_pubkey().as_bytes());
                buf.extend_from_slice(&p.amount_sat.to_le_bytes());
            }
            *sha256d::Hash::hash(&buf).as_byte_array()
        };
        let fee_sat = 500;
        let tx = build_settlement_tx_p2tr(
            &commit_hash,
            &payouts,
            &[utxo],
            &vault_addr,
            fee_sat,
            &keys,
        )
        .expect("build p2tr settlement tx");

        let sent_txid = daemon
            .sendrawtransaction(&tx)
            .await
            .expect("sendrawtransaction p2tr");
        eprintln!("[btc-p2tr] settlement tx broadcast: {sent_txid}");

        let _ = wallet.generate_to_address(1, &miner_addr).await;

        // Verify P2TR payout to provider 0 landed at the right address +
        // value.
        let settle_tx = daemon
            .getrawtransaction_verbose(&sent_txid)
            .await
            .expect("getrawtransaction_verbose settlement");
        let settle_vouts = settle_tx["vout"].as_array().expect("vout array");
        let p0_str = p0_addr.to_string();
        let provider_vout = settle_vouts
            .iter()
            .find(|v| v["scriptPubKey"]["address"].as_str() == Some(p0_str.as_str()))
            .unwrap_or_else(|| {
                panic!("p2tr provider 0 ({p0_str}) not found in vouts: {settle_vouts:?}")
            });
        let provider_value_btc = provider_vout["value"].as_f64().expect("provider value");
        let provider_value_sat = (provider_value_btc * 100_000_000.0).round() as u64;
        assert_eq!(provider_value_sat, 25_000);
        eprintln!(
            "[btc-p2tr] provider 0 (P2TR) received {provider_value_sat} sats ✓"
        );
    }

    /// End-to-end ASC channel cooperative close (Bitcoin Slice 4C):
    ///   1. Build an AscScriptTree (coop + vault + client + sp xonly keys
    ///      + 144-block dispute window).
    ///   2. Fund the resulting P2TR Taproot address on regtest.
    ///   3. Build a cooperative-close transaction spending the funding
    ///      UTXO via key-path Schnorr (the channel's on-chain "hot path").
    ///   4. Broadcast, mine, assert client + sp received their close
    ///      amounts.
    #[tokio::test]
    #[ignore]
    async fn regtest_asc_cooperative_close() {
        let rpc_url = std::env::var("A402_BITCOIN_RPC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:18443".to_string());
        let rpc_user =
            std::env::var("A402_BITCOIN_RPC_USER").unwrap_or_else(|_| "a402".to_string());
        let rpc_pass =
            std::env::var("A402_BITCOIN_RPC_PASSWORD").unwrap_or_else(|_| "a402".to_string());
        let daemon = BtcRpcClient::new(&rpc_url, &rpc_user, &rpc_pass);
        let info = daemon.getblockchaininfo().await.expect("getblockchaininfo");
        assert_eq!(info["chain"].as_str().unwrap(), "regtest");

        let wallet_name = "a402-regtest-asc";
        let _ = daemon.createwallet(wallet_name).await;
        let _ = daemon.loadwallet(wallet_name).await;
        let wallet = daemon.clone().with_wallet(wallet_name.to_string());

        // Channel keys.
        let coop_keys = BtcKeys::from_hex(
            "0x9090909090909090909090909090909090909090909090909090909090909090",
            Network::Regtest,
        )
        .unwrap();
        let vault_keys = BtcKeys::from_hex(
            "0x9191919191919191919191919191919191919191919191919191919191919191",
            Network::Regtest,
        )
        .unwrap();
        let client_keys = BtcKeys::from_hex(
            "0x9292929292929292929292929292929292929292929292929292929292929292",
            Network::Regtest,
        )
        .unwrap();
        let sp_keys = BtcKeys::from_hex(
            "0x9393939393939393939393939393939393939393939393939393939393939393",
            Network::Regtest,
        )
        .unwrap();

        let tree = AscScriptTree {
            cooperative_xonly: coop_keys.taproot_internal_xonly(),
            vault_xonly: vault_keys.taproot_internal_xonly(),
            client_xonly: client_keys.taproot_internal_xonly(),
            sp_xonly: sp_keys.taproot_internal_xonly(),
            dispute_window_blocks: 144,
        };
        let asc = tree
            .build(Network::Regtest)
            .expect("script tree builds");
        let channel_addr = asc.address.to_string();
        assert!(channel_addr.starts_with("bcrt1p"), "expected P2TR, got {channel_addr}");

        // Fund the channel + mature.
        let miner_addr = wallet.getnewaddress().await.expect("getnewaddress");
        let _ = wallet.generate_to_address(101, &miner_addr).await;
        let funding_txid_str = wallet
            .sendtoaddress(&channel_addr, 0.001)
            .await
            .expect("sendtoaddress");
        let _ = wallet.generate_to_address(1, &miner_addr).await;
        let funding_txid: Txid = funding_txid_str.parse().expect("txid parse");

        // Find vault vout.
        let funding_tx = daemon
            .getrawtransaction_verbose(&funding_txid)
            .await
            .expect("getrawtransaction_verbose");
        let vouts = funding_tx["vout"].as_array().expect("vout array");
        let (vout_idx, vout_obj) = vouts
            .iter()
            .enumerate()
            .find(|(_, v)| v["scriptPubKey"]["address"].as_str() == Some(channel_addr.as_str()))
            .expect("channel vout in funding tx");
        let value_btc = vout_obj["value"].as_f64().expect("value");
        let value_sat = (value_btc * 100_000_000.0).round() as u64;
        let funding = VaultUtxo {
            txid: funding_txid,
            vout: vout_idx as u32,
            value_sat,
        };

        // Build the channel + sign cooperative-close.
        let channel =
            BtcAscChannel::new(tree, Network::Regtest, funding).expect("channel new");
        let payouts = vec![
            ChannelOutput {
                address: client_keys.p2tr_address(),
                amount_sat: 60_000,
            },
            ChannelOutput {
                address: sp_keys.p2tr_address(),
                amount_sat: value_sat - 60_000 - 1_000,
            },
        ];
        let secp = Secp256k1::new();
        let coop_sk =
            SecretKey::from_slice(&coop_keys.secret[..]).expect("coop sk");
        let coop_kp = Keypair::from_secret_key(&secp, &coop_sk);
        let close_tx = channel
            .cooperative_close_tx(&payouts, 1_000, &coop_kp)
            .expect("cooperative_close_tx");

        let close_txid = daemon
            .sendrawtransaction(&close_tx)
            .await
            .expect("sendrawtransaction cooperative-close");
        eprintln!("[btc-asc] cooperative close broadcast: {close_txid}");

        let _ = wallet.generate_to_address(1, &miner_addr).await;
        let close_tx_v = daemon
            .getrawtransaction_verbose(&close_txid)
            .await
            .expect("getrawtransaction_verbose close");
        let close_vouts = close_tx_v["vout"].as_array().expect("close vout array");
        let client_addr_str = client_keys.p2tr_address().to_string();
        let client_vout = close_vouts
            .iter()
            .find(|v| v["scriptPubKey"]["address"].as_str() == Some(client_addr_str.as_str()))
            .expect("client payout vout present");
        let client_value_sat =
            (client_vout["value"].as_f64().unwrap() * 100_000_000.0).round() as u64;
        assert_eq!(client_value_sat, 60_000);
        eprintln!(
            "[btc-asc] client received {client_value_sat} sats via key-path close ✓"
        );
    }

    /// End-to-end adversarial-vault recovery (script-path leaf B).
    /// Same setup as `regtest_asc_cooperative_close` but the close path
    /// is `adv_vault_recovery_tx` — Client + SP joint sigs bypass the
    /// Vault entirely. Proves the leaf B script + witness order accepted
    /// by Bitcoin Core.
    #[tokio::test]
    #[ignore]
    async fn regtest_asc_adv_vault_recovery() {
        let rpc_url = std::env::var("A402_BITCOIN_RPC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:18443".to_string());
        let rpc_user =
            std::env::var("A402_BITCOIN_RPC_USER").unwrap_or_else(|_| "a402".to_string());
        let rpc_pass =
            std::env::var("A402_BITCOIN_RPC_PASSWORD").unwrap_or_else(|_| "a402".to_string());
        let daemon = BtcRpcClient::new(&rpc_url, &rpc_user, &rpc_pass);
        let info = daemon.getblockchaininfo().await.expect("getblockchaininfo");
        assert_eq!(info["chain"].as_str().unwrap(), "regtest");

        let wallet_name = "a402-regtest-asc";
        let _ = daemon.createwallet(wallet_name).await;
        let _ = daemon.loadwallet(wallet_name).await;
        let wallet = daemon.clone().with_wallet(wallet_name.to_string());

        // Distinct seeds from the cooperative-close test so the runs can
        // coexist on one regtest datadir without UTXO collisions.
        let coop_keys = BtcKeys::from_hex(
            "0xa0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0",
            Network::Regtest,
        )
        .unwrap();
        let vault_keys = BtcKeys::from_hex(
            "0xa1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
            Network::Regtest,
        )
        .unwrap();
        let client_keys = BtcKeys::from_hex(
            "0xa2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2",
            Network::Regtest,
        )
        .unwrap();
        let sp_keys = BtcKeys::from_hex(
            "0xa3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3",
            Network::Regtest,
        )
        .unwrap();
        let tree = AscScriptTree {
            cooperative_xonly: coop_keys.taproot_internal_xonly(),
            vault_xonly: vault_keys.taproot_internal_xonly(),
            client_xonly: client_keys.taproot_internal_xonly(),
            sp_xonly: sp_keys.taproot_internal_xonly(),
            dispute_window_blocks: 144,
        };
        let asc = tree.build(Network::Regtest).expect("script tree builds");
        let channel_addr = asc.address.to_string();

        // Fund + mature + find vout.
        let miner_addr = wallet.getnewaddress().await.expect("getnewaddress");
        let _ = wallet.generate_to_address(101, &miner_addr).await;
        let funding_txid_str = wallet
            .sendtoaddress(&channel_addr, 0.001)
            .await
            .expect("sendtoaddress");
        let _ = wallet.generate_to_address(1, &miner_addr).await;
        let funding_txid: Txid = funding_txid_str.parse().expect("txid parse");
        let funding_tx = daemon
            .getrawtransaction_verbose(&funding_txid)
            .await
            .expect("getrawtransaction_verbose");
        let vouts = funding_tx["vout"].as_array().expect("vout array");
        let (vout_idx, vout_obj) = vouts
            .iter()
            .enumerate()
            .find(|(_, v)| v["scriptPubKey"]["address"].as_str() == Some(channel_addr.as_str()))
            .expect("channel vout in funding tx");
        let value_sat =
            (vout_obj["value"].as_f64().expect("value") * 100_000_000.0).round() as u64;
        let funding = VaultUtxo {
            txid: funding_txid,
            vout: vout_idx as u32,
            value_sat,
        };

        let channel =
            BtcAscChannel::new(tree, Network::Regtest, funding).expect("channel new");
        let payouts = vec![
            ChannelOutput {
                address: client_keys.p2tr_address(),
                amount_sat: 55_000,
            },
            ChannelOutput {
                address: sp_keys.p2tr_address(),
                amount_sat: value_sat - 55_000 - 1_000,
            },
        ];
        let secp = Secp256k1::new();
        let client_sk = SecretKey::from_slice(&client_keys.secret[..]).expect("client sk");
        let sp_sk = SecretKey::from_slice(&sp_keys.secret[..]).expect("sp sk");
        let client_kp = Keypair::from_secret_key(&secp, &client_sk);
        let sp_kp = Keypair::from_secret_key(&secp, &sp_sk);
        let recovery_tx = channel
            .adv_vault_recovery_tx(&payouts, 1_000, &client_kp, &sp_kp)
            .expect("adv_vault_recovery_tx");

        let txid = daemon
            .sendrawtransaction(&recovery_tx)
            .await
            .expect("sendrawtransaction adv-vault-recovery");
        eprintln!("[btc-asc-adv] adv-vault recovery broadcast: {txid}");

        let _ = wallet.generate_to_address(1, &miner_addr).await;
        let v = daemon
            .getrawtransaction_verbose(&txid)
            .await
            .expect("getrawtransaction_verbose recovery");
        let recv_vouts = v["vout"].as_array().expect("vout array");
        let client_addr_str = client_keys.p2tr_address().to_string();
        let cv = recv_vouts
            .iter()
            .find(|x| x["scriptPubKey"]["address"].as_str() == Some(client_addr_str.as_str()))
            .expect("client payout vout present");
        let cv_sat = (cv["value"].as_f64().unwrap() * 100_000_000.0).round() as u64;
        assert_eq!(cv_sat, 55_000);
        eprintln!(
            "[btc-asc-adv] client received {cv_sat} sats via script-path leaf B (no Vault sig) ✓"
        );
    }

    /// End-to-end force-close after CSV expiry (script-path leaf A).
    /// Funds the ASC, mines `dispute_window_blocks` confirmations to
    /// satisfy BIP-68 relative locktime, then submits a force-close tx
    /// signed solo by the Vault key. Uses a small CSV (10 blocks) so the
    /// test runs fast.
    #[tokio::test]
    #[ignore]
    async fn regtest_asc_force_close_csv() {
        let rpc_url = std::env::var("A402_BITCOIN_RPC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:18443".to_string());
        let rpc_user =
            std::env::var("A402_BITCOIN_RPC_USER").unwrap_or_else(|_| "a402".to_string());
        let rpc_pass =
            std::env::var("A402_BITCOIN_RPC_PASSWORD").unwrap_or_else(|_| "a402".to_string());
        let daemon = BtcRpcClient::new(&rpc_url, &rpc_user, &rpc_pass);
        let info = daemon.getblockchaininfo().await.expect("getblockchaininfo");
        assert_eq!(info["chain"].as_str().unwrap(), "regtest");

        let wallet_name = "a402-regtest-asc";
        let _ = daemon.createwallet(wallet_name).await;
        let _ = daemon.loadwallet(wallet_name).await;
        let wallet = daemon.clone().with_wallet(wallet_name.to_string());

        let coop_keys = BtcKeys::from_hex(
            "0xb0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0",
            Network::Regtest,
        )
        .unwrap();
        let vault_keys = BtcKeys::from_hex(
            "0xb1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1",
            Network::Regtest,
        )
        .unwrap();
        let client_keys = BtcKeys::from_hex(
            "0xb2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2",
            Network::Regtest,
        )
        .unwrap();
        let sp_keys = BtcKeys::from_hex(
            "0xb3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3",
            Network::Regtest,
        )
        .unwrap();

        // Short CSV so the test is quick.
        let csv_blocks: u32 = 10;
        let tree = AscScriptTree {
            cooperative_xonly: coop_keys.taproot_internal_xonly(),
            vault_xonly: vault_keys.taproot_internal_xonly(),
            client_xonly: client_keys.taproot_internal_xonly(),
            sp_xonly: sp_keys.taproot_internal_xonly(),
            dispute_window_blocks: csv_blocks,
        };
        let asc = tree.build(Network::Regtest).expect("script tree builds");
        let channel_addr = asc.address.to_string();

        let miner_addr = wallet.getnewaddress().await.expect("getnewaddress");
        let _ = wallet.generate_to_address(101, &miner_addr).await;
        let funding_txid_str = wallet
            .sendtoaddress(&channel_addr, 0.001)
            .await
            .expect("sendtoaddress");
        let _ = wallet.generate_to_address(1, &miner_addr).await;
        let funding_txid: Txid = funding_txid_str.parse().expect("txid parse");
        let funding_tx = daemon
            .getrawtransaction_verbose(&funding_txid)
            .await
            .expect("getrawtransaction_verbose");
        let vouts = funding_tx["vout"].as_array().expect("vout array");
        let (vout_idx, vout_obj) = vouts
            .iter()
            .enumerate()
            .find(|(_, v)| v["scriptPubKey"]["address"].as_str() == Some(channel_addr.as_str()))
            .expect("channel vout in funding tx");
        let value_sat =
            (vout_obj["value"].as_f64().expect("value") * 100_000_000.0).round() as u64;
        let funding = VaultUtxo {
            txid: funding_txid,
            vout: vout_idx as u32,
            value_sat,
        };

        let channel =
            BtcAscChannel::new(tree, Network::Regtest, funding).expect("channel new");
        let payouts = vec![
            ChannelOutput {
                address: vault_keys.p2tr_address(),
                amount_sat: value_sat - 1_000,
            },
        ];
        let secp = Secp256k1::new();
        let vault_sk = SecretKey::from_slice(&vault_keys.secret[..]).expect("vault sk");
        let vault_kp = Keypair::from_secret_key(&secp, &vault_sk);
        let close_tx = channel
            .force_close_csv_tx(&payouts, 1_000, &vault_kp)
            .expect("force_close_csv_tx");

        // Before CSV maturity, sendrawtransaction MUST reject the tx
        // (BIP-68 non-final). We've only got 1 confirmation so far.
        let early = daemon.sendrawtransaction(&close_tx).await;
        assert!(
            early.is_err(),
            "force-close should be rejected before CSV maturity, got {early:?}"
        );

        // Mine enough blocks for the CSV to mature.
        let _ = wallet
            .generate_to_address(csv_blocks - 1, &miner_addr)
            .await;
        let txid = daemon
            .sendrawtransaction(&close_tx)
            .await
            .expect("sendrawtransaction force-close after CSV");
        eprintln!("[btc-asc-csv] force-close after CSV broadcast: {txid}");
        let _ = wallet.generate_to_address(1, &miner_addr).await;
    }
}
