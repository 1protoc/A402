#!/usr/bin/env node

const express = require("express");
const {
  A402ExactScheme,
  A402FacilitatorClient,
  A402ResourceServer,
  captureA402RawBody,
  paymentMiddleware,
} = require("../../middleware/dist");

const {
  DEFAULT_A402_NETWORK,
  DEMO_DESCRIPTION,
  DEMO_MIME_TYPE,
  DEMO_ROUTE_PATH,
  demoPaymentAmount,
  demoWeatherResponse,
  explorerAddress,
  formatUsdcAtomic,
  loadFourWayDemoEnv,
  logKV,
  printHeader,
  requireEnv,
  resolveDemoSeller,
  shortKey,
} = require("./four-way-common");

function listen(app, host, port) {
  return new Promise((resolve, reject) => {
    const server = app.listen(port, host);
    server.once("listening", () => resolve(server));
    server.once("error", reject);
  });
}

async function main() {
  loadFourWayDemoEnv();

  const facilitatorUrl = requireEnv("A402_PUBLIC_ENCLAVE_URL").replace(
    /\/$/,
    ""
  );
  const usdcMint = requireEnv("A402_USDC_MINT");
  const network = process.env.A402_NETWORK || DEFAULT_A402_NETWORK;
  const host = process.env.A402_SELLER_HOST || "127.0.0.1";
  const port = Number(process.env.A402_SELLER_PORT || 4022);
  const paymentAmount = demoPaymentAmount();
  const { sellerWallet, associatedTokenAccount } = await resolveDemoSeller();

  const facilitatorOptions = {
    url: facilitatorUrl,
    assetMint: usdcMint,
  };

  const routeAccept = {
    scheme: "exact",
    price: paymentAmount,
    network,
    sellerWallet,
  };

  const app = express();
  app.use(express.json({ verify: captureA402RawBody }));

  const facilitator = new A402FacilitatorClient(facilitatorOptions);
  const resourceServer = new A402ResourceServer(facilitator).register(
    "solana:*",
    new A402ExactScheme()
  );

  app.use(
    paymentMiddleware(
      {
        [`GET ${DEMO_ROUTE_PATH}`]: {
          accepts: [routeAccept],
          description: DEMO_DESCRIPTION,
          mimeType: DEMO_MIME_TYPE,
        },
      },
      resourceServer
    )
  );

  app.get(DEMO_ROUTE_PATH, (req, res) => {
    res.json(
      demoWeatherResponse({
        mode: "a402-private-x402",
        providerId: req.a402?.providerId || "derived-open-seller",
      })
    );
  });

  const server = await listen(app, host, port);
  const address = server.address();
  const url = `http://${host}:${address.port}${DEMO_ROUTE_PATH}`;

  printHeader("A402 Seller");
  logKV(
    "Docs pattern",
    "a402-express paymentMiddleware + A402ExactScheme"
  );
  logKV("URL", url);
  logKV("Route", `GET ${DEMO_ROUTE_PATH}`);
  logKV("Network", network);
  logKV("Facilitator", facilitatorUrl);
  logKV(
    "Provider mode",
    "open seller; no pre-registration or provider API key"
  );
  logKV("Price", formatUsdcAtomic(paymentAmount));
  logKV("Seller wallet", sellerWallet);
  logKV("Seller token account", associatedTokenAccount);
  logKV(
    "Provider id",
    "derived from network + asset mint + seller token account"
  );
  logKV("Provider auth", "none");
  logKV("Explorer", explorerAddress(associatedTokenAccount));
  console.log("");
  console.log("Run the paired buyer in another terminal:");
  console.log(`  A402_SELLER_URL='${url}' yarn demo:a402-buyer`);
  console.log("");
  console.log("Privacy note:");
  console.log(
    `  Buyer pays ${shortKey(
      requireEnv("A402_VAULT_TOKEN_ACCOUNT")
    )} first; seller payout is batched later.`
  );
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
