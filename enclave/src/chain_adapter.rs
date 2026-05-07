use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChainKind {
    Solana,
    Ethereum,
    Bitcoin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainDescriptor {
    pub kind: ChainKind,
    pub network: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementTarget {
    pub network: String,
    pub asset_id: String,
    pub settlement_address: String,
    pub solana_settlement_token_account: Option<Pubkey>,
    pub solana_asset_mint: Option<Pubkey>,
}

impl SettlementTarget {
    pub fn parse(network: &str, asset_id: &str, settlement_address: &str) -> Result<Self, String> {
        let descriptor = parse_network(network)?;
        match descriptor.kind {
            ChainKind::Solana => {
                let settlement_token_account = Pubkey::from_str(settlement_address)
                    .map_err(|_| "Solana settlement address must be a valid pubkey".to_string())?;
                let asset_mint = Pubkey::from_str(asset_id)
                    .map_err(|_| "Solana asset id must be a valid mint pubkey".to_string())?;
                Ok(Self {
                    network: network.to_string(),
                    asset_id: asset_id.to_string(),
                    settlement_address: settlement_address.to_string(),
                    solana_settlement_token_account: Some(settlement_token_account),
                    solana_asset_mint: Some(asset_mint),
                })
            }
            ChainKind::Ethereum => {
                validate_ethereum_address(settlement_address)?;
                validate_ethereum_asset(asset_id)?;
                Ok(Self {
                    network: network.to_string(),
                    asset_id: asset_id.to_string(),
                    settlement_address: normalize_eth_address(settlement_address),
                    solana_settlement_token_account: None,
                    solana_asset_mint: None,
                })
            }
            ChainKind::Bitcoin => {
                validate_bitcoin_address(settlement_address)?;
                validate_bitcoin_asset(asset_id)?;
                Ok(Self {
                    network: network.to_string(),
                    asset_id: asset_id.to_string(),
                    settlement_address: settlement_address.to_string(),
                    solana_settlement_token_account: None,
                    solana_asset_mint: None,
                })
            }
        }
    }
}

pub fn parse_network(network: &str) -> Result<ChainDescriptor, String> {
    let kind = if network.starts_with("solana:") || network == "solana-localnet" {
        ChainKind::Solana
    } else if network.starts_with("eip155:") || network.starts_with("ethereum:") {
        ChainKind::Ethereum
    } else if network.starts_with("bip122:") || network.starts_with("bitcoin:") {
        ChainKind::Bitcoin
    } else {
        return Err(format!("unsupported network: {network}"));
    };

    Ok(ChainDescriptor {
        kind,
        network: network.to_string(),
    })
}

pub fn is_solana_network(network: &str) -> bool {
    parse_network(network)
        .map(|descriptor| descriptor.kind == ChainKind::Solana)
        .unwrap_or(false)
}

fn normalize_eth_address(address: &str) -> String {
    format!("0x{}", &address.trim_start_matches("0x").to_ascii_lowercase())
}

fn validate_ethereum_address(address: &str) -> Result<(), String> {
    let stripped = address.strip_prefix("0x").unwrap_or(address);
    if stripped.len() == 40 && stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("Ethereum settlement address must be a 20-byte hex address".to_string())
    }
}

fn validate_ethereum_asset(asset_id: &str) -> Result<(), String> {
    if asset_id.eq_ignore_ascii_case("native")
        || asset_id.eq_ignore_ascii_case("eth")
        || validate_ethereum_address(asset_id).is_ok()
    {
        Ok(())
    } else {
        Err("Ethereum asset id must be native/eth or an ERC-20 address".to_string())
    }
}

fn validate_bitcoin_address(address: &str) -> Result<(), String> {
    let lower = address.to_ascii_lowercase();
    let plausible = lower.starts_with("bc1")
        || lower.starts_with("tb1")
        || lower.starts_with("bcrt1")
        || lower.starts_with('1')
        || lower.starts_with('3')
        || lower.starts_with('m')
        || lower.starts_with('n')
        || lower.starts_with('2');
    if plausible && address.len() >= 14 && address.len() <= 90 {
        Ok(())
    } else {
        Err("Bitcoin settlement address must look like a mainnet/testnet/regtest address".to_string())
    }
}

fn validate_bitcoin_asset(asset_id: &str) -> Result<(), String> {
    if asset_id.eq_ignore_ascii_case("btc") || asset_id.eq_ignore_ascii_case("native") {
        Ok(())
    } else {
        Err("Bitcoin asset id must be btc/native".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_solana_target() {
        let pay_to = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let target =
            SettlementTarget::parse("solana:localnet", &mint.to_string(), &pay_to.to_string())
                .unwrap();
        assert_eq!(target.solana_settlement_token_account, Some(pay_to));
        assert_eq!(target.solana_asset_mint, Some(mint));
    }

    #[test]
    fn parses_ethereum_target() {
        let target = SettlementTarget::parse(
            "eip155:1",
            "0xA0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            "0x1111111111111111111111111111111111111111",
        )
        .unwrap();
        assert_eq!(target.solana_settlement_token_account, None);
        assert_eq!(
            target.settlement_address,
            "0x1111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn parses_bitcoin_target() {
        let target =
            SettlementTarget::parse("bitcoin:regtest", "btc", "bcrt1qtestaddress0000000000000000")
                .unwrap();
        assert_eq!(target.solana_asset_mint, None);
    }

    #[test]
    fn rejects_unknown_network() {
        assert!(SettlementTarget::parse("cosmos:foo", "atom", "addr").is_err());
    }
}
