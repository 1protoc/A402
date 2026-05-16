#!/usr/bin/env node
"use strict";

/**
 * Deterministic ABI-encoding fixture for the EVM ASC adapter.
 *
 * Generates calldata for each ASCManager function the Rust enclave needs to
 * encode, using viem as the source of truth. The Rust `evm_chain` module's
 * cross-stack test consumes the resulting JSON and asserts byte-exact agreement.
 *
 * Run with:
 *   node scripts/demo/evm-asc-atomic/gen-abi-fixture.js
 *
 * Inputs are hand-picked constants that exercise:
 *   - fixed-width primitives (bytes32, uint256, address)
 *   - dynamic bytes (the closeASC / initForceClose / forceClose signatures)
 *   - the "tails follow heads" offset encoding for multiple dynamic args
 *   - the ascStateHash packed-keccak
 */

const fs = require("fs");
const path = require("path");
const {encodeFunctionData, keccak256, encodePacked} = require("viem");

const ASC_MANAGER_ABI = [
    {
        type: "function",
        name: "createASC",
        stateMutability: "nonpayable",
        inputs: [
            {name: "cid", type: "bytes32"},
            {name: "client", type: "address"},
            {name: "provider", type: "address"},
            {name: "amount", type: "uint256"},
        ],
        outputs: [],
    },
    {
        type: "function",
        name: "closeASC",
        stateMutability: "nonpayable",
        inputs: [
            {name: "cid", type: "bytes32"},
            {name: "balanceC", type: "uint256"},
            {name: "balanceS", type: "uint256"},
            {name: "version", type: "uint256"},
            {name: "sigC", type: "bytes"},
            {name: "sigS", type: "bytes"},
        ],
        outputs: [],
    },
    {
        type: "function",
        name: "initForceClose",
        stateMutability: "nonpayable",
        inputs: [
            {name: "cid", type: "bytes32"},
            {name: "balanceC", type: "uint256"},
            {name: "balanceS", type: "uint256"},
            {name: "version", type: "uint256"},
            {name: "sig", type: "bytes"},
        ],
        outputs: [],
    },
    {
        type: "function",
        name: "finalForceClose",
        stateMutability: "nonpayable",
        inputs: [{name: "cid", type: "bytes32"}],
        outputs: [],
    },
    {
        type: "function",
        name: "forceClose",
        stateMutability: "nonpayable",
        inputs: [
            {name: "cid", type: "bytes32"},
            {name: "balanceC", type: "uint256"},
            {name: "balanceS", type: "uint256"},
            {name: "version", type: "uint256"},
            {name: "sigU", type: "bytes"},
            {name: "sigS", type: "bytes"},
            {name: "px", type: "uint256"},
            {name: "e", type: "uint256"},
            {name: "s", type: "uint256"},
        ],
        outputs: [],
    },
];

function hex(bytes) {
    return "0x" + Buffer.from(bytes).toString("hex");
}

function main() {
    // Pinned inputs used across functions.
    const cid =
        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const client = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC";
    const provider = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";
    const ascManager = "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0";
    const amount = 100_000n;
    const balanceC = 95_000n;
    const balanceS = 5_000n;
    const version = 5n;
    // 65-byte ECDSA-shaped signatures (r || s || v) filled with sentinel bytes.
    const sigC = hex(new Uint8Array(65).fill(0xc1));
    const sigS = hex(new Uint8Array(65).fill(0xd2));
    const sigU = hex(new Uint8Array(65).fill(0xa3));
    // Schnorr proof tuple — 32-byte big-endian scalars.
    const px = "0x1122334455667788991122334455667788991122334455667788991122334455";
    const e = "0xaabbccddeeff00112233445566778899aabbccddeeff001122334455667788ff";
    const s = "0x9999888877776666555544443333222211110000ffffeeeeddddccccbbbbaaaa";

    const fixture = {
        comment:
            "deterministic ABI-encoding fixture; run gen-abi-fixture.js to refresh",
        inputs: {
            ascManager,
            cid,
            client,
            provider,
            amount: amount.toString(),
            balanceC: balanceC.toString(),
            balanceS: balanceS.toString(),
            version: version.toString(),
            sigC,
            sigS,
            sigU,
            px,
            e,
            s,
        },
        ascStateHash: keccak256(
            encodePacked(
                ["string", "address", "bytes32", "uint256", "uint256", "uint256"],
                ["A402_ASC_STATE_V1", ascManager, cid, balanceC, balanceS, version]
            )
        ),
        calldata: {
            createASC: encodeFunctionData({
                abi: ASC_MANAGER_ABI,
                functionName: "createASC",
                args: [cid, client, provider, amount],
            }),
            closeASC: encodeFunctionData({
                abi: ASC_MANAGER_ABI,
                functionName: "closeASC",
                args: [cid, balanceC, balanceS, version, sigC, sigS],
            }),
            initForceClose: encodeFunctionData({
                abi: ASC_MANAGER_ABI,
                functionName: "initForceClose",
                args: [cid, balanceC, balanceS, version, sigC],
            }),
            finalForceClose: encodeFunctionData({
                abi: ASC_MANAGER_ABI,
                functionName: "finalForceClose",
                args: [cid],
            }),
            forceClose: encodeFunctionData({
                abi: ASC_MANAGER_ABI,
                functionName: "forceClose",
                args: [
                    cid,
                    balanceC,
                    balanceS,
                    version,
                    sigU,
                    sigS,
                    BigInt(px),
                    BigInt(e),
                    BigInt(s),
                ],
            }),
        },
    };

    const outDir = path.resolve(__dirname, "..", "..", "..", "tests", "fixtures");
    fs.mkdirSync(outDir, {recursive: true});
    const outPath = path.join(outDir, "evm_calldata_fixture.json");
    fs.writeFileSync(outPath, JSON.stringify(fixture, null, 2) + "\n");
    console.log(`wrote ${outPath}`);
    for (const [k, v] of Object.entries(fixture.calldata)) {
        console.log(`  ${k.padEnd(18)} len=${(v.length - 2) / 2}B`);
    }
    console.log(`  ascStateHash      ${fixture.ascStateHash}`);
}

main();
