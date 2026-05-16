// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Script, console2} from "forge-std/Script.sol";
import {A402VaultSettlement} from "../src/A402VaultSettlement.sol";
import {ASCManager} from "../src/ASCManager.sol";
import {SchnorrVerifier} from "../src/SchnorrVerifier.sol";
import {MockUsdc} from "../src/mocks/MockUsdc.sol";

/// @notice Local Anvil deploy script.
///
///   forge script script/Deploy.s.sol \
///     --rpc-url http://127.0.0.1:8545 \
///     --broadcast \
///     --private-key $ANVIL_PRIVATE_KEY
///
/// Deploys:
///   - MockUsdc                (the demo ERC20 + EIP-3009)
///   - A402VaultSettlement     (Liquidity Vault mode, )
///   - SchnorrVerifier         (ecrecover-trick verifier for atomic ASC)
///   - ASCManager              (Standard ASC mode, ) wired to verifier
///
/// The vault role for ASCManager is set to ASC_VAULT_EOA (default: Anvil
/// account #1, 0x7099...). That account becomes the "vault coordinator" the
/// Node.js demo service uses to call createASC / closeASC.
///
/// Writes deployment addresses to deployments/local.json so the bootstrap
/// helper and the demo scripts can pick them up via .env files.
contract Deploy is Script {
    function run() external {
        address submitter = vm.envOr("EVM_SUBMITTER", vm.addr(vm.envUint("PRIVATE_KEY")));
        address ascVaultEoa = vm.envOr(
            "ASC_VAULT_EOA", address(0x70997970C51812dc3A010C7d01b50e0d17dc79C8)
        );
        uint256 mintAmount = vm.envOr("USDC_PREFUND", uint256(10_000_000 * 1e6));
        uint256 buyerPrefund = vm.envOr("BUYER_PREFUND", uint256(1_000 * 1e6));
        address buyerAddress = vm.envOr(
            "BUYER_ADDRESS", address(0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC)
        );

        vm.startBroadcast();

        MockUsdc usdc = new MockUsdc();
        A402VaultSettlement vault = new A402VaultSettlement(submitter);
        SchnorrVerifier schnorr = new SchnorrVerifier();
        ASCManager ascManager = new ASCManager(address(usdc), ascVaultEoa, address(schnorr));
        usdc.mint(address(vault), mintAmount);
        usdc.mint(buyerAddress, buyerPrefund);

        vm.stopBroadcast();

        console2.log("MockUsdc (USDC):    ", address(usdc));
        console2.log("A402VaultSettlement:", address(vault));
        console2.log("SchnorrVerifier:    ", address(schnorr));
        console2.log("ASCManager:         ", address(ascManager));
        console2.log("submitter:          ", submitter);
        console2.log("ASC vault EOA:      ", ascVaultEoa);
        console2.log("buyer prefunded:    ", buyerAddress);
        console2.log("buyer prefund USDC: ", buyerPrefund);
        console2.log("vault prefund USDC: ", mintAmount);

        _writeDeployment(
            address(usdc),
            address(vault),
            address(ascManager),
            address(schnorr),
            submitter,
            ascVaultEoa,
            mintAmount
        );
    }

    function _writeDeployment(
        address usdc,
        address vault,
        address ascManager,
        address schnorrVerifier,
        address submitter,
        address ascVaultEoa,
        uint256 prefundedUsdc
    ) internal {
        string memory key = "deployment";
        vm.serializeAddress(key, "mockUsdc", usdc);
        vm.serializeAddress(key, "vaultSettlement", vault);
        vm.serializeAddress(key, "ascManager", ascManager);
        vm.serializeAddress(key, "schnorrVerifier", schnorrVerifier);
        vm.serializeAddress(key, "submitter", submitter);
        vm.serializeAddress(key, "ascVaultEoa", ascVaultEoa);
        vm.serializeUint(key, "prefundedUsdc", prefundedUsdc);
        string memory json = vm.serializeUint(key, "chainId", block.chainid);
        vm.writeJson(json, "./deployments/local.json");
    }
}
