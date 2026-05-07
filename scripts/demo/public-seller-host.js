#!/usr/bin/env node

const express = require("express");
const {
  paymentMiddleware,
  x402ResourceServer: X402ResourceServer,
} = require("@x402/express");
const { HTTPFacilitatorClient } = require("@x402/core/server");
const {
  ExactSvmScheme: X402SellerExactSvmScheme,
} = require("@x402/svm/exact/server");
const {
  A402ExactScheme,
  A402FacilitatorClient,
  A402ResourceServer,
  captureA402RawBody,
  paymentMiddleware: a402PaymentMiddleware,
} = require("../../middleware/dist");

const {
  DEFAULT_A402_NETWORK,
  DEFAULT_X402_FACILITATOR_URL,
  DEFAULT_X402_NETWORK,
  DEMO_DESCRIPTION,
  DEMO_MIME_TYPE,
  demoPaymentAmount,
  demoWeatherResponse,
  deriveTokenAccount,
  explorerAddress,
  formatUsdcAtomic,
  loadFourWayDemoEnv,
  logKV,
  printHeader,
  requireEnv,
} = require("./four-way-common");

const X402_ROUTE = "/x402/weather";
const A402_ROUTE = "/a402/weather";

function listen(app, host, port) {
  return new Promise((resolve, reject) => {
    const server = app.listen(port, host);
    server.once("listening", () => resolve(server));
    server.once("error", reject);
  });
}

async function fetchAttestationSummary(facilitatorUrl) {
  const response = await fetch(`${facilitatorUrl}/v1/attestation`, {
    cache: "no-store",
  });
  if (!response.ok) {
    return {
      ok: false,
      status: response.status,
      message: await response.text(),
    };
  }
  const body = await response.json();
  return {
    ok: true,
    vaultConfig: body.vaultConfig,
    vaultSigner: body.vaultSigner,
    attestationPolicyHash: body.attestationPolicyHash,
    tlsPublicKeySha256: body.tlsPublicKeySha256,
    snapshotSeqno: body.snapshotSeqno,
    issuedAt: body.issuedAt,
    expiresAt: body.expiresAt,
  };
}

async function main() {
  loadFourWayDemoEnv();

  const host = process.env.A402_SELLER_HOST || "0.0.0.0";
  const port = Number(process.env.A402_SELLER_PORT || 8080);
  const configuredPublicBaseUrl = process.env.A402_SELLER_PUBLIC_URL;
  let publicBaseUrl = configuredPublicBaseUrl || `http://localhost:${port}`;
  const sellerWallet =
    process.env.A402_DEMO_SELLER_WALLET || process.env.SELLER_WALLET;
  if (!sellerWallet) {
    throw new Error(
      "A402_DEMO_SELLER_WALLET or SELLER_WALLET is required on the seller host"
    );
  }

  const usdcMint = requireEnv("A402_USDC_MINT");
  const sellerTokenAccount = await deriveTokenAccount(sellerWallet, usdcMint);
  const paymentAmount = demoPaymentAmount();
  const a402FacilitatorUrl = requireEnv("A402_PUBLIC_ENCLAVE_URL").replace(
    /\/$/,
    ""
  );
  const a402Network = process.env.A402_NETWORK || DEFAULT_A402_NETWORK;
  const x402Network = process.env.A402_X402_NETWORK || DEFAULT_X402_NETWORK;
  const x402FacilitatorUrl =
    process.env.A402_X402_FACILITATOR_URL || DEFAULT_X402_FACILITATOR_URL;

  const app = express();
  app.use(express.json({ verify: captureA402RawBody }));

  const x402Facilitator = new HTTPFacilitatorClient({
    url: x402FacilitatorUrl,
  });
  const officialX402ResourceServer = new X402ResourceServer(
    x402Facilitator
  ).register(x402Network, new X402SellerExactSvmScheme());

  app.use(
    paymentMiddleware(
      {
        [`GET ${X402_ROUTE}`]: {
          accepts: [
            {
              scheme: "exact",
              price: {
                asset: usdcMint,
                amount: paymentAmount.toString(),
              },
              network: x402Network,
              payTo: sellerWallet,
              maxTimeoutSeconds: 120,
            },
          ],
          description: DEMO_DESCRIPTION,
          mimeType: DEMO_MIME_TYPE,
        },
      },
      officialX402ResourceServer,
      {
        appName: "A402 public seller host official x402 route",
        testnet: true,
      }
    )
  );

  const a402Facilitator = new A402FacilitatorClient({
    url: a402FacilitatorUrl,
    assetMint: usdcMint,
  });
  const a402ResourceServer = new A402ResourceServer(
    a402Facilitator
  ).register("solana:*", new A402ExactScheme());

  app.use(
    a402PaymentMiddleware(
      {
        [`GET ${A402_ROUTE}`]: {
          accepts: [
            {
              scheme: "exact",
              price: paymentAmount,
              network: a402Network,
              sellerWallet,
            },
          ],
          description: DEMO_DESCRIPTION,
          mimeType: DEMO_MIME_TYPE,
        },
      },
      a402ResourceServer
    )
  );

  app.get("/healthz", (_req, res) => {
    res.json({ ok: true });
  });

  app.get("/.well-known/a402.json", async (_req, res) => {
    res.json({
      ok: true,
      publicBaseUrl,
      sellerWallet,
      sellerTokenAccount,
      asset: {
        kind: "spl-token",
        mint: usdcMint,
        decimals: 6,
        symbol: "USDC",
      },
      routes: {
        x402: `${publicBaseUrl}${X402_ROUTE}`,
        a402: `${publicBaseUrl}${A402_ROUTE}`,
      },
      x402: {
        network: x402Network,
        facilitatorUrl: x402FacilitatorUrl,
      },
      a402: {
        network: a402Network,
        facilitatorUrl: a402FacilitatorUrl,
        attestation: await fetchAttestationSummary(a402FacilitatorUrl),
      },
    });
  });

  app.get(X402_ROUTE, (_req, res) => {
    res.json(
      demoWeatherResponse({
        mode: "official-x402-direct",
        providerId: "official-x402-weather",
      })
    );
  });

  app.get(A402_ROUTE, (req, res) => {
    res.json(
      demoWeatherResponse({
        mode: "a402-private-x402",
        providerId: req.a402?.providerId || "derived-open-seller",
      })
    );
  });

  const server = await listen(app, host, port);
  const address = server.address();
  publicBaseUrl = configuredPublicBaseUrl || `http://localhost:${address.port}`;
  const boundUrl = `http://${host}:${address.port}`;

  printHeader("Public Seller Host");
  logKV("Bind", boundUrl);
  logKV("Public base URL", publicBaseUrl);
  logKV("Seller wallet", sellerWallet);
  logKV("Seller token account", sellerTokenAccount);
  logKV("Seller explorer", explorerAddress(sellerTokenAccount));
  logKV("Price", formatUsdcAtomic(paymentAmount));
  logKV("Official x402 route", `${publicBaseUrl}${X402_ROUTE}`);
  logKV("A402 route", `${publicBaseUrl}${A402_ROUTE}`);
  logKV("Metadata", `${publicBaseUrl}/.well-known/a402.json`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
