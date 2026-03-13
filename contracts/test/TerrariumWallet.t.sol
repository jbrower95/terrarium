// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import "forge-std/Test.sol";
import "../src/TerrariumWallet.sol";

contract TerrariumWalletTest is Test {
    TerrariumWallet wallet;

    function setUp() public {
        wallet = new TerrariumWallet();
    }

    function test_deployed() public view {
        assertTrue(address(wallet) != address(0));
    }
}
