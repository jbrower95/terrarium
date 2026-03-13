// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import "forge-std/Script.sol";
import "../src/TerrariumWallet.sol";

contract DeployWallet is Script {
    function run() public {
        vm.startBroadcast();
        new TerrariumWallet();
        vm.stopBroadcast();
    }
}
