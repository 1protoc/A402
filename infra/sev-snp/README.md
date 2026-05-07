# AMD SEV-SNP Runtime

This directory contains the deployment shape for running the A402 vault runtime
inside an AMD SEV-SNP confidential VM.

SEV-SNP is a VM-level TEE rather than a separate enclave process. For A402 this
means the trusted boundary is the confidential VM itself: `a402-enclave` runs as
a normal Linux process inside the SNP guest, TLS terminates inside that guest,
and the host/hypervisor is treated as untrusted infrastructure.

## Topology

```text
Internet
  -> TCP passthrough load balancer
  -> SEV-SNP guest VM
  -> a402-enclave rustls/hyper facilitator
  -> Solana / Ethereum / Bitcoin RPC, provider endpoints
```

Unlike the Nitro deployment, there is no parent-to-enclave vsock split by
default. If an operator wants the same separation, they can still run the
existing `parent` relay outside the SNP guest and forward TCP into the guest,
but the security boundary remains the SEV-SNP VM.

## Current Support Level

The Rust runtime supports `A402_TEE_PLATFORM=sev-snp` and publishes an
`amd-sev-snp` attestation envelope from `/v1/attestation`.

The current implementation expects a base64 SNP report supplied through:

```bash
A402_SEV_SNP_ATTESTATION_REPORT_B64=
```

Production deployments should replace this static report source with an
in-guest collector that reads `/dev/sev-guest`, requests a fresh SNP report, and
binds the runtime TLS public key hash into SNP `REPORT_DATA`.

Client-side verification should validate:

- the AMD VCEK certificate chain;
- the SNP report signature;
- `measurement`;
- `host_data` or launch digest policy;
- `report_data` binding to the A402 TLS public key hash;
- the `attestationPolicyHash` advertised by the facilitator.

The TypeScript SDK exposes the generic `attestationVerifier` hook for this
verification path.

## Files

- `env/enclave.env.example`: runtime variables for an SNP guest
- `systemd/a402-sev-snp-enclave.service`: example long-running service
- `scripts/collect-sev-snp-report.sh`: placeholder report collection hook

## Key Differences from Nitro

- Nitro uses a separate enclave and NSM attestation document.
- SEV-SNP attests the whole confidential VM through an SNP report.
- AWS KMS recipient attestation is Nitro-specific; SNP deployments should use a
  cloud KMS with SNP attestation support or inject encrypted bootstrap secrets
  through another measured bootstrapping path.
- WAL and snapshots still leave the trusted boundary encrypted, but they are
  written by the SNP guest directly unless an external relay is added.
