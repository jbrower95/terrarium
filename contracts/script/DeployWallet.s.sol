// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "forge-std/Script.sol";
import "../src/TerrariumWallet.sol";
import "../src/JwksRegistry.sol";
import "../src/SpendPolicy.sol";
import {IEntryPoint} from "@account-abstraction/interfaces/IEntryPoint.sol";

contract DeployWallet is Script {
    function run() public {
        vm.startBroadcast();

        address entryPointAddr = vm.envAddress("ENTRY_POINT");
        bytes32 repoHash = vm.envBytes32("REPO_HASH");
        bytes32 workflowHash = vm.envBytes32("WORKFLOW_HASH");
        bytes32 refHash = vm.envBytes32("REF_HASH");
        address jwksRegistryAddr = vm.envAddress("JWKS_REGISTRY");
        address spendPolicyAddr = vm.envAddress("SPEND_POLICY");

        new TerrariumWallet(
            IEntryPoint(entryPointAddr),
            repoHash,
            workflowHash,
            refHash,
            JwksRegistry(jwksRegistryAddr),
            SpendPolicy(spendPolicyAddr)
        );

        vm.stopBroadcast();
    }
}
