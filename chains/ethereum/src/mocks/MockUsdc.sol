// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title MockUsdc — minimal ERC20 + EIP-3009 (transferWithAuthorization)
/// @notice Stand-in for USDC during local Anvil development. Implements the
///         signature-based transfer flow x402's `exact` scheme depends on.
///
///         EIP-3009 spec: https://eips.ethereum.org/EIPS/eip-3009
///         EIP-712 domain matches USDC on Ethereum mainnet (name "USDC",
///         version "2") so that buyer wallets can reuse the same typed-data
///         signing code paths against either token.
contract MockUsdc {
    string public constant name = "USD Coin";
    string public constant symbol = "USDC";
    uint8 public constant decimals = 6;
    string public constant version = "2";

    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    /// @dev EIP-3009: tracks whether an authorization nonce has been used or
    ///      cancelled for a given authorizer.
    mapping(address => mapping(bytes32 => bool)) public authorizationState;

    bytes32 public immutable DOMAIN_SEPARATOR;

    /// keccak256("TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)")
    bytes32 public constant TRANSFER_WITH_AUTHORIZATION_TYPEHASH =
        0x7c7c6cdb67a18743f49ec6fa9b35f50d52ed05cbed4cc592e13b44501c1a2267;

    /// keccak256("CancelAuthorization(address authorizer,bytes32 nonce)")
    bytes32 public constant CANCEL_AUTHORIZATION_TYPEHASH =
        0x158b0a9edf7a828aad02f63cd515c68ef2f50ba807396f6d12842833a1597429;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event AuthorizationUsed(address indexed authorizer, bytes32 indexed nonce);
    event AuthorizationCanceled(address indexed authorizer, bytes32 indexed nonce);

    error InsufficientBalance();
    error InsufficientAllowance();
    error AuthorizationNotYetValid();
    error AuthorizationExpired();
    error AuthorizationAlreadyUsed();
    error InvalidSignature();

    constructor() {
        DOMAIN_SEPARATOR = keccak256(
            abi.encode(
                keccak256(
                    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
                ),
                keccak256(bytes(name)),
                keccak256(bytes(version)),
                block.chainid,
                address(this)
            )
        );
    }

    /* -------------------------------------------------------------------------- */
    /*                                   ERC20                                    */
    /* -------------------------------------------------------------------------- */

    function mint(address to, uint256 amount) external {
        totalSupply += amount;
        unchecked {
            balanceOf[to] += amount;
        }
        emit Transfer(address(0), to, amount);
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        _transfer(msg.sender, to, amount);
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        uint256 current = allowance[from][msg.sender];
        if (current != type(uint256).max) {
            if (current < amount) revert InsufficientAllowance();
            unchecked {
                allowance[from][msg.sender] = current - amount;
            }
        }
        _transfer(from, to, amount);
        return true;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function _transfer(address from, address to, uint256 amount) internal {
        uint256 bal = balanceOf[from];
        if (bal < amount) revert InsufficientBalance();
        unchecked {
            balanceOf[from] = bal - amount;
            balanceOf[to] += amount;
        }
        emit Transfer(from, to, amount);
    }

    /* -------------------------------------------------------------------------- */
    /*                                  EIP-3009                                  */
    /* -------------------------------------------------------------------------- */

    /// @notice Execute a signed transfer authorization.
    /// @param from           Token holder who signed the authorization.
    /// @param to             Recipient (the seller / facilitator destination).
    /// @param value          Amount to transfer.
    /// @param validAfter     Unix timestamp the authorization becomes valid.
    /// @param validBefore    Unix timestamp the authorization expires.
    /// @param nonce          Unique 32-byte nonce per authorization.
    /// @param v,r,s          EIP-712 signature over TransferWithAuthorization.
    function transferWithAuthorization(
        address from,
        address to,
        uint256 value,
        uint256 validAfter,
        uint256 validBefore,
        bytes32 nonce,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) external {
        if (block.timestamp <= validAfter) revert AuthorizationNotYetValid();
        if (block.timestamp >= validBefore) revert AuthorizationExpired();
        if (authorizationState[from][nonce]) revert AuthorizationAlreadyUsed();

        bytes32 structHash = keccak256(
            abi.encode(
                TRANSFER_WITH_AUTHORIZATION_TYPEHASH,
                from,
                to,
                value,
                validAfter,
                validBefore,
                nonce
            )
        );
        bytes32 digest = keccak256(abi.encodePacked("\x19\x01", DOMAIN_SEPARATOR, structHash));

        address signer = ecrecover(digest, v, r, s);
        if (signer == address(0) || signer != from) revert InvalidSignature();

        authorizationState[from][nonce] = true;
        emit AuthorizationUsed(from, nonce);

        _transfer(from, to, value);
    }

    /// @notice Cancel an outstanding authorization before it has been used.
    function cancelAuthorization(address authorizer, bytes32 nonce, uint8 v, bytes32 r, bytes32 s)
        external
    {
        if (authorizationState[authorizer][nonce]) revert AuthorizationAlreadyUsed();

        bytes32 structHash =
            keccak256(abi.encode(CANCEL_AUTHORIZATION_TYPEHASH, authorizer, nonce));
        bytes32 digest = keccak256(abi.encodePacked("\x19\x01", DOMAIN_SEPARATOR, structHash));

        address signer = ecrecover(digest, v, r, s);
        if (signer == address(0) || signer != authorizer) revert InvalidSignature();

        authorizationState[authorizer][nonce] = true;
        emit AuthorizationCanceled(authorizer, nonce);
    }
}
