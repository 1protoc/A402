// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

interface IERC20Like {
    function transfer(address to, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
}

interface ISchnorrVerifier {
    function verifySignature(
        uint256 px,
        uint256 e,
        uint256 s,
        bytes32 message
    ) external view returns (bool);
}

/// @notice A402 Ethereum settlement contract.
/// @dev Mirrors the paper/Solana architecture: ASC lifecycle, liquidity vault,
///      and Schnorr/adaptor verification hooks. The Nitro vault is the normal
///      cooperative operator; participant force paths exist for vault downtime.
contract ASCManager {
    enum Status {
        OPEN,
        CLOSING,
        CLOSED
    }

    struct ASCState {
        address client;
        address provider;
        uint256 balanceC;
        uint256 balanceS;
        uint256 version;
        Status status;
        uint256 createdAt;
        uint256 totalDeposit;
    }

    struct ForceCloseRequest {
        uint256 balanceC;
        uint256 balanceS;
        uint256 version;
        uint256 submitTime;
        bool exists;
    }

    struct VaultAccount {
        uint256 balance;
        bool initialized;
    }

    struct ForceSettleRequest {
        uint256 amount;
        uint256 submitTime;
        bool exists;
    }

    error NotVault();
    error NotParticipant();
    error InvalidState();
    error InvalidAmount();
    error InvalidSignature();
    error DisputeWindowActive();
    error DisputeWindowExpired();
    error AlreadyUsedChunk();
    error TransferFailed();

    uint256 public constant DISPUTE_WINDOW = 24 hours;
    uint256 public constant SETTLE_DISPUTE_WINDOW = 48 hours;

    IERC20Like public immutable paymentToken;
    address public immutable vault;
    ISchnorrVerifier public immutable schnorrVerifier;

    mapping(bytes32 => ASCState) public ascs;
    mapping(bytes32 => ForceCloseRequest) public forceCloseReqs;
    mapping(address => VaultAccount) public vaultAccounts;
    mapping(address => ForceSettleRequest) public forceSettleReqs;
    mapping(bytes32 => bool) public settledChunks;

    event ASCCreated(bytes32 indexed cid, address indexed client, address indexed provider, uint256 amount);
    event ASCClosed(bytes32 indexed cid, uint256 balanceC, uint256 balanceS, uint256 version);
    event ForceCloseInitiated(bytes32 indexed cid, uint256 version, uint256 deadline);
    event ForceCloseChallenged(bytes32 indexed cid, uint256 version);
    event ForceClosed(bytes32 indexed cid, uint256 balanceC, uint256 balanceS, uint256 version);
    event VaultInitialized(address indexed participant, uint256 amount);
    event VaultSettled(address indexed participant, uint256 amount);
    event VaultBatchSettled(uint256 indexed batchId, bytes32 indexed chunkHash, uint256 count, uint256 totalAmount);
    event ForceSettleInitiated(address indexed participant, uint256 amount, uint256 deadline);
    event ForceSettleFinalized(address indexed participant, uint256 amount);

    constructor(address paymentToken_, address vault_, address schnorrVerifier_) {
        if (paymentToken_ == address(0) || vault_ == address(0)) {
            revert InvalidState();
        }
        paymentToken = IERC20Like(paymentToken_);
        vault = vault_;
        schnorrVerifier = ISchnorrVerifier(schnorrVerifier_);
    }

    modifier onlyVault() {
        if (msg.sender != vault) {
            revert NotVault();
        }
        _;
    }

    // ── Module 1: Standard ASC ──

    function createASC(bytes32 cid, address client, address provider, uint256 amount) external onlyVault {
        if (ascs[cid].createdAt != 0 || client == address(0) || provider == address(0) || amount == 0) {
            revert InvalidState();
        }
        _pull(client, amount);
        ascs[cid] = ASCState({
            client: client,
            provider: provider,
            balanceC: amount,
            balanceS: 0,
            version: 0,
            status: Status.OPEN,
            createdAt: block.timestamp,
            totalDeposit: amount
        });
        emit ASCCreated(cid, client, provider, amount);
    }

    function closeASC(
        bytes32 cid,
        uint256 balanceC,
        uint256 balanceS,
        uint256 version,
        bytes calldata sigC,
        bytes calldata sigS
    ) external onlyVault {
        ASCState storage asc = ascs[cid];
        _requireOpen(asc);
        _requireBalanced(asc, balanceC, balanceS);
        if (version <= asc.version) {
            revert InvalidState();
        }
        bytes32 digest = ascStateHash(cid, balanceC, balanceS, version);
        if (!_signedBy(digest, sigC, asc.client) || !_signedBy(digest, sigS, asc.provider)) {
            revert InvalidSignature();
        }
        _closeAndPay(cid, asc, balanceC, balanceS, version);
    }

    function initForceClose(
        bytes32 cid,
        uint256 balanceC,
        uint256 balanceS,
        uint256 version,
        bytes calldata sig
    ) external {
        ASCState storage asc = ascs[cid];
        _requireOpen(asc);
        if (msg.sender != asc.client) {
            revert NotParticipant();
        }
        _requireBalanced(asc, balanceC, balanceS);
        if (version < asc.version || !_signedBy(ascStateHash(cid, balanceC, balanceS, version), sig, asc.client)) {
            revert InvalidSignature();
        }
        asc.status = Status.CLOSING;
        forceCloseReqs[cid] = ForceCloseRequest({
            balanceC: balanceC,
            balanceS: balanceS,
            version: version,
            submitTime: block.timestamp,
            exists: true
        });
        emit ForceCloseInitiated(cid, version, block.timestamp + DISPUTE_WINDOW);
    }

    function challengeForceClose(
        bytes32 cid,
        uint256 balanceC,
        uint256 balanceS,
        uint256 version,
        bytes calldata sig
    ) external {
        ASCState storage asc = ascs[cid];
        ForceCloseRequest storage req = forceCloseReqs[cid];
        if (asc.status != Status.CLOSING || !req.exists || block.timestamp > req.submitTime + DISPUTE_WINDOW) {
            revert InvalidState();
        }
        if (msg.sender != asc.provider && msg.sender != vault) {
            revert NotParticipant();
        }
        _requireBalanced(asc, balanceC, balanceS);
        if (version <= req.version || !_signedBy(ascStateHash(cid, balanceC, balanceS, version), sig, vault)) {
            revert InvalidSignature();
        }
        req.balanceC = balanceC;
        req.balanceS = balanceS;
        req.version = version;
        emit ForceCloseChallenged(cid, version);
    }

    function finalForceClose(bytes32 cid) external {
        ASCState storage asc = ascs[cid];
        ForceCloseRequest memory req = forceCloseReqs[cid];
        if (asc.status != Status.CLOSING || !req.exists) {
            revert InvalidState();
        }
        if (block.timestamp < req.submitTime + DISPUTE_WINDOW) {
            revert DisputeWindowActive();
        }
        delete forceCloseReqs[cid];
        _closeAndPay(cid, asc, req.balanceC, req.balanceS, req.version);
    }

    function forceClose(
        bytes32 cid,
        uint256 balanceC,
        uint256 balanceS,
        uint256 version,
        bytes calldata sigU,
        bytes calldata sigS,
        uint256 px,
        uint256 e,
        uint256 s
    ) external {
        ASCState storage asc = ascs[cid];
        _requireOpen(asc);
        if (msg.sender != asc.provider) {
            revert NotParticipant();
        }
        _requireBalanced(asc, balanceC, balanceS);
        bytes32 digest = ascStateHash(cid, balanceC, balanceS, version);
        if (!_signedBy(digest, sigU, vault) || !_signedBy(digest, sigS, asc.provider)) {
            revert InvalidSignature();
        }
        if (!verifySchnorr(px, e, s, digest)) {
            revert InvalidSignature();
        }
        _closeAndPay(cid, asc, balanceC, balanceS, version);
        emit ForceClosed(cid, balanceC, balanceS, version);
    }

    // ── Module 2: Liquidity Vault ──

    function initVault(address participant, uint256 amount, bytes calldata) external {
        if (participant == address(0) || amount == 0 || msg.sender != participant) {
            revert InvalidState();
        }
        _pull(participant, amount);
        VaultAccount storage account = vaultAccounts[participant];
        account.balance += amount;
        account.initialized = true;
        emit VaultInitialized(participant, amount);
    }

    function settleVault(address participant, uint256 amount, bytes calldata sig) external onlyVault {
        _settleVaultWithSig(participant, amount, sig);
    }

    /// @notice Enclave batch submitter entry. This is the Ethereum analogue of
    ///         Solana `settle_vault` chunks.
    function settleVaultBatch(
        uint256 batchId,
        bytes32 chunkHash,
        address[] calldata participants,
        uint256[] calldata amounts,
        bytes calldata sig
    ) external onlyVault {
        if (participants.length == 0 || participants.length != amounts.length || settledChunks[chunkHash]) {
            revert InvalidState();
        }
        bytes32 digest = vaultBatchHash(batchId, chunkHash, participants, amounts);
        if (sig.length != 0 && !_signedBy(digest, sig, vault)) {
            revert InvalidSignature();
        }
        settledChunks[chunkHash] = true;

        uint256 total;
        for (uint256 i = 0; i < participants.length; i++) {
            _debitAndPay(participants[i], amounts[i]);
            total += amounts[i];
        }
        emit VaultBatchSettled(batchId, chunkHash, participants.length, total);
    }

    /// @notice Packed batch endpoint used by the enclave submitter. It mirrors
    ///         `A402VaultSettlement` calldata and is cheaper than ABI arrays.
    /// @param packedSettlements repeated participant(20 bytes) || amount(32 bytes).
    function settleBatch(
        uint256 batchId,
        bytes32 chunkHash,
        address,
        bytes calldata packedSettlements,
        bytes32
    ) external onlyVault {
        if (packedSettlements.length == 0 || packedSettlements.length % 52 != 0 || settledChunks[chunkHash]) {
            revert InvalidState();
        }
        settledChunks[chunkHash] = true;

        uint256 count = packedSettlements.length / 52;
        uint256 total;
        for (uint256 offset = 0; offset < packedSettlements.length; offset += 52) {
            address participant;
            uint256 amount;
            assembly {
                participant := shr(96, calldataload(add(packedSettlements.offset, offset)))
                amount := calldataload(add(add(packedSettlements.offset, offset), 20))
            }
            if (participant == address(0) || amount == 0) {
                revert InvalidAmount();
            }
            total += amount;
            _push(participant, amount);
        }
        emit VaultBatchSettled(batchId, chunkHash, count, total);
    }

    function initForceSettle(address participant, uint256 amount, bytes calldata proof) external {
        if (msg.sender != participant || amount == 0) {
            revert InvalidState();
        }
        bytes32 digest = vaultSettleHash(participant, amount);
        if (!_signedBy(digest, proof, vault)) {
            revert InvalidSignature();
        }
        forceSettleReqs[participant] = ForceSettleRequest({
            amount: amount,
            submitTime: block.timestamp,
            exists: true
        });
        emit ForceSettleInitiated(participant, amount, block.timestamp + SETTLE_DISPUTE_WINDOW);
    }

    function finalForceSettle(address participant) external {
        ForceSettleRequest memory req = forceSettleReqs[participant];
        if (!req.exists) {
            revert InvalidState();
        }
        if (block.timestamp < req.submitTime + SETTLE_DISPUTE_WINDOW) {
            revert DisputeWindowActive();
        }
        delete forceSettleReqs[participant];
        _debitAndPay(participant, req.amount);
        emit ForceSettleFinalized(participant, req.amount);
    }

    // ── Module 3: Schnorr / adaptor signature verification ──

    function verifySchnorr(uint256 px, uint256 e, uint256 s, bytes32 message) public view returns (bool) {
        if (address(schnorrVerifier) == address(0)) {
            return false;
        }
        return schnorrVerifier.verifySignature(px, e, s, message);
    }

    // ── Hash helpers ──

    function ascStateHash(
        bytes32 cid,
        uint256 balanceC,
        uint256 balanceS,
        uint256 version
    ) public view returns (bytes32) {
        return keccak256(abi.encodePacked("A402_ASC_STATE_V1", address(this), cid, balanceC, balanceS, version));
    }

    function vaultSettleHash(address participant, uint256 amount) public view returns (bytes32) {
        return keccak256(abi.encodePacked("A402_VAULT_SETTLE_V1", address(this), participant, amount));
    }

    function vaultBatchHash(
        uint256 batchId,
        bytes32 chunkHash,
        address[] calldata participants,
        uint256[] calldata amounts
    ) public view returns (bytes32) {
        return keccak256(abi.encode("A402_VAULT_BATCH_V1", address(this), batchId, chunkHash, participants, amounts));
    }

    // ── Internal helpers ──

    function _requireOpen(ASCState storage asc) internal view {
        if (asc.status != Status.OPEN || asc.createdAt == 0) {
            revert InvalidState();
        }
    }

    function _requireBalanced(ASCState storage asc, uint256 balanceC, uint256 balanceS) internal view {
        if (balanceC + balanceS != asc.totalDeposit) {
            revert InvalidAmount();
        }
    }

    function _closeAndPay(
        bytes32 cid,
        ASCState storage asc,
        uint256 balanceC,
        uint256 balanceS,
        uint256 version
    ) internal {
        asc.status = Status.CLOSED;
        asc.balanceC = balanceC;
        asc.balanceS = balanceS;
        asc.version = version;
        if (balanceC > 0) {
            _push(asc.client, balanceC);
        }
        if (balanceS > 0) {
            _push(asc.provider, balanceS);
        }
        emit ASCClosed(cid, balanceC, balanceS, version);
    }

    function _settleVaultWithSig(address participant, uint256 amount, bytes calldata sig) internal {
        if (!_signedBy(vaultSettleHash(participant, amount), sig, vault)) {
            revert InvalidSignature();
        }
        _debitAndPay(participant, amount);
        emit VaultSettled(participant, amount);
    }

    function _debitAndPay(address participant, uint256 amount) internal {
        VaultAccount storage account = vaultAccounts[participant];
        if (participant == address(0) || amount == 0 || account.balance < amount) {
            revert InvalidAmount();
        }
        account.balance -= amount;
        _push(participant, amount);
    }

    function _pull(address from, uint256 amount) internal {
        if (!paymentToken.transferFrom(from, address(this), amount)) {
            revert TransferFailed();
        }
    }

    function _push(address to, uint256 amount) internal {
        if (!paymentToken.transfer(to, amount)) {
            revert TransferFailed();
        }
    }

    function _signedBy(bytes32 digest, bytes calldata signature, address expected) internal pure returns (bool) {
        if (signature.length != 65) {
            return false;
        }
        bytes32 r;
        bytes32 s;
        uint8 v;
        assembly {
            r := calldataload(signature.offset)
            s := calldataload(add(signature.offset, 32))
            v := byte(0, calldataload(add(signature.offset, 64)))
        }
        if (v < 27) {
            v += 27;
        }
        return ecrecover(toEthSignedMessageHash(digest), v, r, s) == expected;
    }

    function toEthSignedMessageHash(bytes32 digest) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", digest));
    }
}
