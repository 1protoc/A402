use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("invalid hex: {0}")]
    Hex(String),
    #[error("invalid key material: {0}")]
    Key(String),
    #[error("invalid signature: {0}")]
    Signature(String),
    #[error("schnorr pre-signature failed p_verify")]
    PreSigInvalid,
    #[error("adapted signature failed verify_full")]
    FullSigInvalid,
    #[error("EncRes decryption failed (tag mismatch or wrong t)")]
    DecryptFailed,
    #[error("T point reconstructed from t did not match SP-supplied bigT")]
    WitnessMismatch,
    #[error("on-chain state mismatch: {0}")]
    OnChain(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("json error: {0}")]
    Json(String),
    #[error("SP responded with error: {status} — {message}")]
    SpResponse { status: u16, message: String },
    #[error("invalid SP response: {0}")]
    InvalidSpResponse(String),
}

impl From<reqwest::Error> for ClientError {
    fn from(value: reqwest::Error) -> Self {
        ClientError::Http(value.to_string())
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(value: serde_json::Error) -> Self {
        ClientError::Json(value.to_string())
    }
}
