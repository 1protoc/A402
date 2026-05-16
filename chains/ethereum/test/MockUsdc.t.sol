// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {MockUsdc} from "../src/mocks/MockUsdc.sol";

contract MockUsdcTest is Test {
    MockUsdc internal usdc;

    uint256 internal alicePrivateKey = 0xA11CE;
    uint256 internal mallotyPrivateKey = 0xBADBAD;

    address internal alice;
    address internal bob = makeAddr("bob");
    address internal mallory;

    uint256 internal constant ONE_USDC = 1e6;

    bytes32 internal constant TRANSFER_TYPEHASH = keccak256(
        "TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)"
    );

    function setUp() public {
        usdc = new MockUsdc();
        alice = vm.addr(alicePrivateKey);
        mallory = vm.addr(mallotyPrivateKey);
        usdc.mint(alice, 100 * ONE_USDC);
    }

    /* -------------------------------------------------------------------------- */
    /*                              EIP-712 sanity                                */
    /* -------------------------------------------------------------------------- */

    function test_typehash_matches_constant() public view {
        assertEq(usdc.TRANSFER_WITH_AUTHORIZATION_TYPEHASH(), TRANSFER_TYPEHASH);
    }

    function test_domainSeparator_includesChainId() public view {
        bytes32 expected = keccak256(
            abi.encode(
                keccak256(
                    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
                ),
                keccak256(bytes(usdc.name())),
                keccak256(bytes(usdc.version())),
                block.chainid,
                address(usdc)
            )
        );
        assertEq(usdc.DOMAIN_SEPARATOR(), expected);
    }

    /* -------------------------------------------------------------------------- */
    /*                       transferWithAuthorization happy path                 */
    /* -------------------------------------------------------------------------- */

    function test_transferWithAuthorization_happyPath() public {
        uint256 value = 10 * ONE_USDC;
        uint256 validAfter = 0;
        uint256 validBefore = block.timestamp + 1 hours;
        bytes32 nonce = keccak256("nonce-1");

        (uint8 v, bytes32 r, bytes32 s) =
            _signTransfer(alicePrivateKey, alice, bob, value, validAfter, validBefore, nonce);

        // Anyone can submit the signed authorization — relayer pays the gas.
        address relayer = makeAddr("relayer");
        vm.prank(relayer);
        usdc.transferWithAuthorization(alice, bob, value, validAfter, validBefore, nonce, v, r, s);

        assertEq(usdc.balanceOf(alice), 90 * ONE_USDC);
        assertEq(usdc.balanceOf(bob), value);
        assertTrue(usdc.authorizationState(alice, nonce));
    }

    /* -------------------------------------------------------------------------- */
    /*                                  Reverts                                   */
    /* -------------------------------------------------------------------------- */

    function test_revert_notYetValid() public {
        // validAfter in the future
        uint256 validAfter = block.timestamp + 100;
        uint256 validBefore = block.timestamp + 1 hours;
        bytes32 nonce = keccak256("not-yet");
        (uint8 v, bytes32 r, bytes32 s) =
            _signTransfer(alicePrivateKey, alice, bob, ONE_USDC, validAfter, validBefore, nonce);

        vm.expectRevert(MockUsdc.AuthorizationNotYetValid.selector);
        usdc.transferWithAuthorization(alice, bob, ONE_USDC, validAfter, validBefore, nonce, v, r, s);
    }

    function test_revert_expired() public {
        vm.warp(1_000_000);
        uint256 validBefore = block.timestamp - 1;
        bytes32 nonce = keccak256("expired");
        (uint8 v, bytes32 r, bytes32 s) =
            _signTransfer(alicePrivateKey, alice, bob, ONE_USDC, 0, validBefore, nonce);

        vm.expectRevert(MockUsdc.AuthorizationExpired.selector);
        usdc.transferWithAuthorization(alice, bob, ONE_USDC, 0, validBefore, nonce, v, r, s);
    }

    function test_revert_replay() public {
        uint256 validBefore = block.timestamp + 1 hours;
        bytes32 nonce = keccak256("replay");
        (uint8 v, bytes32 r, bytes32 s) =
            _signTransfer(alicePrivateKey, alice, bob, ONE_USDC, 0, validBefore, nonce);

        usdc.transferWithAuthorization(alice, bob, ONE_USDC, 0, validBefore, nonce, v, r, s);

        vm.expectRevert(MockUsdc.AuthorizationAlreadyUsed.selector);
        usdc.transferWithAuthorization(alice, bob, ONE_USDC, 0, validBefore, nonce, v, r, s);
    }

    function test_revert_wrongSigner() public {
        uint256 validBefore = block.timestamp + 1 hours;
        bytes32 nonce = keccak256("wrong-signer");
        // mallory signs but claims the from is alice
        (uint8 v, bytes32 r, bytes32 s) =
            _signTransfer(mallotyPrivateKey, alice, bob, ONE_USDC, 0, validBefore, nonce);

        vm.expectRevert(MockUsdc.InvalidSignature.selector);
        usdc.transferWithAuthorization(alice, bob, ONE_USDC, 0, validBefore, nonce, v, r, s);
    }

    function test_revert_tamperedValue() public {
        uint256 validBefore = block.timestamp + 1 hours;
        bytes32 nonce = keccak256("tampered");
        // Sign for 1 USDC, submit for 50 USDC
        (uint8 v, bytes32 r, bytes32 s) =
            _signTransfer(alicePrivateKey, alice, bob, ONE_USDC, 0, validBefore, nonce);

        vm.expectRevert(MockUsdc.InvalidSignature.selector);
        usdc.transferWithAuthorization(alice, bob, 50 * ONE_USDC, 0, validBefore, nonce, v, r, s);
    }

    /* -------------------------------------------------------------------------- */
    /*                              cancelAuthorization                           */
    /* -------------------------------------------------------------------------- */

    function test_cancel_then_replay_fails() public {
        uint256 validBefore = block.timestamp + 1 hours;
        bytes32 nonce = keccak256("cancel-me");

        // First, sign a cancel
        bytes32 cancelStructHash =
            keccak256(abi.encode(usdc.CANCEL_AUTHORIZATION_TYPEHASH(), alice, nonce));
        bytes32 cancelDigest =
            keccak256(abi.encodePacked("\x19\x01", usdc.DOMAIN_SEPARATOR(), cancelStructHash));
        (uint8 cv, bytes32 cr, bytes32 cs) = vm.sign(alicePrivateKey, cancelDigest);

        usdc.cancelAuthorization(alice, nonce, cv, cr, cs);
        assertTrue(usdc.authorizationState(alice, nonce));

        // Now try to use that same nonce for a transfer — must fail
        (uint8 v, bytes32 r, bytes32 s) =
            _signTransfer(alicePrivateKey, alice, bob, ONE_USDC, 0, validBefore, nonce);
        vm.expectRevert(MockUsdc.AuthorizationAlreadyUsed.selector);
        usdc.transferWithAuthorization(alice, bob, ONE_USDC, 0, validBefore, nonce, v, r, s);
    }

    /* -------------------------------------------------------------------------- */
    /*                                  Helpers                                   */
    /* -------------------------------------------------------------------------- */

    function _signTransfer(
        uint256 privateKey,
        address from,
        address to,
        uint256 value,
        uint256 validAfter,
        uint256 validBefore,
        bytes32 nonce
    ) internal view returns (uint8 v, bytes32 r, bytes32 s) {
        bytes32 structHash = keccak256(
            abi.encode(TRANSFER_TYPEHASH, from, to, value, validAfter, validBefore, nonce)
        );
        bytes32 digest =
            keccak256(abi.encodePacked("\x19\x01", usdc.DOMAIN_SEPARATOR(), structHash));
        (v, r, s) = vm.sign(privateKey, digest);
    }
}
