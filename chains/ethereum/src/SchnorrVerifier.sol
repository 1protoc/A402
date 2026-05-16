// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice Secp256k1 Schnorr signature verifier matching the convention in
///         https://github.com/noot/schnorr-verify.
///
///         Verifies `s · G == R + e · P` by exploiting the ecrecover precompile
///         to compute the point operation cheaply. The price we pay is a
///         constrained challenge hash:
///
///             e = keccak256(R_address || 0x1b || px || message)
///
///         where R_address is the Ethereum-address encoding of point R, and
///         the verified public key P satisfies P.x < HALF_Q AND P.y is even
///         (parity byte hard-coded to 27 = 0x1b). Signers normalize their
///         keypair to satisfy these constraints — if `x · G` has odd y, use
///         `(n - x) · G` instead, which shares the same x and has even y.
contract SchnorrVerifier {
    uint256 public constant Q =
        0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141;
    uint256 public constant HALF_Q = (Q >> 1) + 1;

    /// @notice Returns true iff (e, s) is a valid Schnorr signature for `message`
    ///         under pubkey px (with even y).
    function verifySignature(uint256 px, uint256 e, uint256 s, bytes32 message)
        external
        pure
        returns (bool)
    {
        if (px == 0 || px >= HALF_Q) return false;
        if (s == 0 || s >= Q) return false;
        if (e == 0 || e >= Q) return false;

        uint256 sp = Q - mulmod(s, px, Q);
        uint256 ep = Q - mulmod(e, px, Q);
        if (sp == 0 || ep == 0) return false;

        address R = ecrecover(bytes32(sp), 27, bytes32(px), bytes32(ep));
        if (R == address(0)) return false;

        return e == uint256(keccak256(abi.encodePacked(R, uint8(27), px, message)));
    }
}
