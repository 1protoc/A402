import type { Address, MessagePartialSigner } from "@solana/kit";

export type A402WireScheme =
  | "a402-svm-v1"
  | "a402-v1"
  | "a402-evm-v1"
  | "a402-btc-v1";

export type A402AssetKind = "spl-token" | "erc20" | "native" | "btc";

export type A402PublicKeyLike =
  | string
  | Uint8Array
  | {
      toBase58(): string;
      toBuffer?(): Uint8Array;
      toBytes?(): Uint8Array;
    };

/** Payment details from a 402 response */
export interface PaymentDetails {
  scheme: A402WireScheme;
  network: string;
  amount: string;
  asset: {
    kind: A402AssetKind | string;
    mint: string;
    decimals: number;
    symbol: string;
  };
  payTo: string;
  providerId: string;
  facilitatorUrl: string;
  vault: {
    config: string;
    signer: string;
    attestationPolicyHash: string;
  };
  paymentDetailsId: string;
  verifyWindowSec: number;
  maxSettlementDelaySec: number;
  privacyMode: string;
}

/** 402 Payment Required response body */
export interface PaymentRequiredResponse {
  accepts: PaymentDetails[];
  error?: string;
  facilitatorError?: string;
  message?: string;
}

/** Payment signature payload sent by client */
export interface PaymentPayload {
  version: number;
  scheme: string;
  paymentId: string;
  client: string;
  vault: string;
  providerId: string;
  payTo: string;
  network: string;
  assetMint: string;
  amount: string;
  requestHash: string;
  paymentDetailsHash: string;
  expiresAt: string;
  nonce: string;
  clientSig: string;
}

/** Facilitator /v1/verify response */
export interface VerifyResponse {
  ok: boolean;
  verificationId: string;
  reservationId: string;
  reservationExpiresAt: string;
  providerId: string;
  amount: string;
  verificationReceipt: string;
}

export interface VerificationReceiptEnvelope {
  verificationId: string;
  reservationId: string;
  paymentId: string;
  client: string;
  providerId: string;
  amount: string;
  requestHash: string;
  paymentDetailsHash: string;
  reservationExpiresAt: string;
  vaultConfig: string;
  signature: string;
  message: string;
}

/** Facilitator /v1/settle response */
export interface SettleResponse {
  ok: boolean;
  settlementId: string;
  offchainSettledAt: string;
  providerCreditAmount: string;
  batchId: number | null;
  participantReceipt: string;
}

/** Facilitator /v1/attestation response */
export type A402TeePlatform = "local-dev" | "aws-nitro" | "amd-sev-snp";

export interface AttestationResponse {
  vaultConfig: string;
  vaultSigner: string;
  attestationPolicyHash: string;
  teePlatform?: A402TeePlatform | string;
  attestationDocument: string;
  snapshotSeqno?: number;
  tlsPublicKeySha256?: string;
  manifestHash?: string;
  issuedAt: string;
  expiresAt: string;
}

export interface NitroAttestationPolicy {
  version: number;
  pcrs: Record<string, string>;
  eifSigningCertSha256: string;
  kmsKeyArnSha256: string;
  protocol: string;
}

export interface A402NitroUserDataEnvelope {
  version: number;
  vaultConfig: string;
  vaultSigner: string;
  attestationPolicyHash: string;
  snapshotSeqno: number;
  tlsPublicKeySha256?: string;
  manifestHash?: string;
}

export interface A402SevSnpAttestationEnvelope {
  version: number;
  platform: "amd-sev-snp";
  reportB64: string;
  vaultConfig: string;
  vaultSigner: string;
  attestationPolicyHash: string;
  snapshotSeqno: number;
  tlsPublicKeySha256?: string;
  manifestHash?: string;
  issuedAt: string;
  expiresAt: string;
}

export interface NitroAttestationDocument {
  moduleId: string;
  timestampMs: number;
  digest: string;
  pcrs: Record<string, string>;
  certificatePem: string;
  cabundlePem: string[];
  publicKeyDerB64?: string;
  userDataB64?: string;
  nonceB64?: string;
  parsedA402UserData?: A402NitroUserDataEnvelope | null;
}

export interface NitroAttestationConfig {
  policy?: NitroAttestationPolicy;
  expectedPcrs?: Record<string, string>;
  expectedPolicyHash?: string;
  expectedNonce?: string | Uint8Array;
  expectedVaultSigner?: string;
  maxAgeMs?: number;
  rootCertificatesPem?: string[];
  requireA402UserData?: boolean;
  /**
   * Escape hatch for callers who explicitly want to skip PCR pinning. Without
   * this, `verifyNitroAttestationDocument()` throws when neither `policy.pcrs`
   * nor `expectedPcrs` is provided, because a self-referential policy hash
   * check gives no cryptographic binding to a specific enclave image.
   */
  allowMissingPcrPinning?: boolean;
  documentValidator?: (
    document: NitroAttestationDocument,
    attestation: AttestationResponse
  ) => Promise<void> | void;
}

/** Facilitator /v1/withdraw-auth response */
export interface WithdrawAuthResponse {
  ok: boolean;
  withdrawNonce: number;
  expiresAt: number;
  signature: string;
  message: string;
}

/** PAYMENT-RESPONSE header content */
export interface PaymentResponse {
  scheme: string;
  paymentId: string;
  verificationId: string;
  settlementId: string | null;
  batchId: number | null;
  txSignature: string | null;
  participantReceipt: string | null;
}

/** Facilitator /v1/balance response */
export interface BalanceResponse {
  ok: boolean;
  client: string;
  free: number;
  locked: number;
  totalDeposited: number;
  totalWithdrawn: number;
}

/** Facilitator /v1/receipt response (ParticipantReceipt) */
export interface ParticipantReceiptResponse {
  ok: boolean;
  participant: string;
  participantKind: number;
  recipientAta: string;
  freeBalance: number;
  lockedBalance: number;
  maxLockExpiresAt: number;
  nonce: number;
  timestamp: number;
  snapshotSeqno: number;
  vaultConfig: string;
  signature: string;
  message: string;
}

// ── Phase 3: Atomic Service Channel Types ──

/** Channel status */
export type ChannelStatus = "open" | "locked" | "pending" | "closed";

/** Facilitator /v1/channel/open response */
export interface OpenChannelResponse {
  ok: boolean;
  channelId: string;
  clientFree: number;
  clientLocked: number;
}

/** Facilitator /v1/channel/request response */
export interface ChannelRequestResponse {
  ok: boolean;
  channelId: string;
  requestId: string;
  status: ChannelStatus;
}

export interface AscClaimVoucher {
  message: string;
  signature: string;
  issuedAt: number;
  channelIdHash: string;
  requestIdHash: string;
}

/** Facilitator /v1/channel/deliver response */
export interface ChannelDeliverResponse {
  ok: boolean;
  channelId: string;
  status: ChannelStatus;
  claimVoucher: AscClaimVoucher;
}

/** Facilitator /v1/channel/finalize response */
export interface ChannelFinalizeResponse {
  ok: boolean;
  channelId: string;
  result: string; // base64 encoded
  amountPaid: number;
  status: ChannelStatus;
}

/** Facilitator /v1/channel/close response */
export interface CloseChannelResponse {
  ok: boolean;
  channelId: string;
  providerId: string;
  returnedToClient: number;
  providerEarned: number;
}

/** Direct vault/enclave client configuration */
export interface A402VaultClientConfig {
  /** Client wallet keypair */
  walletKeypair: {
    publicKey: A402PublicKeyLike;
    secretKey: Uint8Array;
  };
  /** VaultConfig PDA address */
  vaultAddress: A402PublicKeyLike;
  /** Enclave facilitator base URL */
  enclaveUrl: string;
  /** Solana RPC URL */
  rpcUrl?: string;
  /** Built-in Nitro attestation verification configuration. */
  nitroAttestation?: NitroAttestationConfig;
  /** Optional custom verifier for non-local attestation documents. */
  attestationVerifier?: (
    attestation: AttestationResponse
  ) => Promise<void> | void;
}

export interface A402Signer {
  address?: Address<string> | string;
  publicKey?: string | { toBase58(): string };
  secretKey?: Uint8Array;
  signMessage?: (message: Uint8Array) => Promise<Uint8Array> | Uint8Array;
  signMessages?: MessagePartialSigner["signMessages"];
}

export interface A402AutoDepositContext {
  /** Amount requested by the selected payment details, in atomic token units. */
  amount: string;
  amountAtomic: bigint;
  details: PaymentDetails;
  facilitatorUrl: string;
  reason: string;
  attempt: number;
}

export interface A402AutoDepositConfig {
  mode?: "on-demand";
  /** Budget guard for the on-demand deposit itself. */
  maxDepositPerRequest?: string | number;
  /**
   * Performs the deposit/top-up and waits until the facilitator can observe it.
   * Browser, Node, and agent runtimes can provide wallet-adapter, Anchor, or
   * custody-specific implementations here.
   */
  deposit: (context: A402AutoDepositContext) => Promise<void> | void;
}

/** x402-compatible wrapper client configuration */
export interface A402ClientConfig {
  /**
   * Optional convenience signer. For the x402-like interface, prefer
   * client.register("solana:*", new A402ExactScheme(signer)).
   */
  signer?: A402Signer;
  network?: string;
  /** Allowed A402 facilitator base URLs discovered from HTTP 402 responses. */
  trustedFacilitators?: string[];
  policy?: {
    /** Budget guard only. Actual price is discovered from HTTP 402. */
    maxPaymentPerRequest?: string | number;
  };
  nitroAttestation?: NitroAttestationConfig;
  attestationVerifier?: (
    attestation: AttestationResponse
  ) => Promise<void> | void;
  /**
   * Optional x402-like on-demand deposit. When a signed retry fails because the
   * vault balance is insufficient, the SDK calls this hook, then signs and
   * retries the request once more.
   */
  autoDeposit?:
    | false
    | A402AutoDepositConfig
    | A402AutoDepositConfig["deposit"];
}
