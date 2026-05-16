//! Thin reqwest wrapper around the Service Provider's `/v1/sp/*` routes.
//!
//! The on-the-wire JSON schema is the same one [service_provider/src/handlers.rs]
//! defines (camelCase). We deserialize it directly here rather than reaching
//! across crates, because the SP request/response types use raw 33-byte point
//! encodings and Vec<u8> ciphertexts that don't fit serde derives.
//!
//! All methods return `Result<T, ClientError>`. We turn non-2xx HTTP
//! responses into `ClientError::SpResponse { status, message }` so callers
//! get the SP's error envelope verbatim.

use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

use crate::error::ClientError;

pub struct SpHttpClient {
    base_url: String,
    http: HttpClient,
}

impl SpHttpClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self, ClientError> {
        // `no_proxy()` is the same fix we applied in shared/evm_chain.rs to
        // dodge macOS Privoxy intercepting localhost in dev.
        let http = HttpClient::builder()
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| ClientError::Http(e.to_string()))?;
        Ok(Self {
            base_url: base_url.into(),
            http,
        })
    }

    pub async fn get_info(&self) -> Result<InfoResponse, ClientError> {
        let url = format!("{}/v1/sp/info", self.base_url.trim_end_matches('/'));
        let resp = self.http.get(&url).send().await?;
        self.decode(resp).await
    }

    pub async fn register_vault(
        &self,
        uid: &str,
        vault_eoa: &str,
        code_hash: Option<&str>,
    ) -> Result<RegisterVaultResponse, ClientError> {
        let body = json!({
            "uid": uid,
            "vaultEoa": vault_eoa,
            "codeHash": code_hash,
        });
        self.post("/v1/sp/register-vault", &body).await
    }

    pub async fn register(
        &self,
        cid: &str,
        buyer: &str,
        total_deposit: u128,
        vault_uid: &str,
    ) -> Result<RegisterResponse, ClientError> {
        let body = json!({
            "cid": cid,
            "buyer": buyer,
            "totalDeposit": total_deposit.to_string(),
            "vaultUid": vault_uid,
        });
        self.post("/v1/sp/register", &body).await
    }

    pub async fn request(&self, cid: &str) -> Result<AtomicResp, ClientError> {
        let body = json!({ "cid": cid, "req": serde_json::Value::Null });
        self.post("/v1/sp/request", &body).await
    }

    pub async fn finalize(
        &self,
        cid: &str,
        version: u64,
        sig_c: &[u8; 65],
    ) -> Result<FinalizeResp, ClientError> {
        let body = json!({
            "cid": cid,
            "version": version,
            "sigC": format!("0x{}", hex::encode(sig_c)),
        });
        self.post("/v1/sp/finalize", &body).await
    }

    pub async fn force_close(
        &self,
        cid: &str,
        vault_sig_u: &[u8; 65],
    ) -> Result<ForceCloseResp, ClientError> {
        let body = json!({
            "cid": cid,
            "vaultSigU": format!("0x{}", hex::encode(vault_sig_u)),
        });
        self.post("/v1/sp/force-close", &body).await
    }

    async fn post<B: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<R, ClientError> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let resp = self.http.post(&url).json(body).send().await?;
        self.decode(resp).await
    }

    async fn decode<R: for<'de> Deserialize<'de>>(
        &self,
        resp: reqwest::Response,
    ) -> Result<R, ClientError> {
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            // Try to surface the SP's {"error": "..."} envelope.
            let message = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str().map(str::to_string)))
                .unwrap_or_else(|| String::from_utf8_lossy(&bytes).to_string());
            return Err(ClientError::SpResponse {
                status: status.as_u16(),
                message,
            });
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| ClientError::InvalidSpResponse(e.to_string()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoResponse {
    pub address: String,
    pub schnorr_px: String,
    pub asc_manager: String,
    pub vault_addr: String,
    pub registered_vaults: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterVaultResponse {
    pub ok: bool,
    pub uid: String,
    pub vault_eoa: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResponse {
    pub ok: bool,
    pub cid: String,
    pub seller_address: String,
    pub seller_schnorr_px: String,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeResp {
    pub cid: String,
    pub version: u64,
    pub t: String,
    pub sig_s: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForceCloseResp {
    pub cid: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub schnorr_px: String,
    pub schnorr_e: String,
    pub schnorr_s: String,
}
