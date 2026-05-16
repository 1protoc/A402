// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {A402VaultSettlement} from "../src/A402VaultSettlement.sol";
import {MockUsdc} from "../src/mocks/MockUsdc.sol";

contract A402VaultSettlementTest is Test {
    A402VaultSettlement internal vault;
    MockUsdc internal usdc;

    address internal submitter = makeAddr("submitter");
    address internal stranger = makeAddr("stranger");
    address internal providerA = makeAddr("providerA");
    address internal providerB = makeAddr("providerB");

    uint256 internal constant ONE_USDC = 1e6;

    event BatchSettled(
        uint256 indexed batchId,
        bytes32 indexed chunkHash,
        address indexed asset,
        bytes32 auditRoot,
        uint256 providerCount,
        uint256 totalAmount
    );

    function setUp() public {
        vault = new A402VaultSettlement(submitter);
        usdc = new MockUsdc();
        usdc.mint(address(vault), 1_000 * ONE_USDC);
    }

    /* -------------------------------------------------------------------------- */
    /*                                Happy paths                                 */
    /* -------------------------------------------------------------------------- */

    function test_settleBatch_singleProviderErc20() public {
        bytes memory packed = _pack(providerA, 100 * ONE_USDC);
        bytes32 chunkHash = keccak256("chunk-1");
        bytes32 auditRoot = keccak256("audit-1");

        vm.expectEmit(true, true, true, true);
        emit BatchSettled(1, chunkHash, address(usdc), auditRoot, 1, 100 * ONE_USDC);

        vm.prank(submitter);
        vault.settleBatch(1, chunkHash, address(usdc), packed, auditRoot);

        assertEq(usdc.balanceOf(providerA), 100 * ONE_USDC);
        assertEq(usdc.balanceOf(address(vault)), 900 * ONE_USDC);
        assertTrue(vault.settledChunks(chunkHash));
    }

    function test_settleBatch_multiProviderErc20() public {
        bytes memory packed = abi.encodePacked(
            _pack(providerA, 60 * ONE_USDC),
            _pack(providerB, 40 * ONE_USDC)
        );
        bytes32 chunkHash = keccak256("chunk-2");

        vm.prank(submitter);
        vault.settleBatch(2, chunkHash, address(usdc), packed, keccak256("audit-2"));

        assertEq(usdc.balanceOf(providerA), 60 * ONE_USDC);
        assertEq(usdc.balanceOf(providerB), 40 * ONE_USDC);
        assertEq(usdc.balanceOf(address(vault)), 900 * ONE_USDC);
    }

    function test_settleBatch_nativeEth() public {
        vm.deal(address(vault), 10 ether);
        uint256 beforeBal = providerA.balance;

        bytes memory packed = _pack(providerA, 1 ether);
        bytes32 chunkHash = keccak256("chunk-eth");

        vm.prank(submitter);
        vault.settleBatch(3, chunkHash, address(0), packed, keccak256("audit-eth"));

        assertEq(providerA.balance - beforeBal, 1 ether);
        assertEq(address(vault).balance, 9 ether);
    }

    /* -------------------------------------------------------------------------- */
    /*                                  Reverts                                   */
    /* -------------------------------------------------------------------------- */

    function test_revert_notSubmitter() public {
        bytes memory packed = _pack(providerA, ONE_USDC);
        vm.expectRevert(A402VaultSettlement.NotSubmitter.selector);
        vm.prank(stranger);
        vault.settleBatch(1, keccak256("c"), address(usdc), packed, bytes32(0));
    }

    function test_revert_emptyBatch() public {
        vm.expectRevert(A402VaultSettlement.EmptyBatch.selector);
        vm.prank(submitter);
        vault.settleBatch(1, keccak256("c"), address(usdc), "", bytes32(0));
    }

    function test_revert_badPackedLength() public {
        bytes memory bad = new bytes(51); // 52 expected
        vm.expectRevert(A402VaultSettlement.BadPackedSettlementLength.selector);
        vm.prank(submitter);
        vault.settleBatch(1, keccak256("c"), address(usdc), bad, bytes32(0));
    }

    function test_revert_zeroProvider() public {
        bytes memory packed = _pack(address(0), ONE_USDC);
        vm.expectRevert(A402VaultSettlement.BadSettlement.selector);
        vm.prank(submitter);
        vault.settleBatch(1, keccak256("c"), address(usdc), packed, bytes32(0));
    }

    function test_revert_zeroAmount() public {
        bytes memory packed = _pack(providerA, 0);
        vm.expectRevert(A402VaultSettlement.BadSettlement.selector);
        vm.prank(submitter);
        vault.settleBatch(1, keccak256("c"), address(usdc), packed, bytes32(0));
    }

    function test_revert_chunkReplay() public {
        bytes memory packed = _pack(providerA, ONE_USDC);
        bytes32 chunkHash = keccak256("chunk-replay");

        vm.prank(submitter);
        vault.settleBatch(1, chunkHash, address(usdc), packed, bytes32(0));

        vm.expectRevert(A402VaultSettlement.ChunkAlreadySettled.selector);
        vm.prank(submitter);
        vault.settleBatch(1, chunkHash, address(usdc), packed, bytes32(0));
    }

    /* -------------------------------------------------------------------------- */
    /*                                  Fuzz                                      */
    /* -------------------------------------------------------------------------- */

    function testFuzz_settleBatch_singleProvider(uint96 amount) public {
        vm.assume(amount > 0 && amount <= 1_000 * ONE_USDC);
        bytes memory packed = _pack(providerA, amount);
        bytes32 chunkHash = keccak256(abi.encodePacked("fuzz", amount));

        vm.prank(submitter);
        vault.settleBatch(uint256(amount), chunkHash, address(usdc), packed, bytes32(0));

        assertEq(usdc.balanceOf(providerA), amount);
    }

    /* -------------------------------------------------------------------------- */
    /*                                 Helpers                                    */
    /* -------------------------------------------------------------------------- */

    /// @dev Packs `provider(20 bytes) || amount(32 bytes)` — matches the
    /// `MultiChainSettlementEntry` layout the enclave submitter encodes in
    /// `enclave/src/multichain_settlement.rs::encode_settle_batch_calldata`.
    function _pack(address provider, uint256 amount) internal pure returns (bytes memory) {
        return abi.encodePacked(provider, amount);
    }
}
