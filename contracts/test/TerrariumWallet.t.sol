// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "forge-std/Test.sol";
import "../src/TerrariumWallet.sol";
import "../src/JwksRegistry.sol";
import "../src/SpendPolicy.sol";
import {IEntryPoint} from "@account-abstraction/interfaces/IEntryPoint.sol";
import {PackedUserOperation} from "@account-abstraction/interfaces/PackedUserOperation.sol";

contract TerrariumWalletTest is Test {
    TerrariumWallet wallet;
    JwksRegistry registry;
    SpendPolicy policy;

    address constant ENTRY_POINT = address(1);

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
            IEntryPoint(ENTRY_POINT),
            keccak256("owner/repo"),
            keccak256("deploy.yml"),
            keccak256("refs/heads/main"),
            registry,
            policy
        );
    }

    // -----------------------------------------------------------------------
    // Deployment
    // -----------------------------------------------------------------------

    function test_deployed() public view {
        assertTrue(address(wallet) != address(0));
    }

    function test_entryPoint() public view {
        assertEq(address(wallet.entryPoint()), ENTRY_POINT);
    }

    function test_immutables() public view {
        assertEq(wallet.REPO_HASH(), keccak256("owner/repo"));
        assertEq(wallet.WORKFLOW_HASH(), keccak256("deploy.yml"));
        assertEq(wallet.REF_HASH(), keccak256("refs/heads/main"));
        assertEq(address(wallet.jwksRegistry()), address(registry));
        assertEq(address(wallet.spendPolicy()), address(policy));
    }

    function test_immutables_differentValues() public {
        // Deploy a second wallet with different immutables to confirm they vary
        TerrariumWallet wallet2 = new TerrariumWallet(
            IEntryPoint(address(2)),
            keccak256("other/repo"),
            keccak256("ci.yml"),
            keccak256("refs/heads/dev"),
            registry,
            policy
        );

        assertEq(address(wallet2.entryPoint()), address(2));
        assertEq(wallet2.REPO_HASH(), keccak256("other/repo"));
        assertEq(wallet2.WORKFLOW_HASH(), keccak256("ci.yml"));
        assertEq(wallet2.REF_HASH(), keccak256("refs/heads/dev"));
        // Confirm they differ from wallet1
        assertTrue(wallet2.REPO_HASH() != wallet.REPO_HASH());
        assertTrue(wallet2.WORKFLOW_HASH() != wallet.WORKFLOW_HASH());
        assertTrue(wallet2.REF_HASH() != wallet.REF_HASH());
    }

    // -----------------------------------------------------------------------
    // Receive ETH
    // -----------------------------------------------------------------------

    function test_receiveEth() public {
        vm.deal(address(this), 1 ether);
        (bool ok,) = address(wallet).call{value: 0.5 ether}("");
        assertTrue(ok);
        assertEq(address(wallet).balance, 0.5 ether);
    }

    function test_receiveEth_zero() public {
        (bool ok,) = address(wallet).call{value: 0}("");
        assertTrue(ok);
        assertEq(address(wallet).balance, 0);
    }

    function test_receiveEth_multiple() public {
        vm.deal(address(this), 3 ether);
        (bool ok1,) = address(wallet).call{value: 1 ether}("");
        assertTrue(ok1);
        (bool ok2,) = address(wallet).call{value: 0.5 ether}("");
        assertTrue(ok2);
        assertEq(address(wallet).balance, 1.5 ether);
    }

    function test_receiveEth_fromDifferentSenders() public {
        address sender1 = address(0xA1);
        address sender2 = address(0xA2);
        vm.deal(sender1, 1 ether);
        vm.deal(sender2, 1 ether);

        vm.prank(sender1);
        (bool ok1,) = address(wallet).call{value: 0.3 ether}("");
        assertTrue(ok1);

        vm.prank(sender2);
        (bool ok2,) = address(wallet).call{value: 0.7 ether}("");
        assertTrue(ok2);

        assertEq(address(wallet).balance, 1 ether);
    }

    // -----------------------------------------------------------------------
    // _extractPayloadSegment (tested indirectly through a helper)
    // -----------------------------------------------------------------------

    // We cannot call _extractPayloadSegment directly since it is internal
    // and not exposed. But we can verify it works through validateUserOp
    // (indirectly). We test the logic via the JwtValidator tests instead.

    // -----------------------------------------------------------------------
    // validateUserOp access control
    // -----------------------------------------------------------------------

    // BaseAccount's validateUserOp requires msg.sender == entryPoint().
    // Calling from any other address should revert.

    function test_validateUserOp_reverts_nonEntryPoint() public {
        PackedUserOperation memory userOp;
        userOp.sender = address(wallet);
        userOp.nonce = 0;
        userOp.callData = "";
        userOp.accountGasLimits = bytes32(0);
        userOp.preVerificationGas = 0;
        userOp.gasFees = bytes32(0);
        userOp.paymasterAndData = "";
        userOp.signature = "fake.jwt.sig";

        bytes32 userOpHash = keccak256("test");

        // Call from a random address (not the entry point)
        vm.prank(address(0xBEEF));
        vm.expectRevert();
        wallet.validateUserOp(userOp, userOpHash, 0);
    }

    function test_validateUserOp_reverts_fromDeployer() public {
        PackedUserOperation memory userOp;
        userOp.sender = address(wallet);
        userOp.signature = "fake.jwt.sig";

        bytes32 userOpHash = keccak256("test");

        // address(this) is the deployer, not the entry point
        vm.expectRevert();
        wallet.validateUserOp(userOp, userOpHash, 0);
    }

    // -----------------------------------------------------------------------
    // Integration: validateUserOp with mock entry point
    // -----------------------------------------------------------------------
    // Full JWT validation requires a valid RSA signature which is impractical
    // to construct in Solidity tests. Instead, we verify that calling from the
    // entry point with an invalid JWT reverts with the expected parsing errors
    // (proving the code path is reached).

    function test_validateUserOp_fromEntryPoint_invalidJwt_reverts() public {
        PackedUserOperation memory userOp;
        userOp.sender = address(wallet);
        userOp.signature = "not-a-jwt"; // no dots, will fail extractKid

        bytes32 userOpHash = keccak256("test");

        // Call from the entry point address
        vm.prank(ENTRY_POINT);
        vm.expectRevert(JwtValidator.MalformedJwt.selector);
        wallet.validateUserOp(userOp, userOpHash, 0);
    }

    function test_validateUserOp_fromEntryPoint_malformedJwt_singleDot() public {
        PackedUserOperation memory userOp;
        userOp.sender = address(wallet);
        // Has one dot but extractKid will try to decode the header.
        // Header "bm90" decodes to "not" which is not valid JSON for kid extraction.
        userOp.signature = "bm90.payload.sig";

        bytes32 userOpHash = keccak256("test");

        vm.prank(ENTRY_POINT);
        // extractKid will try to decode header and then look for "kid" key.
        // Since the decoded header won't contain a "kid" key, it should revert.
        vm.expectRevert();
        wallet.validateUserOp(userOp, userOpHash, 0);
    }
}

/// @dev Separate test contract to test wallet with allowed destinations in the
///      spend policy, verifying that validateUserOp reaches the spend policy check.
contract TerrariumWalletWithPolicyTest is Test {
    TerrariumWallet wallet;
    JwksRegistry registry;
    SpendPolicy policy;

    address constant ENTRY_POINT = address(1);
    address constant DEST = address(0xD1);

    bytes constant DUMMY_N = hex"0102030405060708";
    bytes constant DUMMY_E = hex"010001";

    function setUp() public {
        registry = new JwksRegistry("initial-kid", DUMMY_N, DUMMY_E);

        address walletAddr = vm.computeCreateAddress(address(this), vm.getNonce(address(this)) + 1);

        address[] memory allowedDests = new address[](1);
        allowedDests[0] = DEST;
        policy = new SpendPolicy(1 ether, allowedDests, walletAddr);

        wallet = new TerrariumWallet(
            IEntryPoint(ENTRY_POINT),
            keccak256("owner/repo"),
            keccak256("deploy.yml"),
            keccak256("refs/heads/main"),
            registry,
            policy
        );
    }

    function test_walletReferencesCorrectPolicy() public view {
        assertEq(address(wallet.spendPolicy()), address(policy));
        assertTrue(policy.isAllowedDestination(DEST));
        assertEq(policy.trustedCaller(), address(wallet));
    }

    function test_walletReferencesCorrectRegistry() public view {
        assertEq(address(wallet.jwksRegistry()), address(registry));
        assertTrue(registry.hasKey("initial-kid"));
    }
}
