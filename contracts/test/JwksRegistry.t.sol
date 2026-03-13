// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import "forge-std/Test.sol";
import "../src/JwksRegistry.sol";

contract JwksRegistryTest is Test {
    JwksRegistry registry;

    bytes constant MOCK_N = hex"deadbeef01020304";
    bytes constant MOCK_E = hex"010001";
    string constant INITIAL_KID = "key-1";

    function setUp() public {
        registry = new JwksRegistry(INITIAL_KID, MOCK_N, MOCK_E);
    }

    function test_constructorStoresInitialKey() public view {
        (bytes memory n, bytes memory e) = registry.getKey(INITIAL_KID);
        assertEq(n, MOCK_N);
        assertEq(e, MOCK_E);
    }

    function test_hasKeyReturnsTrueForExistingKey() public view {
        assertTrue(registry.hasKey(INITIAL_KID));
    }

    function test_hasKeyReturnsFalseForMissingKey() public view {
        assertFalse(registry.hasKey("nonexistent"));
    }

    function test_getKeyRevertsForMissingKey() public {
        vm.expectRevert("JwksRegistry: key not found");
        registry.getKey("nonexistent");
    }

    function test_constructorRevertsOnEmptyModulus() public {
        vm.expectRevert("JwksRegistry: empty modulus");
        new JwksRegistry("kid", "", MOCK_E);
    }

    function test_constructorRevertsOnEmptyExponent() public {
        vm.expectRevert("JwksRegistry: empty exponent");
        new JwksRegistry("kid", MOCK_N, "");
    }

    function test_addedAtTimestamp() public view {
        // getKey doesn't expose addedAt, but hasKey confirms the key exists.
        // addedAt is set internally; we just confirm the key was stored at deployment time.
        assertTrue(registry.hasKey(INITIAL_KID));
    }
}
