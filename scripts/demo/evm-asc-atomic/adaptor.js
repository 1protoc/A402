"use strict";

/**
 * Schnorr adaptor signatures on secp256k1, matching the on-chain verifier in
 * `chains/ethereum/src/SchnorrVerifier.sol` (which itself follows the
 * https://github.com/noot/schnorr-verify convention).
 *
 * Challenge form (so the on-chain ecrecover trick works):
 *
 *     e = keccak256( R_address(20 bytes) || 0x1b || px(32 bytes) || message(32) )
 *
 * Constraints (enforced both off-chain and on-chain):
 *
 *   - px      < HALF_Q                    (BIP-340 style)
 *   - pubkey  has EVEN y-coordinate        (parity byte hard-coded to 27)
 *   - 0 < s   < Q
 *   - 0 < e   < Q
 *
 * If a freshly generated private key produces an odd-y pubkey, we replace
 * the secret with `n - secret`; the resulting pubkey shares the same `px`
 * and has even y. Schnorr's symmetry under negation makes this safe.
 *
 * API (paper §II-B / §IV-B):
 *
 *   pSign(sk, m, T) → σ̂                  pre-sign m under statement T = t·G
 *   pVerify(pk, m, T, σ̂) → bool           check σ̂ completes under any t
 *   adapt(σ̂, t) → σ                       complete the pre-sig given t
 *   extract(σ̂, σ) → t                     recover t from completed sig
 */

const {secp256k1} = require("@noble/curves/secp256k1");
const {keccak_256} = require("@noble/hashes/sha3");
const crypto = require("crypto");

const Q = secp256k1.CURVE.n;
const HALF_Q = (Q >> 1n) + 1n;
const G = secp256k1.ProjectivePoint.BASE;

/* --------------------------------- helpers -------------------------------- */

function modQ(n) {
    let r = n % Q;
    if (r < 0n) r += Q;
    return r;
}

function bytesToBigint(bytes) {
    let n = 0n;
    for (const b of bytes) n = (n << 8n) | BigInt(b);
    return n;
}

function bigintToBytes32(n) {
    const hex = n.toString(16).padStart(64, "0");
    if (hex.length !== 64) throw new Error(`bigintToBytes32: value too large (${hex.length})`);
    return Buffer.from(hex, "hex");
}

function hexToBytes(hex) {
    return Uint8Array.from(Buffer.from(hex.replace(/^0x/, ""), "hex"));
}

function randomScalar() {
    for (;;) {
        const n = bytesToBigint(crypto.randomBytes(32));
        if (n > 0n && n < Q) return n;
    }
}

function scalarMulG(scalar) {
    return G.multiply(typeof scalar === "bigint" ? scalar : bytesToBigint(scalar));
}

function pointToCompressedHex(point) {
    return "0x" + Buffer.from(point.toRawBytes(true)).toString("hex");
}

function pointFromHex(hex) {
    return secp256k1.ProjectivePoint.fromHex(hex.replace(/^0x/, ""));
}

/**
 * Ethereum-address encoding of an EC point: keccak256(uncompressed.x||y)[12:].
 * Used in both the challenge hash and ecrecover's return value.
 */
function pointToAddress(point) {
    const aff = point.toAffine();
    const xb = bigintToBytes32(aff.x);
    const yb = bigintToBytes32(aff.y);
    const concat = new Uint8Array(64);
    concat.set(xb, 0);
    concat.set(yb, 32);
    const hash = keccak_256(concat);
    return Buffer.from(hash.slice(12, 32));
}

/* ---------------------------- key normalization --------------------------- */

/**
 * Treats the input as a 32-byte seed and returns a derived Schnorr keypair
 * `{ priv, pub, px, py }` satisfying:
 *
 *   - py is even (parity byte = 0x1b on-chain)
 *   - px < HALF_Q (the ecrecover trick requires uniqueness in [0, HALF_Q))
 *
 * If the literal seed produces a pubkey violating those constraints, we
 * deterministically re-hash and try again. Almost-all seeds converge within
 * a couple of iterations; the function gives up after 256 attempts.
 */
function normalizeKeypair(seedBytes) {
    let bytes = Uint8Array.from(seedBytes);
    for (let i = 0; i < 256; i++) {
        let x = bytesToBigint(bytes);
        if (x === 0n || x >= Q) {
            bytes = keccak_256(bytes);
            continue;
        }
        let P = scalarMulG(x);
        let aff = P.toAffine();
        if (aff.y % 2n !== 0n) {
            x = Q - x;
            P = scalarMulG(x);
            aff = P.toAffine();
        }
        if (aff.x >= HALF_Q) {
            bytes = keccak_256(bytes);
            continue;
        }
        return {
            privScalar: x,
            priv: bigintToBytes32(x),
            pub: P,
            px: aff.x,
            py: aff.y,
            iterations: i,
        };
    }
    throw new Error("normalizeKeypair: exhausted 256 attempts");
}

/* --------------------------- challenge construction ----------------------- */

/**
 * e = keccak256( R_address(20) || 0x1b || px(32) || message(32) )  mod Q
 */
function challenge(R, px, message) {
    const rAddr = pointToAddress(R);
    const pxBytes = bigintToBytes32(px);
    const msgBytes =
        message instanceof Uint8Array || Buffer.isBuffer(message)
            ? Uint8Array.from(message)
            : hexToBytes(message);
    if (msgBytes.length !== 32) {
        throw new Error(`challenge: message must be 32 bytes, got ${msgBytes.length}`);
    }
    const buf = new Uint8Array(20 + 1 + 32 + 32);
    buf.set(rAddr, 0);
    buf[20] = 0x1b; // parity = 27
    buf.set(pxBytes, 21);
    buf.set(msgBytes, 53);
    const h = keccak_256(buf);
    const e = modQ(bytesToBigint(h));
    if (e === 0n) throw new Error("challenge: e == 0, retry");
    return e;
}

/* ---------------------------------- API ----------------------------------- */

/**
 * pSign(sk, msg, T):
 *   sample r'
 *   R'  = r' · G
 *   R   = R' + T
 *   e   = H(R || P || msg)
 *   s'  = r' + e · sk
 *   return (R', s')
 *
 * The signer must use a NORMALIZED key (px < HALF_Q, py even).
 */
function pSign(normalizedPriv, msg, T) {
    const x = typeof normalizedPriv === "bigint" ? normalizedPriv : bytesToBigint(normalizedPriv);
    if (x === 0n || x >= Q) throw new Error("invalid private key");
    const P = scalarMulG(x);
    const aff = P.toAffine();
    if (aff.y % 2n !== 0n || aff.x >= HALF_Q) {
        throw new Error("pSign: pubkey not normalized (need even y and px < HALF_Q)");
    }

    for (;;) {
        const rPrime = randomScalar();
        const RPrime = scalarMulG(rPrime);
        const R = RPrime.add(T);
        // R must also be non-zero
        if (R.equals(secp256k1.ProjectivePoint.ZERO)) continue;

        let e;
        try {
            e = challenge(R, aff.x, msg);
        } catch (_) {
            continue;
        }
        const sPrime = modQ(rPrime + e * x);
        if (sPrime === 0n) continue;

        return {
            RPrime: pointToCompressedHex(RPrime),
            sPrimeHex: "0x" + Buffer.from(bigintToBytes32(sPrime)).toString("hex"),
            // Cache px so consumers don't have to re-derive
            px: "0x" + bigintToBytes32(aff.x).toString("hex"),
        };
    }
}

function pVerify(normalizedPub, msg, T, sigHat) {
    const aff = normalizedPub.toAffine();
    if (aff.y % 2n !== 0n || aff.x >= HALF_Q) return false;

    const RPrime = pointFromHex(sigHat.RPrime);
    const sPrime = bytesToBigint(hexToBytes(sigHat.sPrimeHex));
    if (sPrime === 0n || sPrime >= Q) return false;

    const R = RPrime.add(T);
    if (R.equals(secp256k1.ProjectivePoint.ZERO)) return false;

    let e;
    try {
        e = challenge(R, aff.x, msg);
    } catch (_) {
        return false;
    }

    const lhs = scalarMulG(sPrime);
    const rhs = RPrime.add(normalizedPub.multiply(e));
    return lhs.equals(rhs);
}

function adapt(sigHat, t) {
    const RPrime = pointFromHex(sigHat.RPrime);
    const sPrime = bytesToBigint(hexToBytes(sigHat.sPrimeHex));
    const tScalar = typeof t === "bigint" ? t : bytesToBigint(t);
    const R = RPrime.add(scalarMulG(tScalar));
    const s = modQ(sPrime + tScalar);
    return {
        R: pointToCompressedHex(R),
        sHex: "0x" + Buffer.from(bigintToBytes32(s)).toString("hex"),
        // Convenience: package the form the on-chain verifier expects
        toOnchain(px, msg) {
            const eScalar = challenge(R, BigInt(px), msg);
            return {
                px: "0x" + bigintToBytes32(BigInt(px)).toString("hex"),
                e: "0x" + bigintToBytes32(eScalar).toString("hex"),
                s: "0x" + Buffer.from(bigintToBytes32(s)).toString("hex"),
            };
        },
    };
}

function extract(sigHat, sigFull) {
    const sPrime = bytesToBigint(hexToBytes(sigHat.sPrimeHex));
    const s = bytesToBigint(hexToBytes(sigFull.sHex));
    return modQ(s - sPrime);
}

function verifyFull(normalizedPub, msg, sigFull) {
    const aff = normalizedPub.toAffine();
    if (aff.y % 2n !== 0n || aff.x >= HALF_Q) return false;

    const R = pointFromHex(sigFull.R);
    const s = bytesToBigint(hexToBytes(sigFull.sHex));
    if (s === 0n || s >= Q) return false;

    let e;
    try {
        e = challenge(R, aff.x, msg);
    } catch (_) {
        return false;
    }

    const lhs = scalarMulG(s);
    const rhs = R.add(normalizedPub.multiply(e));
    return lhs.equals(rhs);
}

/* ----------------------------- encryption shim --------------------------- */

function deriveSymKey(tScalar) {
    const tBytes = bigintToBytes32(typeof tScalar === "bigint" ? tScalar : bytesToBigint(tScalar));
    const tagged = new Uint8Array(21 + 32);
    tagged.set(Buffer.from("a402-atomic-result-v1", "utf8"), 0);
    tagged.set(tBytes, 21);
    return Buffer.from(keccak_256(tagged));
}

function encryptResult(plaintext, tScalar) {
    const key = deriveSymKey(tScalar);
    const iv = crypto.randomBytes(12);
    const cipher = crypto.createCipheriv("aes-256-gcm", key, iv);
    const ciphertext = Buffer.concat([cipher.update(Buffer.from(plaintext, "utf8")), cipher.final()]);
    const tag = cipher.getAuthTag();
    return {
        iv: "0x" + iv.toString("hex"),
        ciphertext: "0x" + ciphertext.toString("hex"),
        tag: "0x" + tag.toString("hex"),
    };
}

function decryptResult(encrypted, tScalar) {
    const key = deriveSymKey(tScalar);
    const iv = Buffer.from(encrypted.iv.replace(/^0x/, ""), "hex");
    const ciphertext = Buffer.from(encrypted.ciphertext.replace(/^0x/, ""), "hex");
    const tag = Buffer.from(encrypted.tag.replace(/^0x/, ""), "hex");
    const decipher = crypto.createDecipheriv("aes-256-gcm", key, iv);
    decipher.setAuthTag(tag);
    return Buffer.concat([decipher.update(ciphertext), decipher.final()]).toString("utf8");
}

module.exports = {
    Q,
    HALF_Q,
    G,
    pSign,
    pVerify,
    adapt,
    extract,
    verifyFull,
    randomScalar,
    scalarMulG,
    pointFromHex,
    pointToCompressedHex,
    pointToAddress,
    normalizeKeypair,
    challenge,
    encryptResult,
    decryptResult,
    deriveSymKey,
    bigintToBytes32,
    bytesToBigint,
};
