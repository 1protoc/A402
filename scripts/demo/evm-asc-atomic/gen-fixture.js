#!/usr/bin/env node
"use strict";

/**
 * Produces a deterministic cross-stack fixture for the secp256k1 Schnorr
 * adaptor signature scheme.
 *
 * The fixture pins:
 *   - signer seed → normalized keypair (px, secret)
 *   - witness scalar t           (and T = t·G)
 *   - message digest             (32 bytes)
 *   - pre-signature σ̂_S = (R', s') produced by p_sign
 *   - adapted signature σ_S = (R, s) produced by adapt(σ̂_S, t)
 *   - on-chain proof tuple (px, e, s) produced by toOnchain(px, msg)
 *
 * The Rust `adaptor_sig_secp::cross_stack` test consumes this file and
 * verifies that:
 *   - p_verify  accepts σ̂_S         (same math both sides)
 *   - adapt(σ̂_S, t) reproduces       σ_S byte-for-byte
 *   - extract(σ̂_S, σ_S)              recovers exactly t
 *   - verify_full accepts σ_S        (same math both sides)
 *   - build_onchain_proof reproduces (px, e, s) byte-for-byte
 *
 * If either side changes the challenge format or the byte layout, this
 * fixture's assertions break — which is exactly what we want.
 */

const fs = require("fs");
const path = require("path");

const {
    pSign,
    pVerify,
    adapt,
    extract,
    verifyFull,
    normalizeKeypair,
    scalarMulG,
    bytesToBigint,
    bigintToBytes32,
} = require("./adaptor");

function hex(buf) {
    const b = buf instanceof Uint8Array ? Buffer.from(buf) : Buffer.from(buf);
    return "0x" + b.toString("hex");
}

function generate() {
    // Fixed seeds make the fixture reproducible across runs.
    const seed = Buffer.from(
        "a402-cross-stack-seed-0000000000000000000000000000000000000000",
        "utf8"
    ).subarray(0, 32);
    const t_seed = Buffer.from(
        "a402-cross-stack-witness-tttttttttttttttttttttttttttttttt",
        "utf8"
    ).subarray(0, 32);
    const msg = Buffer.from(
        "a402-cross-stack-message-mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm",
        "utf8"
    ).subarray(0, 32);

    const kp = normalizeKeypair(seed);
    const t = bytesToBigint(t_seed);
    const T = scalarMulG(t);

    // p_sign is randomized (r' is sampled inside), so we run it once and pin
    // the resulting σ̂_S as the canonical pre-sig for the rest of the chain.
    const sigHat = pSign(kp.priv, msg, T);

    // Sanity: pVerify accepts what we just produced.
    if (!pVerify(kp.pub, msg, T, sigHat)) {
        throw new Error("p_sign produced a σ̂_S that p_verify rejected");
    }

    const full = adapt(sigHat, t);
    if (!verifyFull(kp.pub, msg, full)) {
        throw new Error("adapted σ_S failed verify_full");
    }
    const recoveredT = extract(sigHat, full);
    if (recoveredT !== t) {
        throw new Error("extract(σ̂, σ) did not recover the original t");
    }

    const onchain = full.toOnchain(kp.px, msg);

    return {
        comment: "deterministic cross-stack fixture for adaptor_sig_secp; do not hand-edit",
        seed: hex(seed),
        message: hex(msg),
        signer: {
            secret: "0x" + Buffer.from(kp.priv).toString("hex"),
            px: "0x" + bigintToBytes32(kp.px).toString("hex"),
            py: "0x" + bigintToBytes32(kp.py).toString("hex"),
            iterations: kp.iterations,
        },
        witness: {
            t: "0x" + bigintToBytes32(t).toString("hex"),
            T_compressed:
                "0x" + Buffer.from(T.toRawBytes(true)).toString("hex"),
        },
        pre_signature: {
            r_prime_compressed: sigHat.RPrime,
            s_prime: sigHat.sPrimeHex,
        },
        full_signature: {
            r_compressed: full.R,
            s: full.sHex,
        },
        onchain_proof: {
            px: onchain.px,
            e: onchain.e,
            s: onchain.s,
        },
    };
}

function main() {
    const outDir = path.resolve(__dirname, "..", "..", "..", "tests", "fixtures");
    fs.mkdirSync(outDir, {recursive: true});
    const outPath = path.join(outDir, "adaptor_sig_secp_fixture.json");

    const fixture = generate();
    fs.writeFileSync(outPath, JSON.stringify(fixture, null, 2) + "\n");

    console.log(`wrote ${outPath}`);
    console.log("  px =", fixture.signer.px);
    console.log("  T  =", fixture.witness.T_compressed);
    console.log("  R' =", fixture.pre_signature.r_prime_compressed);
    console.log("  s' =", fixture.pre_signature.s_prime);
    console.log("  R  =", fixture.full_signature.r_compressed);
    console.log("  s  =", fixture.full_signature.s);
    console.log("  e  =", fixture.onchain_proof.e);
}

main();
