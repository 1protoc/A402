use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::chain_adapter::{parse_network, ChainKind};

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
            ChainKind::Bitcoin => submit_bitcoin_batch(&network, &asset_id, &group, batch_id)
                .await?,
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

async fn submit_bitcoin_batch(
    network: &str,
    asset_id: &str,
    entries: &[MultiChainSettlementEntry],
    batch_id: u64,
) -> Result<MultiChainBatchReceipt, String> {
    if !asset_id.eq_ignore_ascii_case("btc") && !asset_id.eq_ignore_ascii_case("native") {
        return Err(format!("unsupported Bitcoin asset id: {asset_id}"));
    }

    let rpc_url = std::env::var("A402_BITCOIN_RPC_URL")
        .map_err(|_| "A402_BITCOIN_RPC_URL is required for Bitcoin settlement".to_string())?;
    let rpc_user = std::env::var("A402_BITCOIN_RPC_USER").ok();
    let rpc_password = std::env::var("A402_BITCOIN_RPC_PASSWORD").ok();
    let fee_rate = std::env::var("A402_BITCOIN_FEE_RATE")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(2.0);

    let payouts = aggregate_payouts(entries)?;
    let commitment = batch_commitment(b"A402-BTC-BATCH-V1", batch_id, network, asset_id, entries);
    let mut outputs = Vec::with_capacity(payouts.len() + 1);
    outputs.push(json!({ "data": hex::encode(commitment) }));
    for payout in &payouts {
        outputs.push(json!({
            payout.settlement_address.clone(): sats_to_btc(payout.amount)
        }));
    }

    let options = json!({
        "fee_rate": fee_rate,
        "subtractFeeFromOutputs": [],
        "replaceable": true,
    });
    let psbt: String = bitcoin_rpc(
        &rpc_url,
        rpc_user.as_deref(),
        rpc_password.as_deref(),
        "walletcreatefundedpsbt",
        json!([[], outputs, 0, options, true]),
    )
    .await?
    .get("psbt")
    .and_then(|value| value.as_str())
    .ok_or_else(|| "walletcreatefundedpsbt response missing psbt".to_string())?
    .to_string();

    let processed: String = bitcoin_rpc(
        &rpc_url,
        rpc_user.as_deref(),
        rpc_password.as_deref(),
        "walletprocesspsbt",
        json!([psbt]),
    )
    .await?
    .get("psbt")
    .and_then(|value| value.as_str())
    .ok_or_else(|| "walletprocesspsbt response missing psbt".to_string())?
    .to_string();

    let finalized = bitcoin_rpc(
        &rpc_url,
        rpc_user.as_deref(),
        rpc_password.as_deref(),
        "finalizepsbt",
        json!([processed]),
    )
    .await?;
    if finalized.get("complete").and_then(|value| value.as_bool()) != Some(true) {
        return Err("Bitcoin PSBT was not fully signed by enclave wallet".to_string());
    }
    let hex = finalized
        .get("hex")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "finalizepsbt response missing hex".to_string())?;

    let txid: String = bitcoin_rpc(
        &rpc_url,
        rpc_user.as_deref(),
        rpc_password.as_deref(),
        "sendrawtransaction",
        json!([hex]),
    )
    .await?
    .as_str()
    .ok_or_else(|| "sendrawtransaction response missing txid".to_string())?
    .to_string();

    Ok(MultiChainBatchReceipt {
        chain_kind: ChainKind::Bitcoin,
        network: network.to_string(),
        tx_id: txid,
        settlement_ids: entries.iter().map(|entry| entry.settlement_id.clone()).collect(),
        provider_count: payouts.len(),
        total_amount: entries.iter().map(|entry| entry.amount).sum(),
    })
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
