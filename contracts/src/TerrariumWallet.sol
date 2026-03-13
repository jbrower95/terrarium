// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {BaseAccount} from "@account-abstraction/core/BaseAccount.sol";
import {IEntryPoint} from "@account-abstraction/interfaces/IEntryPoint.sol";
import {PackedUserOperation} from "@account-abstraction/interfaces/PackedUserOperation.sol";
import {_packValidationData, SIG_VALIDATION_FAILED} from "@account-abstraction/core/Helpers.sol";
import {JwtValidator} from "./JwtValidator.sol";
import {JwksRegistry} from "./JwksRegistry.sol";
import {SpendPolicy} from "./SpendPolicy.sol";

/// @title TerrariumWallet
/// @notice ERC-4337 smart wallet that authenticates via GitHub Actions OIDC JWTs.
///         No private key, no admin — authorization is proven by a valid JWT from
///         a pinned repository, workflow, and ref.
contract TerrariumWallet is BaseAccount {
    // -----------------------------------------------------------------------
    // Immutable state
    // -----------------------------------------------------------------------

    IEntryPoint private immutable _entryPoint;
    bytes32 public immutable REPO_HASH;
    bytes32 public immutable WORKFLOW_HASH;
    bytes32 public immutable REF_HASH;
    JwksRegistry public immutable jwksRegistry;
    SpendPolicy public immutable spendPolicy;

    /// @dev Expected issuer for GitHub Actions OIDC tokens.
    bytes32 private constant EXPECTED_ISS_HASH =
        keccak256("https://token.actions.githubusercontent.com");

    /// @dev Expected repository visibility.
    bytes32 private constant EXPECTED_VISIBILITY_HASH = keccak256("public");

    // -----------------------------------------------------------------------
    // Constructor
    // -----------------------------------------------------------------------

    /// @param entryPoint_    The ERC-4337 EntryPoint contract.
    /// @param repoHash       keccak256 of the authorized "owner/repo" string.
    /// @param workflowHash   keccak256 of the authorized workflow filename.
    /// @param refHash        keccak256 of the authorized git ref (e.g. "refs/heads/main").
    /// @param jwksRegistry_  The JWKS registry for looking up RSA public keys.
    /// @param spendPolicy_   The spend-policy contract enforcing destination and rate limits.
    constructor(
        IEntryPoint entryPoint_,
        bytes32 repoHash,
        bytes32 workflowHash,
        bytes32 refHash,
        JwksRegistry jwksRegistry_,
        SpendPolicy spendPolicy_
    ) {
        _entryPoint = entryPoint_;
        REPO_HASH = repoHash;
        WORKFLOW_HASH = workflowHash;
        REF_HASH = refHash;
        jwksRegistry = jwksRegistry_;
        spendPolicy = spendPolicy_;
    }

    // -----------------------------------------------------------------------
    // BaseAccount overrides
    // -----------------------------------------------------------------------

    /// @inheritdoc BaseAccount
    function entryPoint() public view override returns (IEntryPoint) {
        return _entryPoint;
    }

    /// @inheritdoc BaseAccount
    function _validateSignature(
        PackedUserOperation calldata userOp,
        bytes32 /* userOpHash */
    ) internal view override returns (uint256 validationData) {
        // 1. The JWT is passed as userOp.signature
        bytes memory jwt = userOp.signature;

        // 2. Extract kid from the JWT header
        string memory kid = JwtValidator.extractKid(jwt);

        // 3. Look up the RSA public key from the registry
        (bytes memory n, bytes memory e) = jwksRegistry.getKey(kid);

        // 4. Verify the JWT signature
        bool sigValid = JwtValidator.verifySignature(jwt, n, e);
        if (!sigValid) {
            return SIG_VALIDATION_FAILED;
        }

        // 5. Extract claims from the JWT payload (signature already verified above)
        bytes memory payloadB64 = _extractPayloadSegment(jwt);
        JwtValidator.JwtClaims memory claims = JwtValidator.extractClaims(payloadB64);

        // 6. Validate claims
        if (keccak256(claims.iss) != EXPECTED_ISS_HASH) {
            return SIG_VALIDATION_FAILED;
        }
        if (keccak256(claims.repository) != REPO_HASH) {
            return SIG_VALIDATION_FAILED;
        }
        if (keccak256(claims.workflow) != WORKFLOW_HASH) {
            return SIG_VALIDATION_FAILED;
        }
        if (keccak256(claims.ref_) != REF_HASH) {
            return SIG_VALIDATION_FAILED;
        }
        if (keccak256(claims.repository_visibility) != EXPECTED_VISIBILITY_HASH) {
            return SIG_VALIDATION_FAILED;
        }
        if (claims.exp <= block.timestamp) {
            return SIG_VALIDATION_FAILED;
        }

        // 7. Check spend policy constraints on the call encoded in callData.
        //    Decode the execute(address,uint256,bytes) call to extract `to` and `value`.
        if (userOp.callData.length >= 68) {
            (address to, uint256 value,) = abi.decode(
                userOp.callData[4:],
                (address, uint256, bytes)
            );
            spendPolicy.check(to, value);
        }

        // 8. Pack validation data: sigFailed=false, validUntil=exp, validAfter=iat
        validationData = _packValidationData(
            false,
            uint48(claims.exp),
            uint48(claims.iat)
        );
    }

    // -----------------------------------------------------------------------
    // Receive ETH
    // -----------------------------------------------------------------------

    receive() external payable {}

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// @dev Extracts the base64url-encoded payload segment (between first and second dot)
    ///      from a raw JWT.
    function _extractPayloadSegment(bytes memory jwt) internal pure returns (bytes memory) {
        uint256 firstDot = type(uint256).max;
        uint256 secondDot = type(uint256).max;

        for (uint256 i = 0; i < jwt.length; i++) {
            if (jwt[i] == 0x2E) {
                if (firstDot == type(uint256).max) {
                    firstDot = i;
                } else {
                    secondDot = i;
                    break;
                }
            }
        }

        require(secondDot != type(uint256).max, "TerrariumWallet: malformed JWT");

        uint256 payloadLen = secondDot - firstDot - 1;
        bytes memory payload = new bytes(payloadLen);
        for (uint256 i = 0; i < payloadLen; i++) {
            payload[i] = jwt[firstDot + 1 + i];
        }
        return payload;
    }
}
