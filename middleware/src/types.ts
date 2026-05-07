import type { Request, Response, NextFunction } from "express";

/** Provider payment configuration */
export interface A402ProviderConfig {
  /** Base URL of the enclave facilitator */
  facilitatorUrl: string;
  /** Seller/provider identifier. Open sellers can use the derived ID. */
  providerId: string;
  /** Provider auth mode used against the facilitator */
  authMode?: "none" | "bearer" | "api-key" | "mtls";
  /** Optional advanced facilitator auth secret; not used by the default open seller flow. */
  apiKey?: string;
  /** Optional mTLS client certificate configuration */
  mtls?: {
    certPath: string;
    keyPath: string;
    caPath?: string;
    serverName?: string;
  };
  /** Provider settlement address. Solana uses a token account; EVM/BTC use chain-native addresses. */
  payTo: string;
  /** CAIP-2 network identifier */
  network: string;
  /** Chain asset id: SPL mint, ERC-20 address, or native asset id such as eth/btc/native. */
  assetMint: string;
  /** Asset decimals */
  assetDecimals: number;
  /** Asset symbol */
  assetSymbol: string;
  /** Asset kind used in the x402 payment details envelope. */
  assetKind?: A402AssetKind;
  /** VaultConfig PDA address */
  vaultConfig: string;
  /** Vault signer pubkey */
  vaultSigner: string;
  /** Attestation policy hash */
  attestationPolicyHash: string;
}

/** Inputs required to produce an ASC delivery artifact */
export interface AscDeliveryInput {
  channelId: string;
  requestId: string;
  amount: string | number;
  requestHash: string;
  result: Uint8Array | Buffer | string;
  providerSecretKey?: Uint8Array | string;
  adaptorSecret?: Uint8Array | string;
}

/** Provider-generated ASC delivery payload */
export interface AscDeliveryArtifact {
  adaptorPoint: string;
  preSigRPrime: string;
  preSigSPrime: string;
  encryptedResult: string;
  resultHash: string;
  providerPubkey: string;
  adaptorSecret: string;
}

export interface AscClaimVoucher {
  message: string;
  signature: string;
  issuedAt: number;
  channelIdHash: string;
  requestIdHash: string;
}

/** Facilitator /v1/channel/deliver response */
export interface AscDeliverResponse {
  ok: boolean;
  channelId: string;
  status: string;
  claimVoucher: AscClaimVoucher;
}

/** Pricing function: given a request, return the price in atomic units (or null if free) */
export type PricingFn = (req: Request) => string | null;

/** Options for the a402 middleware */
export interface A402MiddlewareOptions {
  config: A402ProviderConfig;
  /** Return the price for this request, or null if no payment required */
  pricing: PricingFn;
}

/** Extended request with payment context */
export interface A402Request extends Request {
  rawBody?: Buffer | string;
  a402?: {
    verificationId: string;
    paymentId: string;
    amount: string;
    providerId: string;
  };
}

export interface SettlementStatusResponse {
  ok: boolean;
  settlementId: string;
  verificationId: string;
  providerId: string;
  status: string;
  batchId: number | null;
  txSignature: string | null;
}

export type A402WireScheme =
  | "a402-svm-v1"
  | "a402-v1"
  | "a402-evm-v1"
  | "a402-btc-v1";

export type A402Scheme = "exact" | "a402-exact" | A402WireScheme;

export type A402AssetKind = "spl-token" | "erc20" | "native" | "btc";

export interface A402RouteAccept {
  /** Developer-facing scheme. "exact" maps to a chain-aware A402 wire scheme. */
  scheme?: A402Scheme;
  /** Human-readable price such as "$0.001", or atomic token units such as "1000". */
  price: string | number;
  /** CAIP-2 network identifier. */
  network: string;
  /** Optional provider identifier. If omitted, A402 derives one from network, asset mint, and payTo. */
  providerId?: string;
  /** Seller wallet owner. On Solana this derives an ATA; on EVM/BTC it is used directly as payTo. */
  sellerWallet?: string;
  /** Provider settlement address. Advanced override; normally derive this from sellerWallet on Solana. */
  payTo?: string;
  /** Optional per-route asset override. Defaults to the facilitator client's asset config. */
  asset?: {
    kind?: A402AssetKind;
    mint: string;
    decimals?: number;
    symbol?: string;
  };
  assetMint?: string;
  assetDecimals?: number;
  assetSymbol?: string;
}

export interface A402RouteConfig {
  accepts: A402RouteAccept[];
  description?: string;
  mimeType?: string;
}

export type A402Routes = Record<string, A402RouteConfig>;

export interface A402FacilitatorClientOptions {
  /** Base URL of the A402 facilitator / Nitro enclave ingress. */
  url: string;
  /** Optional advanced facilitator auth secret; omit for the default open seller flow. */
  providerApiKey?: string;
  authMode?: A402ProviderConfig["authMode"];
  mtls?: A402ProviderConfig["mtls"];
  /** Optional cached attestation fields. If omitted, middleware fetches /v1/attestation. */
  vaultConfig?: string;
  vaultSigner?: string;
  attestationPolicyHash?: string;
  /** Default settlement asset used by route accepts that omit asset fields. */
  assetMint?: string;
  assetDecimals?: number;
  assetSymbol?: string;
  assetKind?: A402AssetKind;
}
