# Nitro Devnet Deployment

This runbook is the shortest path for publishing `Privacy First x402` to Devnet with AWS Nitro Enclaves.

For routine updates to the existing `api.demo.a402fi.com` environment, use the [Devnet Redeploy Runbook](./redeploy-devnet.md). That flow builds artifacts on the Build EC2 instance and deploys them to the Parent EC2 instance through S3.

Prerequisites:

- You can deploy the Solana program to Devnet
- AWS CLI, Terraform, Docker, and Nitro CLI are available
- You have a Devnet RPC URL and a funded deploy wallet
- An EIF signing certificate is prepared
- The parent EC2 instance uses a Nitro Enclaves-capable instance type

Generated artifacts are written under `infra/nitro/generated/`.

## 0. Create the KMS Key First

`yarn nitro:prepare` immediately converts the vault signer seed into KMS ciphertext, so you need a KMS key ARN before running it.

In the AWS Console:

1. Open `KMS`
2. Open `Customer managed keys`
3. Select `Create key`
4. Select `Symmetric`
5. Select `Encrypt and decrypt`
6. Set an alias
7. Copy the key ARN

Put that ARN in `.env.devnet.local` as `A402_KMS_KEY_ARN`.

## 0. Local Env

Add at least these values to `.env.devnet.local`:

```bash
export A402_SOLANA_RPC_URL='https://<your-devnet-rpc>'
export A402_SOLANA_WS_URL='wss://<your-devnet-ws>'
export ANCHOR_PROVIDER_URL="$A402_SOLANA_RPC_URL"
export ANCHOR_WALLET="$HOME/.config/solana/<wallet>.json"

export A402_KMS_KEY_ARN='arn:aws:kms:us-east-1:123456789012:key/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx'
export A402_KMS_KEY_ID="$A402_KMS_KEY_ARN"
export A402_SNAPSHOT_DATA_KEY_ID="$A402_KMS_KEY_ARN"
export A402_EIF_SIGNING_CERT_PATH="$PWD/infra/nitro/certs/eif-signing-cert.pem"
export A402_NITRO_SIGNING_PRIVATE_KEY="$PWD/infra/nitro/certs/eif-signing-key.pem"
export AWS_REGION='us-east-1'
```

Set TLS certificate source paths before building the EIF:

```bash
export A402_ENCLAVE_TLS_CERT_SOURCE="$PWD/infra/nitro/certs/server.crt"
export A402_ENCLAVE_TLS_KEY_SOURCE="$PWD/infra/nitro/certs/server.key"
```

## 1. Program Deploy

```bash
source ./.env.devnet.local
NO_DNA=1 anchor build
NO_DNA=1 anchor deploy   --provider.cluster "$A402_SOLANA_RPC_URL"   --provider.wallet "$ANCHOR_WALLET"
```

## 2. Nitro Prepare

This step:

- Finalizes the planned `vaultConfig` and `vaultTokenAccount`
- Generates the vault signer seed and converts it to KMS ciphertext
- Generates and funds the watchtower keypair
- Generates `parent.env`, `watchtower.env`, `enclave.env`, and `run-enclave.json`

```bash
yarn nitro:prepare
```

Outputs:

- `infra/nitro/generated/nitro-plan.json`
- `infra/nitro/generated/parent.env`
- `infra/nitro/generated/watchtower.env`
- `infra/nitro/generated/enclave.env`
- `infra/nitro/generated/run-enclave.json`

## 3. Build Signed EIF

```bash
yarn nitro:build-eif
```

Outputs:

- `infra/nitro/generated/a402-enclave.eif`
- `infra/nitro/generated/eif-measurements.json`

Important notes:

- Nitro does not bake `attestation_policy_hash` into the EIF
- The enclave runtime derives the hash from measured PCR values plus `A402_KMS_KEY_ARN_SHA256` and `A402_EIF_SIGNING_CERT_SHA256`
- Therefore finalizing the policy hash after the EIF build does not create a circular dependency

## 4. On-chain Initialize + Policy Materialization

Build the policy hash from the measured EIF values and pin it in `initialize_vault`.

This step also derives `PCR3` from the IAM role ARN for the parent EC2 instance. If you change Terraform `project_name` from the default (`a402-devnet`), set `A402_NITRO_PROJECT_NAME` to the same value before running this command.

```bash
yarn nitro:provision
```

Outputs:

- `infra/nitro/generated/attestation-policy.json`
- `infra/nitro/generated/attestation-policy.hash`
- `infra/nitro/generated/terraform.attestation.auto.tfvars.json`
- `infra/nitro/generated/nitro-state.json`
- `infra/nitro/generated/client.env`

## 4.5. Build Parent and Watchtower Release Binaries

```bash
NO_DNA=1 cargo build --release -p a402-parent -p a402-watchtower
```

## 5. Terraform Apply

Copy `infra/nitro/generated/terraform.attestation.auto.tfvars.json` into `infra/nitro/terraform/` or pass it with `-var-file`.

Example:

```bash
cd infra/nitro/terraform
terraform init
terraform apply   -var-file=../generated/terraform.attestation.auto.tfvars.json   -var="existing_runtime_kms_key_arn=$A402_KMS_KEY_ARN"   -var='aws_region=us-east-1'   -var='vpc_id=vpc-xxxx'   -var='nlb_subnet_ids=["subnet-a","subnet-b"]'   -var='instance_subnet_id=subnet-a'   -var='ami_id=ami-xxxx'   -var='snapshot_bucket_name=a402-devnet-snapshots-xxxx'
```

Add `kms_provisioner_principal_arns` to the tfvars file if needed.

Important notes:

- Pass the same KMS key used by `nitro:prepare` as `existing_runtime_kms_key_arn`
- Terraform applies an attestation-aware policy to that key
- Do not create a separate KMS key at this step

## 6. Parent Instance Setup

Place these files on the EC2 instance:

- `target/release/a402-parent`
- `target/release/a402-watchtower`
- `infra/nitro/generated/a402-enclave.eif`
- `infra/nitro/generated/parent.env`
- `infra/nitro/generated/watchtower.env`
- `infra/nitro/generated/run-enclave.json`
- `scripts/nitro/start-parent.sh`
- `scripts/nitro/start-watchtower.sh`
- `infra/nitro/systemd/a402-parent.service`
- `infra/nitro/systemd/a402-watchtower.service`

Recommended layout:

- `/opt/a402/bin/a402-parent`
- `/opt/a402/bin/a402-watchtower`
- `/opt/a402/bin/start-parent.sh`
- `/opt/a402/bin/start-watchtower.sh`
- `/opt/a402/bin/run-enclave.sh`
- `/opt/a402/enclave/a402-enclave.eif`
- `/etc/a402/parent.env`
- `/etc/a402/watchtower.env`
- `/etc/a402/run-enclave.json`

## 7. Start Runtime

The recommended path is systemd:

```bash
sudo cp infra/nitro/systemd/a402-parent.service /etc/systemd/system/
sudo cp infra/nitro/systemd/a402-watchtower.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now a402-watchtower
sudo systemctl enable --now a402-parent
```

If starting directly, use wrappers that read env files.

watchtower:

```bash
bash /opt/a402/bin/start-watchtower.sh /etc/a402/watchtower.env
```

parent:

```bash
bash /opt/a402/bin/start-parent.sh /etc/a402/parent.env
```

enclave:

```bash
NO_DNA=1 nitro-cli run-enclave --config /etc/a402/run-enclave.json
```

From the repo:

```bash
yarn nitro:run /etc/a402/run-enclave.json
```

Status checks:

```bash
yarn nitro:describe
curl -sk https://<nlb-dns>/v1/attestation | jq .
```

## 8. Public Smoke Test

For the first smoke test only, rebuild the EIF with:

- `A402_ENABLE_PROVIDER_REGISTRATION_API=1`
- `A402_ENABLE_ADMIN_API=1`
- `A402_ADMIN_AUTH_TOKEN=<operator-only-random-token>`

After the public smoke test passes, set both API flags back to `0` and rebuild the EIF. `prepare` writes `A402_ADMIN_AUTH_TOKEN_SHA256` into the enclave env instead of the raw token.

Use `A402_ALLOW_ADMIN_PRIVACY_BYPASS_BATCH=1` only when a single-provider demo needs immediate batching. Keep it at `0` for public runtime.

## Notes

- Do not expose `watchtower` publicly
- Do not terminate TLS at ALB or parent nginx
- Do not place `A402_VAULT_SIGNER_SECRET_KEY_B64` on the parent instance
- Nitro runtime derives `A402_ATTESTATION_POLICY_HASH_HEX` from runtime measurements instead of baking it into the image
