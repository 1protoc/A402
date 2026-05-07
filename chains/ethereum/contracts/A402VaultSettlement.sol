// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

interface IERC20 {
    function transfer(address to, uint256 amount) external returns (bool);
}

/// @notice Ethereum settlement leg for the A402/A402 TEE vault.
/// @dev This mirrors the Solana `settle_vault + record_audit` semantics:
///      the enclave-controlled submitter aggregates provider credits, submits
///      one batch commitment, and the contract releases escrowed funds without
///      exposing client identities.
contract A402VaultSettlement {
    error NotSubmitter();
    error EmptyBatch();
    error BadPackedSettlementLength();
    error BadSettlement();
    error ChunkAlreadySettled();
    error TransferFailed();

    event BatchSettled(
        uint256 indexed batchId,
        bytes32 indexed chunkHash,
        address indexed asset,
        bytes32 auditRoot,
        uint256 providerCount,
        uint256 totalAmount
    );

    address public immutable submitter;
    mapping(bytes32 => bool) public settledChunks;

    constructor(address submitter_) {
        if (submitter_ == address(0)) {
            revert BadSettlement();
        }
        submitter = submitter_;
    }

    receive() external payable {}

    /// @param asset ERC-20 token address, or address(0) for native ETH escrow.
    /// @param packedSettlements repeated provider(20 bytes) || amount(32 bytes).
    function settleBatch(
        uint256 batchId,
        bytes32 chunkHash,
        address asset,
        bytes calldata packedSettlements,
        bytes32 auditRoot
    ) external {
        if (msg.sender != submitter) {
            revert NotSubmitter();
        }
        if (packedSettlements.length == 0) {
            revert EmptyBatch();
        }
        if (packedSettlements.length % 52 != 0) {
            revert BadPackedSettlementLength();
        }
        if (settledChunks[chunkHash]) {
            revert ChunkAlreadySettled();
        }

        settledChunks[chunkHash] = true;

        uint256 providerCount = packedSettlements.length / 52;
        uint256 totalAmount = 0;
        for (uint256 offset = 0; offset < packedSettlements.length; offset += 52) {
            address provider;
            uint256 amount;
            assembly {
                provider := shr(96, calldataload(add(packedSettlements.offset, offset)))
                amount := calldataload(add(add(packedSettlements.offset, offset), 20))
            }
            if (provider == address(0) || amount == 0) {
                revert BadSettlement();
            }
            totalAmount += amount;

            if (asset == address(0)) {
                (bool ok, ) = payable(provider).call{value: amount}("");
                if (!ok) {
                    revert TransferFailed();
                }
            } else if (!IERC20(asset).transfer(provider, amount)) {
                revert TransferFailed();
            }
        }

        emit BatchSettled(batchId, chunkHash, asset, auditRoot, providerCount, totalAmount);
    }
}
