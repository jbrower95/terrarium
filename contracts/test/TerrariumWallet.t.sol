// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "forge-std/Test.sol";
import "../src/TerrariumWallet.sol";
import "../src/JwksRegistry.sol";
import "../src/SpendPolicy.sol";
import {IEntryPoint} from "@account-abstraction/interfaces/IEntryPoint.sol";

contract TerrariumWalletTest is Test {
    TerrariumWallet wallet;
    JwksRegistry registry;
    SpendPolicy policy;

    // Dummy RSA key material for the registry constructor
    bytes constant DUMMY_N = hex"0102030405060708";
    bytes constant DUMMY_E = hex"010001";

    function setUp() public {
        // Deploy the JWKS registry with a dummy initial key
        registry = new JwksRegistry("initial-kid", DUMMY_N, DUMMY_E);

        // Pre-compute the wallet address for the SpendPolicy trustedCaller
        address walletAddr = vm.computeCreateAddress(address(this), vm.getNonce(address(this)) + 1);

        // Deploy the spend policy with a generous limit and no allowed destinations
        address[] memory allowedDests = new address[](0);
        policy = new SpendPolicy(1 ether, allowedDests, walletAddr);

        // Deploy the wallet
        wallet = new TerrariumWallet(
            IEntryPoint(address(1)),       // dummy entry point
            keccak256("owner/repo"),
            keccak256("deploy.yml"),
            keccak256("refs/heads/main"),
            registry,
            policy
        );
    }

    function test_deployed() public view {
        assertTrue(address(wallet) != address(0));
    }

    function test_entryPoint() public view {
        assertEq(address(wallet.entryPoint()), address(1));
    }

    function test_immutables() public view {
        assertEq(wallet.REPO_HASH(), keccak256("owner/repo"));
        assertEq(wallet.WORKFLOW_HASH(), keccak256("deploy.yml"));
        assertEq(wallet.REF_HASH(), keccak256("refs/heads/main"));
        assertEq(address(wallet.jwksRegistry()), address(registry));
        assertEq(address(wallet.spendPolicy()), address(policy));
    }

    function test_receiveEth() public {
        vm.deal(address(this), 1 ether);
        (bool ok,) = address(wallet).call{value: 0.5 ether}("");
        assertTrue(ok);
        assertEq(address(wallet).balance, 0.5 ether);
    }
}
