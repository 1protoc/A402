# Nitro Rollout

This directory is the template for deploying A402 to public Devnet on AWS Nitro Enclaves.

For the shortest path, see [`docs/nitro-devnet-deploy.md`](../../docs/nitro-devnet-deploy.md).

Contents:

- `terraform/`: Skeleton for parent EC2, NLB, IAM, KMS, and snapshot bucket resources
- `env/`: Environment templates for `parent` and `watchtower`
- `systemd/`: Units for running parent and watchtower as long-lived services
- `enclave/`: Dockerfile and entrypoint for building the EIF

Added automation:

- `yarn nitro:prepare`: Generates vault signer ciphertext and runtime env files
- `yarn nitro:build-eif`: Builds the EIF and writes measurements
- `yarn nitro:provision`: Finalizes the policy hash from PCRs and initializes the vault on-chain

Prerequisites:

- The Solana program has already been deployed to Devnet
- `a402-parent`, `a402-watchtower`, and `a402-enclave` can be built
- The EIF for the enclave is built separately
- AWS VPC, subnet, and AMI values are supplied for your environment

Procedure:

1. Create `terraform.tfvars` in `infra/nitro/terraform`
2. Run `terraform init && terraform apply` to create EC2, NLB, KMS, and S3 resources
3. Copy `a402-parent`, `a402-watchtower`, and the EIF to the generated parent EC2 instance
4. Copy `env/*.example` to `/etc/a402/*.env` and fill in the values
5. Copy `systemd/*.service` to `/etc/systemd/system/` and run `systemctl enable --now`
6. Start the EIF with Nitro and connect with `A402_PARENT_INTERCONNECT_MODE=vsock` / `A402_ENCLAVE_INTERCONNECT_MODE=vsock`

Important notes:

- `ingress`, `KMS`, and `snapshot_store` support both `tcp(dev)` and `vsock(prod)` modes
- Enclave outbound HTTP, HTTPS, and Solana RPC traffic exits through `parent egress_relay`
- The Nitro attestation document for bootstrap is generated inside the enclave through NSM and used for KMS decrypt / data-key retrieval
- `deposit_detector` watches the vault token account with `logsSubscribe(finalized)` and catches up after disconnects
- In production, set `A402_EGRESS_ALLOWLIST` to restrict parent relay destinations

Remaining work before keeping the public URL live:

1. Pin `A402_EGRESS_ALLOWLIST` and AWS-side egress controls to production values
2. Move EIF build and PCR measurement into CI or the build script
3. Bind the KMS key policy to the actual PCR values
