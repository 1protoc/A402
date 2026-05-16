use a402_shared::bitcoin::{Address, Network};
use a402_shared::btc_chain::BtcRpcClient;
use a402_shared::btc_tx::{build_settlement_tx, BtcKeys, Payout, VaultUtxo};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::str::FromStr;

use crate::btc_ledger::UtxoKey;
use crate::chain_adapter::{parse_network, ChainKind};
use crate::handlers::AppState;
use crate::wal::WalEntry;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct MultiChainSettlementEntry {
    pub settlement_id: String,
    pub provider_id: String,
    pub network: String,
    pub asset_id: String,
    pub settlement_address: String,
    pub amount: u64,
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct MultiChainBatchReceipt {
    pub chain_kind: ChainKind,
    pub network: String,
    pub tx_id: String,
    pub settlement_ids: Vec<String>,
    pub provider_count: usize,
    pub total_amount: u64,
}

#[derive(Debug, Clone)]
struct AggregatedPayout {
    settlement_address: String,
    amount: u64,
}

pub async fn submit_multichain_batches(
    entries: &[MultiChainSettlementEntry],
    batch_id: u64,
    state: &Arc<AppState>,
) -> Result<Vec<MultiChainBatchReceipt>, String> {
    let mut groups = BTreeMap::<(ChainKind, String, String), Vec<MultiChainSettlementEntry>>::new();
    for entry in entries {
        let descriptor = parse_network(&entry.network)
            .map_err(|error| format!("invalid multichain network {}: {error}", entry.network))?;
        groups
            .entry((descriptor.kind, entry.network.clone(), entry.asset_id.clone()))
            .or_default()
            .push(entry.clone());
    }

    let mut receipts = Vec::new();
    for ((chain_kind, network, asset_id), group) in groups {
        let receipt = match chain_kind {
            ChainKind::Solana => continue,
            ChainKind::Ethereum => submit_ethereum_batch(&network, &asset_id, &group, batch_id)
                .await?,
            ChainKind::Bitcoin => {
                submit_bitcoin_batch(&network, &asset_id, &group, batch_id, state).await?
            }
        };
        receipts.push(receipt);
    }

    Ok(receipts)
}

fn aggregate_payouts(entries: &[MultiChainSettlementEntry]) -> Result<Vec<AggregatedPayout>, String> {
    let mut map = BTreeMap::<String, u64>::new();
    for entry in entries {
        let total = map.entry(entry.settlement_address.clone()).or_insert(0);
        *total = total
            .checked_add(entry.amount)
            .ok_or_else(|| format!("provider amount overflow for {}", entry.settlement_address))?;
    }
    Ok(map
        .into_iter()
        .map(|(settlement_address, amount)| AggregatedPayout {
            settlement_address,
            amount,
        })
        .collect())
}

fn batch_commitment(
    domain: &[u8],
    batch_id: u64,
    network: &str,
    asset_id: &str,
    entries: &[MultiChainSettlementEntry],
) -> [u8; 32] {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.settlement_id.cmp(&b.settlement_id));

    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(batch_id.to_le_bytes());
    hasher.update(network.as_bytes());
    hasher.update([0u8]);
    hasher.update(asset_id.as_bytes());
    for entry in sorted {
        hasher.update(entry.settlement_id.as_bytes());
        hasher.update([0u8]);
        hasher.update(entry.provider_id.as_bytes());
        hasher.update([0u8]);
        hasher.update(entry.settlement_address.as_bytes());
        hasher.update(entry.amount.to_le_bytes());
        hasher.update(entry.timestamp.to_le_bytes());
    }
    hasher.finalize().into()
}

async fn submit_ethereum_batch(
    network: &str,
    asset_id: &str,
    entries: &[MultiChainSettlementEntry],
    batch_id: u64,
) -> Result<MultiChainBatchReceipt, String> {
    let rpc_url = std::env::var("A402_EVM_RPC_URL")
        .map_err(|_| "A402_EVM_RPC_URL is required for Ethereum settlement".to_string())?;
    let contract = std::env::var("A402_EVM_SETTLEMENT_CONTRACT").map_err(|_| {
        "A402_EVM_SETTLEMENT_CONTRACT is required for Ethereum settlement".to_string()
    })?;
    let from = std::env::var("A402_EVM_SUBMITTER")
        .map_err(|_| "A402_EVM_SUBMITTER is required for Ethereum settlement".to_string())?;

    let payouts = aggregate_payouts(entries)?;
    let chunk_hash = batch_commitment(b"A402-EVM-BATCH-V1", batch_id, network, asset_id, entries);
    let audit_root = batch_commitment(b"A402-EVM-AUDIT-ROOT-V1", batch_id, network, asset_id, entries);
    let asset = ethereum_asset_address(asset_id)?;
    let calldata = encode_settle_batch_calldata(batch_id, chunk_hash, &asset, &payouts, audit_root)?;

    let tx = json!({
        "from": from,
        "to": contract,
        "data": calldata,
    });
    let tx_hash: String = json_rpc(&rpc_url, "eth_sendTransaction", json!([tx])).await?;

    Ok(MultiChainBatchReceipt {
        chain_kind: ChainKind::Ethereum,
        network: network.to_string(),
        tx_id: tx_hash,
        settlement_ids: entries.iter().map(|entry| entry.settlement_id.clone()).collect(),
        provider_count: payouts.len(),
        total_amount: entries.iter().map(|entry| entry.amount).sum(),
    })
}

fn ethereum_asset_address(asset_id: &str) -> Result<String, String> {
    if asset_id.eq_ignore_ascii_case("native") || asset_id.eq_ignore_ascii_case("eth") {
        return Ok("0x0000000000000000000000000000000000000000".to_string());
    }
    let stripped = asset_id.strip_prefix("0x").unwrap_or(asset_id);
    if stripped.len() == 40 && stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(format!("0x{}", stripped.to_ascii_lowercase()))
    } else {
        Err(format!("invalid Ethereum asset id: {asset_id}"))
    }
}

fn encode_settle_batch_calldata(
    batch_id: u64,
    chunk_hash: [u8; 32],
    asset: &str,
    payouts: &[AggregatedPayout],
    audit_root: [u8; 32],
) -> Result<String, String> {
    let mut packed = Vec::with_capacity(payouts.len() * 52);
    for payout in payouts {
        let address = decode_eth_address(&payout.settlement_address)?;
        packed.extend_from_slice(&address);
        let mut amount = [0u8; 32];
        amount[24..].copy_from_slice(&payout.amount.to_be_bytes());
        packed.extend_from_slice(&amount);
    }

    let mut encoded = Vec::new();
    encoded.extend_from_slice(&[0x29, 0x37, 0x76, 0xdc]);
    encoded.extend_from_slice(&u256_from_u64(batch_id));
    encoded.extend_from_slice(&chunk_hash);
    encoded.extend_from_slice(&left_pad_20(&decode_eth_address(asset)?));
    encoded.extend_from_slice(&u256_from_u64(160));
    encoded.extend_from_slice(&audit_root);
    encoded.extend_from_slice(&u256_from_u64(packed.len() as u64));
    encoded.extend_from_slice(&packed);
    while encoded.len() % 32 != 0 {
        encoded.push(0);
    }
    Ok(format!("0x{}", hex::encode(encoded)))
}

fn decode_eth_address(address: &str) -> Result<[u8; 20], String> {
    let stripped = address.strip_prefix("0x").unwrap_or(address);
    let bytes = hex::decode(stripped).map_err(|error| format!("invalid hex address: {error}"))?;
    if bytes.len() != 20 {
        return Err(format!("Ethereum address must be 20 bytes: {address}"));
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn left_pad_20(value: &[u8; 20]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[12..].copy_from_slice(value);
    out
}

fn u256_from_u64(value: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[24..].copy_from_slice(&value.to_be_bytes());
    out
}

/// Submits a Bitcoin batch settlement using the in-enclave secp256k1 key.
///
/// Replaces the legacy `walletcreatefundedpsbt + walletprocesspsbt +
/// finalizepsbt` pipeline (which kept private keys in bitcoind's wallet).
/// Now the Vault:
///   1. Loads `sk_BTC` from `A402_BITCOIN_VAULT_PRIV` (KMS-bound in prod).
///   2. Picks UTXOs paid to its own P2WPKH address via `listunspent`.
///   3. Builds the tx + signs every input in-process via
///      `a402_shared::btc_tx::build_settlement_tx`.
///   4. Hands the raw signed tx to bitcoind via `sendrawtransaction`.
/// bitcoind never sees the vault private key.
async fn submit_bitcoin_batch(
    network: &str,
    asset_id: &str,
    entries: &[MultiChainSettlementEntry],
    batch_id: u64,
    state: &Arc<AppState>,
) -> Result<MultiChainBatchReceipt, String> {
    if !asset_id.eq_ignore_ascii_case("btc") && !asset_id.eq_ignore_ascii_case("native") {
        return Err(format!("unsupported Bitcoin asset id: {asset_id}"));
    }

    let net = parse_bitcoin_network(network)?;
    let rpc_url = std::env::var("A402_BITCOIN_RPC_URL")
        .map_err(|_| "A402_BITCOIN_RPC_URL is required for Bitcoin settlement".to_string())?;
    let rpc_user = std::env::var("A402_BITCOIN_RPC_USER").unwrap_or_default();
    let rpc_password = std::env::var("A402_BITCOIN_RPC_PASSWORD").unwrap_or_default();
    let vault_priv = std::env::var("A402_BITCOIN_VAULT_PRIV").map_err(|_| {
        "A402_BITCOIN_VAULT_PRIV (32-byte hex secp256k1 secret) is required for Bitcoin settlement"
            .to_string()
    })?;
    let fee_rate_sat_per_vb = std::env::var("A402_BITCOIN_FEE_RATE")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(2.0)
        .max(1.0);

    let keys = BtcKeys::from_hex(&vault_priv, net)
        .map_err(|e| format!("A402_BITCOIN_VAULT_PRIV invalid: {e}"))?;
    let vault_addr = keys.p2wpkh_address();
    let vault_addr_str = vault_addr.to_string();

    // Aggregate per-provider then parse each settlement address against the
    // network we resolved from `network`. Refuse mismatched addresses early.
    let aggregated = aggregate_payouts(entries)?;
    let mut payouts: Vec<Payout> = Vec::with_capacity(aggregated.len());
    for ap in &aggregated {
        let addr_unchecked = Address::from_str(&ap.settlement_address)
            .map_err(|e| format!("invalid provider address {}: {e}", ap.settlement_address))?;
        let addr = addr_unchecked.require_network(net).map_err(|e| {
            format!("address {} not on {network}: {e}", ap.settlement_address)
        })?;
        payouts.push(Payout {
            address: addr,
            amount_sat: ap.amount,
        });
    }
    let total_out_sat: u64 = payouts.iter().map(|p| p.amount_sat).sum();

    let rpc = BtcRpcClient::new(rpc_url, rpc_user, rpc_password);
    let _ = vault_addr_str; // silence unused: we no longer query bitcoind for vault funds

    let est_vbytes = estimate_vbytes(payouts.len(), /* n_inputs */ 1);
    let mut fee_sat = (fee_rate_sat_per_vb * est_vbytes as f64).ceil() as u64;
    fee_sat = fee_sat.max(500); // cheap floor — testnets occasionally need it
    let needed = total_out_sat + fee_sat;

    // Slice 3C: source the input UTXO from the Vault's own WAL-tracked
    // ledger instead of trusting bitcoind's `listunspent`. Slice 3B's
    // deposit detector keeps the ledger fed.
    let utxo = state
        .btc_ledger
        .read()
        .await
        .pick_single(needed)
        .map_err(|e| format!("btc_ledger.pick_single: {e}"))?;
    let change_sat = utxo.value_sat - total_out_sat - fee_sat;
    let commit_hash =
        batch_commitment(b"A402-BTC-BATCH-V1", batch_id, network, asset_id, entries);
    let tx = build_settlement_tx(&commit_hash, &payouts, &[utxo.clone()], &vault_addr, fee_sat, &keys)
        .map_err(|e| format!("build_settlement_tx: {e}"))?;

    let txid = rpc
        .sendrawtransaction(&tx)
        .await
        .map_err(|e| format!("sendrawtransaction: {e}"))?;

    // After the broadcast succeeds, durably book-keep the spent input and
    // the produced change output so the ledger can survive an enclave
    // restart. WAL-first, then in-memory ledger.
    let _persist_guard = state.persistence_lock.lock().await;
    state
        .wal
        .append(WalEntry::BtcUtxoSpent {
            txid: utxo.txid.to_string(),
            vout: utxo.vout,
            in_settlement_txid: txid.to_string(),
        })
        .await
        .map_err(|e| format!("wal append BtcUtxoSpent: {e}"))?;
    state
        .btc_ledger
        .write()
        .await
        .spend(&UtxoKey::new(utxo.txid, utxo.vout))
        .map_err(|e| format!("btc_ledger.spend: {e}"))?;
    // `build_settlement_tx` emits a change output iff change_sat >= 1_000.
    // Output layout is `[0]=OP_RETURN | [1..1+N]=payouts | [N+1]=change?`.
    if change_sat >= 1_000 {
        let change_vout = (1 + payouts.len()) as u32;
        state
            .wal
            .append(WalEntry::BtcChangeCreated {
                txid: txid.to_string(),
                vout: change_vout,
                value_sat: change_sat,
            })
            .await
            .map_err(|e| format!("wal append BtcChangeCreated: {e}"))?;
        state
            .btc_ledger
            .write()
            .await
            .add(UtxoKey::new(txid, change_vout), change_sat)
            .map_err(|e| format!("btc_ledger.add(change): {e}"))?;
    }
    drop(_persist_guard);

    Ok(MultiChainBatchReceipt {
        chain_kind: ChainKind::Bitcoin,
        network: network.to_string(),
        tx_id: txid.to_string(),
        settlement_ids: entries.iter().map(|entry| entry.settlement_id.clone()).collect(),
        provider_count: payouts.len(),
        total_amount: entries.iter().map(|entry| entry.amount).sum(),
    })
}

fn parse_bitcoin_network(network: &str) -> Result<Network, String> {
    let tail = network
        .strip_prefix("bitcoin:")
        .ok_or_else(|| format!("expected 'bitcoin:<net>', got {network}"))?;
    match tail {
        "mainnet" => Ok(Network::Bitcoin),
        "testnet" => Ok(Network::Testnet),
        "signet" => Ok(Network::Signet),
        "regtest" => Ok(Network::Regtest),
        other => Err(format!("unknown Bitcoin network: {other}")),
    }
}

/// Pick the smallest UTXO covering `needed`. If none does, error out — the
/// slice-2 path only spends one UTXO per batch. Multi-UTXO selection ships
/// with slice 3 (WAL-tracked UTXO set).
fn pick_utxo(unspent: &[Value], needed: u64) -> Result<VaultUtxo, String> {
    let mut candidates: Vec<VaultUtxo> = Vec::new();
    for entry in unspent {
        let txid_str = entry
            .get("txid")
            .and_then(Value::as_str)
            .ok_or_else(|| "listunspent entry missing txid".to_string())?;
        let vout = entry
            .get("vout")
            .and_then(Value::as_u64)
            .ok_or_else(|| "listunspent entry missing vout".to_string())? as u32;
        let amount_btc = entry
            .get("amount")
            .and_then(Value::as_f64)
            .ok_or_else(|| "listunspent entry missing amount".to_string())?;
        let value_sat = (amount_btc * 100_000_000.0).round() as u64;
        let txid = txid_str
            .parse::<a402_shared::bitcoin::Txid>()
            .map_err(|e| format!("listunspent txid parse {txid_str}: {e}"))?;
        candidates.push(VaultUtxo {
            txid,
            vout,
            value_sat,
        });
    }
    candidates.sort_by_key(|u| u.value_sat);
    candidates
        .into_iter()
        .find(|u| u.value_sat >= needed)
        .ok_or_else(|| {
            format!(
                "no single UTXO covers {needed} sats — multi-UTXO selection lands in slice 3"
            )
        })
}

/// Approximate vbyte count for a Vault settlement transaction:
///   `10` (overhead) + `68 × n_inputs` (P2WPKH) + `12` (OP_RETURN 32B) +
///   `31 × (n_payouts + 1 change)` P2WPKH outputs.
fn estimate_vbytes(n_payouts: usize, n_inputs: usize) -> u64 {
    let inputs = 68u64 * n_inputs as u64;
    let outputs = 31u64 * (n_payouts as u64 + 1); // payouts + change
    10 + inputs + 12 + outputs
}

fn sats_to_btc(sats: u64) -> f64 {
    sats as f64 / 100_000_000.0
}

async fn json_rpc<T: for<'de> Deserialize<'de>>(
    url: &str,
    method: &str,
    params: Value,
) -> Result<T, String> {
    let response = reqwest::Client::new()
        .post(url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "a402",
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .map_err(|error| format!("{method} request failed: {error}"))?;
    let body: JsonRpcResponse<T> = response
        .json()
        .await
        .map_err(|error| format!("{method} response decode failed: {error}"))?;
    if let Some(error) = body.error {
        return Err(format!("{method} JSON-RPC error: {error}"));
    }
    body.result
        .ok_or_else(|| format!("{method} JSON-RPC response missing result"))
}

async fn bitcoin_rpc(
    url: &str,
    user: Option<&str>,
    password: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let mut request = reqwest::Client::new()
        .post(url)
        .json(&json!({
            "jsonrpc": "1.0",
            "id": "a402",
            "method": method,
            "params": params,
        }));
    if let (Some(user), Some(password)) = (user, password) {
        request = request.header(
            "Authorization",
            format!("Basic {}", BASE64.encode(format!("{user}:{password}"))),
        );
    }
    let body: JsonRpcResponse<Value> = request
        .send()
        .await
        .map_err(|error| format!("{method} request failed: {error}"))?
        .json()
        .await
        .map_err(|error| format!("{method} response decode failed: {error}"))?;
    if let Some(error) = body.error {
        return Err(format!("{method} JSON-RPC error: {error}"));
    }
    body.result
        .ok_or_else(|| format!("{method} JSON-RPC response missing result"))
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ethereum_calldata_packs_provider_amount_pairs() {
        let calldata = encode_settle_batch_calldata(
            7,
            [1u8; 32],
            "0x0000000000000000000000000000000000000000",
            &[AggregatedPayout {
                settlement_address: "0x1111111111111111111111111111111111111111".to_string(),
                amount: 42,
            }],
            [2u8; 32],
        )
        .unwrap();
        assert!(calldata.starts_with("0x"));
        assert!(calldata.contains("1111111111111111111111111111111111111111"));
        assert!(calldata.ends_with("000000000000000000000000"));
    }

    #[test]
    fn parses_bitcoin_network_strings() {
        assert!(matches!(
            parse_bitcoin_network("bitcoin:mainnet"),
            Ok(Network::Bitcoin)
        ));
        assert!(matches!(
            parse_bitcoin_network("bitcoin:testnet"),
            Ok(Network::Testnet)
        ));
        assert!(matches!(
            parse_bitcoin_network("bitcoin:signet"),
            Ok(Network::Signet)
        ));
        assert!(matches!(
            parse_bitcoin_network("bitcoin:regtest"),
            Ok(Network::Regtest)
        ));
        // Bad prefix and bad tail both rejected.
        assert!(parse_bitcoin_network("eth:mainnet").is_err());
        assert!(parse_bitcoin_network("bitcoin:dogenet").is_err());
    }

    #[test]
    fn estimates_vbytes_within_5_pct_of_reference() {
        // Reference (computed by hand from BIP-141 weight units, see
        // vbyte estimate based on standard BIP-141 weight units:
        //   1 input, 0 payouts (only OP_RETURN + change) → 10 + 68 + 12 + 31 = 121
        //   1 input, 1 payout                            → 10 + 68 + 12 + 62 = 152
        //   1 input, 4 payouts                           → 10 + 68 + 12 + 155 = 245
        //   2 inputs, 4 payouts                          → 10 + 136 + 12 + 155 = 313
        assert_eq!(estimate_vbytes(0, 1), 121);
        assert_eq!(estimate_vbytes(1, 1), 152);
        assert_eq!(estimate_vbytes(4, 1), 245);
        assert_eq!(estimate_vbytes(4, 2), 313);
    }

    #[test]
    fn picks_smallest_utxo_covering_amount() {
        // listunspent-shaped entries: txid (any 32-byte hex), vout, amount BTC.
        let unspent = vec![
            json!({
                "txid": "0000000000000000000000000000000000000000000000000000000000000001",
                "vout": 0,
                "amount": 0.001,    // 100_000 sats
            }),
            json!({
                "txid": "0000000000000000000000000000000000000000000000000000000000000002",
                "vout": 0,
                "amount": 0.0005,   // 50_000 sats
            }),
            json!({
                "txid": "0000000000000000000000000000000000000000000000000000000000000003",
                "vout": 0,
                "amount": 0.002,    // 200_000 sats
            }),
        ];
        // Need 60_000 sats → smallest covering UTXO is the 100_000-sat one.
        let picked = pick_utxo(&unspent, 60_000).unwrap();
        assert_eq!(picked.value_sat, 100_000);
        // Need 300_000 sats → no single UTXO covers, must error out (slice 3
        // territory: multi-UTXO selection).
        assert!(pick_utxo(&unspent, 300_000).is_err());
    }

    #[test]
    fn bitcoin_commitment_is_order_independent() {
        let a = MultiChainSettlementEntry {
            settlement_id: "set_b".to_string(),
            provider_id: "provider".to_string(),
            network: "bitcoin:regtest".to_string(),
            asset_id: "btc".to_string(),
            settlement_address: "bcrt1qexampleaddress000000000000000000000".to_string(),
            amount: 1,
            timestamp: 2,
        };
        let mut b = a.clone();
        b.settlement_id = "set_a".to_string();
        let first = batch_commitment(
            b"A402-BTC-BATCH-V1",
            1,
            "bitcoin:regtest",
            "btc",
            &[a.clone(), b.clone()],
        );
        let second = batch_commitment(
            b"A402-BTC-BATCH-V1",
            1,
            "bitcoin:regtest",
            "btc",
            &[b, a],
        );
        assert_eq!(first, second);
    }
}
