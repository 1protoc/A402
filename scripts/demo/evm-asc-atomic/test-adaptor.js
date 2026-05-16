#!/usr/bin/env node
"use strict";

/**
 * Round-trip tests for the Schnorr adaptor signature primitives, matching the
 * noot-style challenge format used by the on-chain SchnorrVerifier.
 */

const assert = require("assert");
const crypto = require("crypto");
const {
    pSign,
    pVerify,
    adapt,
    extract,
    verifyFull,
    randomScalar,
    scalarMulG,
    normalizeKeypair,
    bigintToBytes32,
    encryptResult,
    decryptResult,
    challenge,
} = require("./adaptor");

const {keccak_256} = require("@noble/hashes/sha3");

function randPriv() {
    return crypto.randomBytes(32);
}

function runCase(name, fn) {
    try {
        fn();
        console.log(`  ok  ${name}`);
    } catch (err) {
        console.error(`  FAIL ${name}: ${err.message}`);
        process.exitCode = 1;
    }
}

console.log("Schnorr adaptor signature round-trip (noot-format):");

runCase("normalizeKeypair returns even-y / px < HALF_Q", () => {
    for (let i = 0; i < 10; i++) {
        const kp = normalizeKeypair(randPriv());
        assert.strictEqual(kp.py % 2n, 0n, "py must be even after normalization");
        const halfQ = require("./adaptor").HALF_Q;
        assert.ok(kp.px < halfQ, "px must be < HALF_Q");
    }
});

runCase("pSign / pVerify accept honest pre-sig", () => {
    const kp = normalizeKeypair(randPriv());
    const t = randomScalar();
    const T = scalarMulG(t);
    const msg = keccak_256(Buffer.from("hello", "utf8"));

    const sigHat = pSign(kp.priv, msg, T);
    assert.ok(pVerify(kp.pub, msg, T, sigHat), "pVerify should accept honest pre-sig");
});

runCase("pVerify rejects pre-sig under wrong T", () => {
    const kp = normalizeKeypair(randPriv());
    const T = scalarMulG(randomScalar());
    const Tbad = scalarMulG(randomScalar());
    const msg = keccak_256(Buffer.from("rejects-wrong-T", "utf8"));
    const sigHat = pSign(kp.priv, msg, T);
    assert.ok(!pVerify(kp.pub, msg, Tbad, sigHat), "pVerify must reject mismatched T");
});

runCase("adapt produces a sig that verifies under the same pk", () => {
    const kp = normalizeKeypair(randPriv());
    const t = randomScalar();
    const T = scalarMulG(t);
    const msg = keccak_256(Buffer.from("adapt-then-verify", "utf8"));
    const sigHat = pSign(kp.priv, msg, T);
    const full = adapt(sigHat, t);
    assert.ok(verifyFull(kp.pub, msg, full), "adapted sig must verify");
});

runCase("extract(σ̂, σ) recovers exactly t", () => {
    const kp = normalizeKeypair(randPriv());
    const t = randomScalar();
    const T = scalarMulG(t);
    const msg = keccak_256(Buffer.from("extract-back", "utf8"));
    const sigHat = pSign(kp.priv, msg, T);
    const full = adapt(sigHat, t);
    const recovered = extract(sigHat, full);
    assert.strictEqual(recovered, t, "extracted scalar must equal original t");
});

runCase("adapt(...).toOnchain matches the recomputed challenge", () => {
    const kp = normalizeKeypair(randPriv());
    const t = randomScalar();
    const T = scalarMulG(t);
    const msg = keccak_256(Buffer.from("onchain-pack", "utf8"));
    const sigHat = pSign(kp.priv, msg, T);
    const full = adapt(sigHat, t);
    const onchain = full.toOnchain(kp.px, msg);
    // The on-chain verifier checks e == H(R||0x1b||px||msg). We recompute that
    // here using the JS challenge() helper and compare.
    const Rpoint = require("./adaptor").pointFromHex(full.R);
    const eExpected = challenge(Rpoint, kp.px, msg);
    assert.strictEqual(BigInt(onchain.e), eExpected, "e in on-chain pack must match challenge()");
});

runCase("encryptResult / decryptResult round-trip", () => {
    const t = randomScalar();
    const plaintext = JSON.stringify({weather: "clear", temp: 72});
    const ct = encryptResult(plaintext, t);
    assert.strictEqual(decryptResult(ct, t), plaintext);
});

runCase("decryptResult fails with wrong t", () => {
    const t = randomScalar();
    const tBad = randomScalar();
    const ct = encryptResult("secret", t);
    let threw = false;
    try {
        decryptResult(ct, tBad);
    } catch (_) {
        threw = true;
    }
    assert.ok(threw, "decryptResult must reject mismatched key");
});

if (process.exitCode) {
    console.log("\nFAIL");
    process.exit(process.exitCode);
}
console.log("\nAll adaptor signature tests passed.");
