// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

/// @title SpendPolicy
/// @notice Immutable policy contract that enforces spending constraints on the terrarium wallet.
///         Once deployed, no parameters can be changed — no admin, no upgradability.
contract SpendPolicy {
    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    event SpendRecorded(uint256 value, uint256 dailyTotal);

    // -----------------------------------------------------------------------
    // Errors
    // -----------------------------------------------------------------------

    error DestinationNotAllowed(address to);
    error DailySpendExceeded(uint256 requested, uint256 remaining);
    error OnlyTrustedCaller();

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    struct SpendEntry {
        uint256 timestamp;
        uint256 value;
    }

    // -----------------------------------------------------------------------
    // Immutable / constant state
    // -----------------------------------------------------------------------

    uint256 private constant WINDOW = 86400; // 24 hours in seconds

    uint256 public immutable maxDailySpend;
    address public immutable trustedCaller;

    // -----------------------------------------------------------------------
    // Storage
    // -----------------------------------------------------------------------

    /// @dev Mapping from address to whether it is an allowed destination.
    mapping(address => bool) private _allowedDestinations;

    /// @dev Array of spend entries used for rolling window tracking.
    SpendEntry[] private _spendLog;

    // -----------------------------------------------------------------------
    // Constructor
    // -----------------------------------------------------------------------

    /// @param _maxDailySpend Maximum ETH value (in wei) that may be spent in any rolling 24h window.
    /// @param _allowedDests  Whitelist of addresses that may receive funds.
    /// @param _trustedCaller Address of the wallet contract that is permitted to call `recordSpend`.
    constructor(uint256 _maxDailySpend, address[] memory _allowedDests, address _trustedCaller) {
        maxDailySpend = _maxDailySpend;
        trustedCaller = _trustedCaller;

        for (uint256 i = 0; i < _allowedDests.length; i++) {
            _allowedDestinations[_allowedDests[i]] = true;
        }
    }

    // -----------------------------------------------------------------------
    // External — validation
    // -----------------------------------------------------------------------

    /// @notice Reverts if the proposed spend violates policy.
    /// @param to    The destination address.
    /// @param value The ETH value (in wei) of the proposed spend.
    function check(address to, uint256 value) external view {
        if (!_allowedDestinations[to]) {
            revert DestinationNotAllowed(to);
        }

        uint256 remaining = _dailySpendRemaining();
        if (value > remaining) {
            revert DailySpendExceeded(value, remaining);
        }
    }

    // -----------------------------------------------------------------------
    // External — state mutation
    // -----------------------------------------------------------------------

    /// @notice Records a spend event. Only callable by the trusted wallet.
    /// @param value The ETH value (in wei) that was spent.
    function recordSpend(uint256 value) external {
        if (msg.sender != trustedCaller) {
            revert OnlyTrustedCaller();
        }

        _spendLog.push(SpendEntry({timestamp: block.timestamp, value: value}));

        uint256 total = _rollingTotal();
        emit SpendRecorded(value, total);
    }

    // -----------------------------------------------------------------------
    // External — views
    // -----------------------------------------------------------------------

    /// @notice Returns how much ETH (in wei) can still be spent in the current rolling 24h window.
    function dailySpendRemaining() external view returns (uint256) {
        return _dailySpendRemaining();
    }

    /// @notice Returns whether `dest` is in the allowed destinations set.
    function isAllowedDestination(address dest) external view returns (bool) {
        return _allowedDestinations[dest];
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    function _dailySpendRemaining() internal view returns (uint256) {
        uint256 total = _rollingTotal();
        if (total >= maxDailySpend) {
            return 0;
        }
        return maxDailySpend - total;
    }

    /// @dev Sums all spend entries whose timestamp falls within the last 86400 seconds.
    function _rollingTotal() internal view returns (uint256 total) {
        uint256 windowStart = block.timestamp > WINDOW ? block.timestamp - WINDOW : 0;

        for (uint256 i = _spendLog.length; i > 0; i--) {
            SpendEntry storage entry = _spendLog[i - 1];
            if (entry.timestamp <= windowStart) {
                break; // entries are in chronological order, so we can stop early
            }
            total += entry.value;
        }
    }
}
