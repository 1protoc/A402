# A402 AMD SEV-SNP Deployment

This document describes the SEV-SNP option for the A402 vault runtime.

## Model

AWS Nitro Enclaves isolate a separate enclave from an untrusted parent instance.
AMD SEV-SNP instead protects an entire VM. In the A402 SEV-SNP deployment, the
trusted component is therefore the confidential guest VM running `a402-enclave`.

TLS still terminates inside the trusted boundary. Public chain observers still
see only vault-to-provider aggregate settlement outputs. The main difference is
how the runtime proves its identity: Nitro emits an NSM attestation document,
while SEV-SNP emits an AMD SNP report.

## Runtime Configuration

Set:

```bash
A402_TEE_PLATFORM=sev-snp
A402_SEV_SNP_ATTESTATION_REPORT_B64=<base64_snp_report>
A402_SEV_SNP_MEASUREMENT_HEX=<launch_measurement_hex>
```

If `A402_ATTESTATION_POLICY_HASH_HEX` is omitted, the enclave derives the policy
hash from a canonical JSON policy:

```json
{
  "version": 1,
  "platform": "amd-sev-snp",
  "measurement": "<A402_SEV_SNP_MEASUREMENT_HEX>",
  "protocol": "a402-svm-v1"
}
```

Optional fields include:

- `A402_SEV_SNP_HOST_DATA_HEX`
- `A402_SEV_SNP_REPORT_DATA_HEX`
- `A402_SEV_SNP_FAMILY_ID_HEX`
- `A402_SEV_SNP_IMAGE_ID_HEX`
- `A402_SEV_SNP_POLICY_HEX`
- `A402_SEV_SNP_VMPL`

## Attestation Response

`GET /v1/attestation` returns:

```json
{
  "teePlatform": "amd-sev-snp",
  "attestationDocument": "base64(json-envelope)",
  "attestationPolicyHash": "...",
  "tlsPublicKeySha256": "..."
}
```

The base64 envelope contains:

- `platform: "amd-sev-snp"`
- `reportB64`
- `vaultConfig`
- `vaultSigner`
- `attestationPolicyHash`
- `snapshotSeqno`
- optional TLS and manifest hashes
- issue and expiry timestamps

## Verification Requirements

A production verifier must validate the AMD VCEK chain and SNP report signature,
then check that the measured launch state and report bindings match the expected
A402 policy. In particular, bind the runtime TLS public key hash into SNP
`REPORT_DATA` so the verifier knows it is talking to the attested runtime.

The SDK exposes a custom `attestationVerifier` hook for SEV-SNP verification.
The built-in Nitro verifier remains Nitro-specific.

## Deployment Files

See [infra/sev-snp/README.md](../infra/sev-snp/README.md) for systemd and env
templates.
