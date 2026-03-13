// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import "forge-std/Test.sol";
import "../src/SpendPolicy.sol";

contract SpendPolicyTest is Test {
    SpendPolicy policy;

    address wallet = address(0x1111);
    address dest1 = address(0xD1);
    address dest2 = address(0xD2);
    address notAllowed = address(0xBA);

    uint256 constant MAX_DAILY = 1 ether;

    function setUp() public {
        address[] memory dests = new address[](2);
        dests[0] = dest1;
        dests[1] = dest2;

        policy = new SpendPolicy(MAX_DAILY, dests, wallet);
    }

    // -----------------------------------------------------------------------
    // Constructor / immutables
    // -----------------------------------------------------------------------

    function test_immutables() public view {
        assertEq(policy.maxDailySpend(), MAX_DAILY);
        assertEq(policy.trustedCaller(), wallet);
    }

    function test_allowedDestinations() public view {
        assertTrue(policy.isAllowedDestination(dest1));
        assertTrue(policy.isAllowedDestination(dest2));
        assertFalse(policy.isAllowedDestination(notAllowed));
    }

    // -----------------------------------------------------------------------
    // check()
    // -----------------------------------------------------------------------

    function test_check_reverts_disallowed_destination() public {
        vm.expectRevert(abi.encodeWithSelector(SpendPolicy.DestinationNotAllowed.selector, notAllowed));
        policy.check(notAllowed, 0.1 ether);
    }

    function test_check_passes_allowed_destination() public view {
        policy.check(dest1, 0.5 ether);
    }

    function test_check_reverts_exceed_daily_limit() public {
        // Record a spend that uses most of the budget
        vm.prank(wallet);
        policy.recordSpend(0.8 ether);

        // Trying to spend 0.3 ether should fail (0.8 + 0.3 > 1.0)
        vm.expectRevert(abi.encodeWithSelector(SpendPolicy.DailySpendExceeded.selector, 0.3 ether, 0.2 ether));
        policy.check(dest1, 0.3 ether);
    }

    function test_check_passes_within_daily_limit() public {
        vm.prank(wallet);
        policy.recordSpend(0.5 ether);

        // 0.5 + 0.4 = 0.9 < 1.0, should pass
        policy.check(dest1, 0.4 ether);
    }

    // -----------------------------------------------------------------------
    // recordSpend()
    // -----------------------------------------------------------------------

    function test_recordSpend_onlyTrustedCaller() public {
        vm.prank(address(0xBEEF));
        vm.expectRevert(SpendPolicy.OnlyTrustedCaller.selector);
        policy.recordSpend(0.1 ether);
    }

    function test_recordSpend_emits_event() public {
        vm.prank(wallet);
        vm.expectEmit(true, true, true, true);
        emit SpendPolicy.SpendRecorded(0.5 ether, 0.5 ether);
        policy.recordSpend(0.5 ether);
    }

    function test_recordSpend_cumulative_total() public {
        vm.prank(wallet);
        policy.recordSpend(0.3 ether);

        vm.prank(wallet);
        policy.recordSpend(0.4 ether);

        assertEq(policy.dailySpendRemaining(), 0.3 ether);
    }

    // -----------------------------------------------------------------------
    // Rolling window
    // -----------------------------------------------------------------------

    function test_rolling_window_expires_old_entries() public {
        // Spend 0.9 ether now
        vm.prank(wallet);
        policy.recordSpend(0.9 ether);
        assertEq(policy.dailySpendRemaining(), 0.1 ether);

        // Warp forward 24h + 1 second — the spend should have expired
        vm.warp(block.timestamp + 86401);
        assertEq(policy.dailySpendRemaining(), MAX_DAILY);

        // Should now be able to spend the full amount again
        policy.check(dest1, 1 ether);
    }

    function test_rolling_window_partial_expiry() public {
        // Spend 0.6 ether at t=0
        vm.prank(wallet);
        policy.recordSpend(0.6 ether);

        // Warp 12 hours, spend 0.3 ether at t=12h
        vm.warp(block.timestamp + 43200);
        vm.prank(wallet);
        policy.recordSpend(0.3 ether);

        // Remaining should be 0.1 ether
        assertEq(policy.dailySpendRemaining(), 0.1 ether);

        // Warp another 12h+1s — first entry expires, second doesn't
        vm.warp(block.timestamp + 43201);
        assertEq(policy.dailySpendRemaining(), 0.7 ether);
    }

    // -----------------------------------------------------------------------
    // dailySpendRemaining()
    // -----------------------------------------------------------------------

    function test_dailySpendRemaining_fresh() public view {
        assertEq(policy.dailySpendRemaining(), MAX_DAILY);
    }

    function test_dailySpendRemaining_saturates_at_zero() public {
        vm.prank(wallet);
        policy.recordSpend(MAX_DAILY);
        assertEq(policy.dailySpendRemaining(), 0);
    }
}
