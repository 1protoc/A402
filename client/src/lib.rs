//! A402 Client (C) — the buyer-side library.
//!
//! Per , Client is the only role that does **not** run inside a
//! TEE. It holds its own secp256k1 keypair, talks to the Service Provider
//! (S) over HTTP (via the Vault (U), or directly during the demo path), and
//! locally verifies every commitment it gets back: the Schnorr adaptor
//! pre-signature `σ̂_S`, the encrypted result
//! `EncRes`, and finally the adapted full signature `σ_S` together with the
//! witness scalar `t` revealed at finalize time.
//!
//! Layout:
//!   - [`keys`]   — `ClientKeys` (ECDSA SigningKey + 20-byte address)
//!   - [`sigs`]   — Ethereum-prefix ECDSA signing / recovery (`σ_C`)
//!   - [`sp_http`] — thin reqwest wrapper around the SP's `/v1/sp/*` routes
//!   - [`atomic`] — parsing + cryptographic verification of the flow
//!
//! The crate is intentionally library-only. Higher-level CLIs / demos
//! compose these primitives.

pub mod atomic;
pub mod error;
pub mod keys;
pub mod sigs;
pub mod sp_http;

pub use error::ClientError;
pub use keys::ClientKeys;
pub use sp_http::SpHttpClient;
