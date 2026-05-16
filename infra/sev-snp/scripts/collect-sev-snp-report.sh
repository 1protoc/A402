#!/usr/bin/env bash
set -euo pipefail

echo "This deployment template expects an in-guest SEV-SNP report collector." >&2
echo "Install a collector that reads /dev/sev-guest and prints base64(report)." >&2
echo "Then export the result as A402_SEV_SNP_ATTESTATION_REPORT_B64." >&2
exit 1
