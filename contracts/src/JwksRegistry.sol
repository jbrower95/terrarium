// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import {JwtValidator} from "./JwtValidator.sol";

/// @title JwksRegistry
/// @notice Stores RSA public keys for OIDC JWT verification, keyed by kid (Key ID).
///         New keys can only be added by presenting a JWT signed by an already-trusted key,
///         making the registry self-bootstrapping after initial deployment.
contract JwksRegistry {
    struct RsaKey {
        bytes n;        // RSA modulus
        bytes e;        // RSA public exponent
        uint256 addedAt; // block.timestamp when the key was stored
    }

    /// @notice Emitted when a key is added during construction.
    event KeyAdded(string kid);

    /// @notice Emitted when a new key is added via rotation (proving possession of a signed JWT).
    event KeyRotated(string oldKid, string newKid);

    /// @dev kid => RsaKey
    mapping(bytes32 => RsaKey) private _keys;

    /// @dev Tracks which kid hashes have been stored, so we can distinguish
    ///      "empty key" from "never set".
    mapping(bytes32 => bool) private _exists;

    constructor(string memory initialKid, bytes memory initialN, bytes memory initialE) {
        require(initialN.length > 0, "JwksRegistry: empty modulus");
        require(initialE.length > 0, "JwksRegistry: empty exponent");

        bytes32 kidHash = keccak256(bytes(initialKid));
        _keys[kidHash] = RsaKey({n: initialN, e: initialE, addedAt: block.timestamp});
        _exists[kidHash] = true;

        emit KeyAdded(initialKid);
    }

    /// @notice Returns the RSA public key for the given kid.
    /// @param kid The key ID string from a JWT header.
    /// @return n The RSA modulus.
    /// @return e The RSA public exponent.
    function getKey(string memory kid) external view returns (bytes memory n, bytes memory e) {
        bytes32 kidHash = keccak256(bytes(kid));
        require(_exists[kidHash], "JwksRegistry: key not found");

        RsaKey storage key = _keys[kidHash];
        return (key.n, key.e);
    }

    /// @notice Checks whether a key with the given kid exists in the registry.
    /// @param kid The key ID string.
    /// @return True if the key exists.
    function hasKey(string memory kid) external view returns (bool) {
        return _exists[keccak256(bytes(kid))];
    }

    /// @notice Adds a new key by proving a JWT was signed by an existing trusted key.
    /// @dev Permissionless: anyone can call this as long as they supply a valid JWT
    ///      signed by a key already in the registry.
    /// @param newKid  The key ID for the new key to store.
    /// @param newN    The RSA modulus of the new key.
    /// @param newE    The RSA public exponent of the new key.
    /// @param jwt     A raw JWT (ASCII bytes) whose header references an existing kid
    ///                and whose signature is valid under that kid's stored key.
    function updateKey(
        string memory newKid,
        bytes memory newN,
        bytes memory newE,
        bytes memory jwt
    ) external {
        require(newN.length > 0, "JwksRegistry: empty modulus");
        require(newE.length > 0, "JwksRegistry: empty exponent");

        // Extract the signing kid from the JWT header
        string memory signingKid = JwtValidator.extractKid(jwt);
        bytes32 signingKidHash = keccak256(bytes(signingKid));

        // The JWT must be signed by a key we already trust
        require(_exists[signingKidHash], "JwksRegistry: signing key not found");

        RsaKey storage signingKey = _keys[signingKidHash];

        // Verify the JWT signature against the stored key
        bool valid = JwtValidator.verifySignature(jwt, signingKey.n, signingKey.e);
        require(valid, "JwksRegistry: invalid JWT signature");

        // Store the new key
        bytes32 newKidHash = keccak256(bytes(newKid));
        _keys[newKidHash] = RsaKey({n: newN, e: newE, addedAt: block.timestamp});
        _exists[newKidHash] = true;

        emit KeyRotated(signingKid, newKid);
    }
}
