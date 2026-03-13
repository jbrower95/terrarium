// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "forge-std/Script.sol";
import "../src/TerrariumWallet.sol";
import "../src/JwksRegistry.sol";
import "../src/SpendPolicy.sol";
import {IEntryPoint} from "@account-abstraction/interfaces/IEntryPoint.sol";

/// @title DeployWallet
/// @notice Deploys the full Terrarium stack (JwksRegistry, SpendPolicy, TerrariumWallet)
///         using CREATE2 for deterministic addresses derived from repo identity.
///
///         Deployment order:
///           1. JwksRegistry  — via CREATE2 (no circular deps)
///           2. SpendPolicy   — via CREATE (needs wallet address, which we precompute)
///           3. TerrariumWallet — via CREATE2 (references both contracts above)
///
///         SpendPolicy uses nonce-based CREATE because it and TerrariumWallet reference
///         each other in their constructors, creating a circular CREATE2 dependency.
///         We break the cycle by predicting SpendPolicy's CREATE address from the
///         deployer nonce, then using that to compute TerrariumWallet's CREATE2 address,
///         which we pass back to SpendPolicy's constructor.
contract DeployWallet is Script {
    address constant CREATE2_DEPLOYER = 0x4e59b44847b379578588920cA78FbF26c0B4956C;
    address constant DEFAULT_ENTRY_POINT = 0x0000000071727De22E5E9d8BAf0edAc6f37da032;

    function run() public {
        // -----------------------------------------------------------------
        // Read environment
        // -----------------------------------------------------------------
        string memory repoOwner = vm.envString("REPO_OWNER");
        string memory repoName = vm.envString("REPO_NAME");
        address entryPointAddr = vm.envOr("ENTRY_POINT", DEFAULT_ENTRY_POINT);
        uint256 maxDailySpend = vm.envUint("MAX_DAILY_SPEND");
        address[] memory allowedDests = vm.envAddress("ALLOWED_DESTINATIONS", ",");
        string memory initialKid = vm.envString("INITIAL_KID");
        bytes memory initialModulus = vm.envBytes("INITIAL_MODULUS");
        bytes memory initialExponent = vm.envBytes("INITIAL_EXPONENT");

        // -----------------------------------------------------------------
        // Derive salts from repo identity
        // -----------------------------------------------------------------
        bytes32 baseSalt = keccak256(abi.encodePacked(repoOwner, "/", repoName));
        bytes32 jwksSalt = keccak256(abi.encodePacked(baseSalt, "jwks"));
        bytes32 walletSalt = keccak256(abi.encodePacked(baseSalt, "wallet"));

        // Claim hashes for TerrariumWallet
        bytes32 repoHash = keccak256(abi.encodePacked(repoOwner, "/", repoName));
        bytes32 workflowHash = keccak256("deploy.yml");
        bytes32 refHash = keccak256("refs/heads/main");

        // -----------------------------------------------------------------
        // Precompute addresses
        // -----------------------------------------------------------------

        // JwksRegistry CREATE2 address
        bytes memory jwksInitcode = abi.encodePacked(
            type(JwksRegistry).creationCode,
            abi.encode(initialKid, initialModulus, initialExponent)
        );
        address jwksAddr = _computeCreate2(jwksSalt, jwksInitcode);

        // Predict SpendPolicy CREATE address from deployer nonce.
        // The JwksRegistry deploy (a .call to the CREATE2 deployer) is the first
        // broadcast transaction and increments the sender's nonce, so SpendPolicy
        // deploys at currentNonce + 1.
        address broadcaster = vm.addr(vm.envUint("PRIVATE_KEY"));
        uint64 currentNonce = vm.getNonce(broadcaster);
        address predictedSpendPolicy = vm.computeCreateAddress(broadcaster, currentNonce + 1);

        // TerrariumWallet CREATE2 address (depends on SpendPolicy address)
        bytes memory walletInitcode = abi.encodePacked(
            type(TerrariumWallet).creationCode,
            abi.encode(
                IEntryPoint(entryPointAddr),
                repoHash,
                workflowHash,
                refHash,
                JwksRegistry(jwksAddr),
                SpendPolicy(predictedSpendPolicy)
            )
        );
        address walletAddr = _computeCreate2(walletSalt, walletInitcode);

        // -----------------------------------------------------------------
        // Deploy
        // -----------------------------------------------------------------
        vm.startBroadcast();

        // 1. JwksRegistry via CREATE2
        address jwksDeployed;
        {
            (bool ok, bytes memory ret) = CREATE2_DEPLOYER.call(
                abi.encodePacked(jwksSalt, jwksInitcode)
            );
            require(ok, "JwksRegistry CREATE2 failed");
            assembly {
                jwksDeployed := mload(add(ret, 20))
            }
        }
        require(jwksDeployed == jwksAddr, "JwksRegistry address mismatch");

        // 2. SpendPolicy via CREATE (nonce-based, breaks circular dep)
        SpendPolicy spendPolicy = new SpendPolicy(
            maxDailySpend,
            allowedDests,
            walletAddr // precomputed CREATE2 address
        );
        require(address(spendPolicy) == predictedSpendPolicy, "SpendPolicy address mismatch");

        // 3. TerrariumWallet via CREATE2
        address walletDeployed;
        {
            (bool ok, bytes memory ret) = CREATE2_DEPLOYER.call(
                abi.encodePacked(walletSalt, walletInitcode)
            );
            require(ok, "TerrariumWallet CREATE2 failed");
            assembly {
                walletDeployed := mload(add(ret, 20))
            }
        }
        require(walletDeployed == walletAddr, "TerrariumWallet address mismatch");

        vm.stopBroadcast();

        // -----------------------------------------------------------------
        // Log results
        // -----------------------------------------------------------------
        console.log("=== Terrarium Deployment ===");
        console.log("Repo:            %s/%s", repoOwner, repoName);
        console.log("JwksRegistry:    %s", jwksDeployed);
        console.log("SpendPolicy:     %s", address(spendPolicy));
        console.log("TerrariumWallet: %s", walletDeployed);
        console.log("EntryPoint:      %s", entryPointAddr);
    }

    /// @dev Computes the CREATE2 address for the deterministic deployer.
    function _computeCreate2(bytes32 salt, bytes memory initcode) internal pure returns (address) {
        return address(
            uint160(
                uint256(
                    keccak256(
                        abi.encodePacked(
                            bytes1(0xff),
                            CREATE2_DEPLOYER,
                            salt,
                            keccak256(initcode)
                        )
                    )
                )
            )
        );
    }
}
