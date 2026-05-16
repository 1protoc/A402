// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {ASCManager} from "../src/ASCManager.sol";
import {MockUsdc} from "../src/mocks/MockUsdc.sol";

/// @notice Covers Standard ASC mode: createASC / closeASC and
/// the surrounding signature + balance invariants. Force-close paths are
/// covered separately because they need block-time manipulation.
contract ASCManagerTest is Test {
    ASCManager internal manager;
    MockUsdc internal usdc;

    uint256 internal vaultPk = 0xCAFE;
    uint256 internal clientPk = 0xA11CE;
    uint256 internal providerPk = 0xB0B;

    address internal vault;
    address internal client;
    address internal provider;

    uint256 internal constant ONE_USDC = 1e6;
    uint256 internal constant DEPOSIT = 100 * ONE_USDC;

    function setUp() public {
        vault = vm.addr(vaultPk);
        client = vm.addr(clientPk);
        provider = vm.addr(providerPk);

        usdc = new MockUsdc();
        manager = new ASCManager(address(usdc), vault, address(0));

        usdc.mint(client, DEPOSIT);
        vm.prank(client);
        usdc.approve(address(manager), type(uint256).max);
    }

    /* -------------------------------------------------------------------------- */
    /*                                createASC                                   */
    /* -------------------------------------------------------------------------- */

    function test_createASC_happyPath() public {
        bytes32 cid = keccak256("channel-1");

        vm.prank(vault);
        manager.createASC(cid, client, provider, DEPOSIT);

        (
            address storedClient,
            address storedProvider,
            uint256 balanceC,
            uint256 balanceS,
            uint256 version,
            ASCManager.Status status,
            ,
            uint256 totalDeposit
        ) = manager.ascs(cid);

        assertEq(storedClient, client);
        assertEq(storedProvider, provider);
        assertEq(balanceC, DEPOSIT);
        assertEq(balanceS, 0);
        assertEq(version, 0);
        assertEq(uint8(status), uint8(ASCManager.Status.OPEN));
        assertEq(totalDeposit, DEPOSIT);

        assertEq(usdc.balanceOf(address(manager)), DEPOSIT);
        assertEq(usdc.balanceOf(client), 0);
    }

    function test_createASC_revert_notVault() public {
        bytes32 cid = keccak256("channel-stranger");
        vm.expectRevert(ASCManager.NotVault.selector);
        manager.createASC(cid, client, provider, DEPOSIT);
    }

    function test_createASC_revert_duplicateCid() public {
        bytes32 cid = keccak256("channel-dup");
        vm.prank(vault);
        manager.createASC(cid, client, provider, DEPOSIT);

        // Top up the client so the second attempt has funds to (try to) pull.
        usdc.mint(client, DEPOSIT);
        vm.prank(vault);
        vm.expectRevert(ASCManager.InvalidState.selector);
        manager.createASC(cid, client, provider, DEPOSIT);
    }

    function test_createASC_revert_zeroAmount() public {
        bytes32 cid = keccak256("channel-zero");
        vm.prank(vault);
        vm.expectRevert(ASCManager.InvalidState.selector);
        manager.createASC(cid, client, provider, 0);
    }

    /* -------------------------------------------------------------------------- */
    /*                                 closeASC                                   */
    /* -------------------------------------------------------------------------- */

    function test_closeASC_happyPath_paysOutBalances() public {
        bytes32 cid = _open();

        // Simulate 30 off-chain micropayments accumulating into balanceS.
        uint256 spent = 30 * ONE_USDC;
        uint256 balanceC = DEPOSIT - spent;
        uint256 balanceS = spent;
        uint256 version = 30;

        bytes32 digest = manager.ascStateHash(cid, balanceC, balanceS, version);
        bytes memory sigC = _sign(clientPk, digest);
        bytes memory sigP = _sign(providerPk, digest);

        vm.prank(vault);
        manager.closeASC(cid, balanceC, balanceS, version, sigC, sigP);

        assertEq(usdc.balanceOf(client), balanceC);
        assertEq(usdc.balanceOf(provider), balanceS);
        assertEq(usdc.balanceOf(address(manager)), 0);

        (, , uint256 storedC, uint256 storedS, uint256 storedV, ASCManager.Status status, , ) =
            manager.ascs(cid);
        assertEq(storedC, balanceC);
        assertEq(storedS, balanceS);
        assertEq(storedV, version);
        assertEq(uint8(status), uint8(ASCManager.Status.CLOSED));
    }

    function test_closeASC_revert_notVault() public {
        bytes32 cid = _open();
        uint256 v = 1;
        bytes32 digest = manager.ascStateHash(cid, DEPOSIT - 1, 1, v);
        bytes memory sigC = _sign(clientPk, digest);
        bytes memory sigP = _sign(providerPk, digest);

        vm.expectRevert(ASCManager.NotVault.selector);
        manager.closeASC(cid, DEPOSIT - 1, 1, v, sigC, sigP);
    }

    function test_closeASC_revert_unbalanced() public {
        bytes32 cid = _open();
        // balanceC + balanceS != totalDeposit
        uint256 badC = DEPOSIT - 1;
        uint256 badS = 2; // sum is DEPOSIT + 1
        bytes32 digest = manager.ascStateHash(cid, badC, badS, 1);
        bytes memory sigC = _sign(clientPk, digest);
        bytes memory sigP = _sign(providerPk, digest);

        vm.prank(vault);
        vm.expectRevert(ASCManager.InvalidAmount.selector);
        manager.closeASC(cid, badC, badS, 1, sigC, sigP);
    }

    function test_closeASC_revert_versionNotIncrementing() public {
        bytes32 cid = _open();
        uint256 version = 0; // must be > asc.version (which is 0 at open)
        bytes32 digest = manager.ascStateHash(cid, DEPOSIT - 1, 1, version);
        bytes memory sigC = _sign(clientPk, digest);
        bytes memory sigP = _sign(providerPk, digest);

        vm.prank(vault);
        vm.expectRevert(ASCManager.InvalidState.selector);
        manager.closeASC(cid, DEPOSIT - 1, 1, version, sigC, sigP);
    }

    function test_closeASC_revert_wrongClientSig() public {
        bytes32 cid = _open();
        uint256 version = 1;
        bytes32 digest = manager.ascStateHash(cid, DEPOSIT - 1, 1, version);
        // mallory pretends to sign for client
        uint256 malloryPk = 0xBADBAD;
        bytes memory sigC = _sign(malloryPk, digest);
        bytes memory sigP = _sign(providerPk, digest);

        vm.prank(vault);
        vm.expectRevert(ASCManager.InvalidSignature.selector);
        manager.closeASC(cid, DEPOSIT - 1, 1, version, sigC, sigP);
    }

    function test_closeASC_revert_wrongProviderSig() public {
        bytes32 cid = _open();
        uint256 version = 1;
        bytes32 digest = manager.ascStateHash(cid, DEPOSIT - 1, 1, version);
        bytes memory sigC = _sign(clientPk, digest);
        bytes memory sigP = _sign(clientPk, digest); // client signs both — wrong

        vm.prank(vault);
        vm.expectRevert(ASCManager.InvalidSignature.selector);
        manager.closeASC(cid, DEPOSIT - 1, 1, version, sigC, sigP);
    }

    function test_closeASC_revert_tamperedBalance() public {
        bytes32 cid = _open();
        // Sign for (90, 10) but submit (50, 50). Balance conservation holds
        // (both sum to DEPOSIT) but the digest no longer matches the signature.
        bytes32 digest = manager.ascStateHash(cid, 90 * ONE_USDC, 10 * ONE_USDC, 1);
        bytes memory sigC = _sign(clientPk, digest);
        bytes memory sigP = _sign(providerPk, digest);

        vm.prank(vault);
        vm.expectRevert(ASCManager.InvalidSignature.selector);
        manager.closeASC(cid, 50 * ONE_USDC, 50 * ONE_USDC, 1, sigC, sigP);
    }

    /* -------------------------------------------------------------------------- */
    /*                       Invariant: balance conservation                      */
    /* -------------------------------------------------------------------------- */

    function testFuzz_closeASC_balanceConservation(uint96 spent96) public {
        uint256 spent = uint256(spent96) % (DEPOSIT + 1);
        bytes32 cid = _open();

        uint256 balanceC = DEPOSIT - spent;
        uint256 balanceS = spent;
        uint256 version = 1;
        bytes32 digest = manager.ascStateHash(cid, balanceC, balanceS, version);
        bytes memory sigC = _sign(clientPk, digest);
        bytes memory sigP = _sign(providerPk, digest);

        vm.prank(vault);
        manager.closeASC(cid, balanceC, balanceS, version, sigC, sigP);

        assertEq(usdc.balanceOf(client) + usdc.balanceOf(provider), DEPOSIT);
        assertEq(usdc.balanceOf(address(manager)), 0);
    }

    /* -------------------------------------------------------------------------- */
    /*                                 Helpers                                    */
    /* -------------------------------------------------------------------------- */

    function _open() internal returns (bytes32 cid) {
        cid = keccak256(abi.encodePacked("ch-", block.number, block.timestamp));
        vm.prank(vault);
        manager.createASC(cid, client, provider, DEPOSIT);
    }

    /// @dev Produces an Ethereum-prefixed signature, matching what the
    ///      contract's `_signedBy` expects (`toEthSignedMessageHash`).
    function _sign(uint256 pk, bytes32 digest) internal pure returns (bytes memory) {
        bytes32 ethDigest = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", digest));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, ethDigest);
        return abi.encodePacked(r, s, v);
    }
}
