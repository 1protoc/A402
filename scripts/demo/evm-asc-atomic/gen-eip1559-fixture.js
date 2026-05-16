#!/usr/bin/env node
"use strict";

/**
 * Deterministic cross-stack fixture for the EIP-1559 transaction signer.
 *
 * Pins:
 *   - private key (Anvil deterministic account #1)
 *   - tx params (chainId, nonce, fees, gas, to, data)
 *   - viem-produced unsigned + signed serialized bytes
 *   - the y-parity / r / s scalars
 *
 * The Rust `evm_tx::tests::cross_stack_eip1559` test consumes this and
 * asserts byte-identical serialization of the unsigned and signed encodings.
 * If viem ever changes the canonical RLP layout for type-2 transactions, this
 * fixture's check will trip immediately.
 */

const fs = require("fs");
const path = require("path");
const {
    privateKeyToAccount,
} = require("viem/accounts");
const {
    serializeTransaction,
    parseTransaction,
    keccak256,
} = require("viem");

async function main() {
    const privateKey =
        "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
    const account = privateKeyToAccount(privateKey);

    const tx = {
        type: "eip1559",
        chainId: 31337,
        nonce: 7,
        maxPriorityFeePerGas: 1_000_000_000n,
        maxFeePerGas: 2_000_000_000n,
        gas: 100_000n,
        to: "0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9",
        value: 0n,
        data: "0xabcdef",
    };

    const unsigned = serializeTransaction(tx);
    const signature = await account.sign({hash: keccak256(unsigned)});
    const signed = serializeTransaction(tx, parseSig(signature));

    const fixture = {
        comment:
            "deterministic EIP-1559 cross-stack fixture; produced by viem (gen-eip1559-fixture.js)",
        privateKey,
        signerAddress: account.address,
        tx: {
            chainId: tx.chainId,
            nonce: tx.nonce,
            maxPriorityFeePerGas: tx.maxPriorityFeePerGas.toString(),
            maxFeePerGas: tx.maxFeePerGas.toString(),
            gasLimit: tx.gas.toString(),
            to: tx.to,
            value: tx.value.toString(),
            data: tx.data,
        },
        unsigned, // 0x02 || rlp([...nine elements])
        signed, // 0x02 || rlp([...twelve elements with yParity/r/s])
        signature: {
            ...parseSig(signature),
        },
        // Reparse to expose the recovered y-parity Viem assigns.
        parsedSigned: stripBigInts(parseTransaction(signed)),
    };

    const outDir = path.resolve(__dirname, "..", "..", "..", "tests", "fixtures");
    fs.mkdirSync(outDir, {recursive: true});
    const outPath = path.join(outDir, "eip1559_fixture.json");
    fs.writeFileSync(outPath, JSON.stringify(fixture, null, 2) + "\n");
    console.log(`wrote ${outPath}`);
    console.log(`  unsigned len = ${(unsigned.length - 2) / 2} bytes`);
    console.log(`  signed   len = ${(signed.length - 2) / 2} bytes`);
}

function parseSig(hex) {
    // viem returns r || s || v as 65 bytes. v is 27 or 28 → yParity = v - 27.
    const stripped = hex.startsWith("0x") ? hex.slice(2) : hex;
    const r = "0x" + stripped.slice(0, 64);
    const s = "0x" + stripped.slice(64, 128);
    const vHex = stripped.slice(128, 130);
    const v = parseInt(vHex, 16);
    const yParity = v - 27;
    return {r, s, v, yParity};
}

function stripBigInts(obj) {
    return JSON.parse(
        JSON.stringify(obj, (_k, v) => (typeof v === "bigint" ? v.toString() : v))
    );
}

main().catch((e) => {
    console.error(e);
    process.exit(1);
});
