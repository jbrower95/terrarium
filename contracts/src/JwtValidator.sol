// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import {RSA} from "@openzeppelin/contracts/utils/cryptography/RSA.sol";

/// @title JwtValidator
/// @notice Library for verifying GitHub Actions OIDC JWTs on-chain.
///         Verifies RSA-PKCS1-SHA256 signatures and extracts claims from the payload.
library JwtValidator {
    // -----------------------------------------------------------------------
    // Errors
    // -----------------------------------------------------------------------

    error InvalidSignature();
    error MalformedJwt();
    error ClaimNotFound(string claim);

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    struct JwtClaims {
        bytes iss;                    // "https://token.actions.githubusercontent.com"
        bytes repository;             // "owner/repo"
        bytes workflow;               // "owner.yml"
        bytes ref_;                   // "refs/heads/main"
        bytes repository_visibility;  // "public"
        uint256 exp;                  // expiration timestamp
        uint256 iat;                  // issued at timestamp
    }

    // -----------------------------------------------------------------------
    // Convenience functions for JwksRegistry
    // -----------------------------------------------------------------------

    /// @notice Extracts the "kid" (Key ID) from a raw JWT's header.
    /// @param jwt The full JWT as bytes (header.payload.signature in ASCII)
    /// @return kid The key ID string from the JWT header
    function extractKid(bytes memory jwt) internal pure returns (string memory kid) {
        // Find first '.' to isolate the header segment
        uint256 firstDot = type(uint256).max;
        for (uint256 i = 0; i < jwt.length; i++) {
            if (jwt[i] == 0x2E) { firstDot = i; break; }
        }
        if (firstDot == type(uint256).max) revert MalformedJwt();

        // Extract and decode the header
        bytes memory headerB64 = new bytes(firstDot);
        for (uint256 i = 0; i < firstDot; i++) headerB64[i] = jwt[i];
        bytes memory headerJson = base64UrlDecode(headerB64);

        // Extract "kid" value
        bytes memory kidBytes = _extractStringValue(headerJson, "kid");
        kid = string(kidBytes);
    }

    /// @notice Verifies an RSA-SHA256 JWT signature given the raw JWT and public key.
    /// @param jwt The full JWT as bytes (header.payload.signature in ASCII)
    /// @param n   RSA modulus
    /// @param e   RSA exponent
    /// @return valid True if the signature is valid
    function verifySignature(
        bytes memory jwt,
        bytes memory n,
        bytes memory e
    ) internal view returns (bool valid) {
        // Find the two dots
        uint256 firstDot = type(uint256).max;
        uint256 secondDot = type(uint256).max;
        for (uint256 i = 0; i < jwt.length; i++) {
            if (jwt[i] == 0x2E) {
                if (firstDot == type(uint256).max) firstDot = i;
                else { secondDot = i; break; }
            }
        }
        if (secondDot == type(uint256).max) revert MalformedJwt();

        // signed data = header.payload (everything before second dot)
        bytes memory signedData = new bytes(secondDot);
        for (uint256 i = 0; i < secondDot; i++) signedData[i] = jwt[i];

        // signature = base64url-decode of everything after second dot
        uint256 sigLen = jwt.length - secondDot - 1;
        bytes memory sigB64 = new bytes(sigLen);
        for (uint256 i = 0; i < sigLen; i++) sigB64[i] = jwt[secondDot + 1 + i];
        bytes memory signature = base64UrlDecode(sigB64);

        // Verify using OpenZeppelin RSA
        valid = RSA.pkcs1Sha256(signedData, signature, e, n);
    }

    // -----------------------------------------------------------------------
    // Main verification entry point
    // -----------------------------------------------------------------------

    /// @notice Verifies the RSA-PKCS1-SHA256 signature of a JWT and extracts claims.
    /// @param headerB64  Base64url-encoded JWT header (not used for claims, but part of signed data)
    /// @param payloadB64 Base64url-encoded JWT payload
    /// @param signature  Raw RSA signature bytes (decoded from base64url)
    /// @param e          RSA public key exponent
    /// @param n          RSA public key modulus
    /// @return claims    The parsed JWT claims
    function verifyAndExtract(
        bytes memory headerB64,
        bytes memory payloadB64,
        bytes memory signature,
        bytes memory e,
        bytes memory n
    ) internal view returns (JwtClaims memory claims) {
        // The signed message for a JWT is: base64url(header) + "." + base64url(payload)
        bytes memory signedMessage = abi.encodePacked(headerB64, ".", payloadB64);

        // Verify RSA-PKCS1-SHA256 signature
        bool valid = RSA.pkcs1Sha256(signedMessage, signature, e, n);
        if (!valid) revert InvalidSignature();

        // Decode the payload from base64url and parse claims
        bytes memory payloadJson = base64UrlDecode(payloadB64);
        claims = _parseClaims(payloadJson);
    }

    /// @notice Verifies the RSA-PKCS1-SHA256 signature of a JWT without extracting claims.
    /// @param headerB64  Base64url-encoded JWT header
    /// @param payloadB64 Base64url-encoded JWT payload
    /// @param signature  Raw RSA signature bytes
    /// @param e          RSA public key exponent
    /// @param n          RSA public key modulus
    /// @return valid     Whether the signature is valid
    function verify(
        bytes memory headerB64,
        bytes memory payloadB64,
        bytes memory signature,
        bytes memory e,
        bytes memory n
    ) internal view returns (bool valid) {
        bytes memory signedMessage = abi.encodePacked(headerB64, ".", payloadB64);
        valid = RSA.pkcs1Sha256(signedMessage, signature, e, n);
    }

    /// @notice Decodes the base64url-encoded payload and extracts claims without signature
    ///         verification. Useful when signature has been verified separately.
    /// @param payloadB64 Base64url-encoded JWT payload
    /// @return claims    The parsed JWT claims
    function extractClaims(bytes memory payloadB64) internal pure returns (JwtClaims memory claims) {
        bytes memory payloadJson = base64UrlDecode(payloadB64);
        claims = _parseClaims(payloadJson);
    }

    // -----------------------------------------------------------------------
    // Base64url decoding
    // -----------------------------------------------------------------------

    /// @notice Decodes a base64url-encoded byte string.
    /// @dev Base64url uses '-' instead of '+', '_' instead of '/', and no padding '='.
    /// @param data The base64url-encoded input
    /// @return result The decoded bytes
    function base64UrlDecode(bytes memory data) internal pure returns (bytes memory result) {
        uint256 len = data.length;
        if (len == 0) return "";

        // Calculate the number of padding characters needed
        uint256 paddingNeeded = (4 - (len % 4)) % 4;
        uint256 paddedLen = len + paddingNeeded;

        // Create a standard base64 string with proper padding
        bytes memory base64Data = new bytes(paddedLen);
        for (uint256 i = 0; i < len; i++) {
            bytes1 c = data[i];
            if (c == 0x2D) {
                // '-' -> '+'
                base64Data[i] = 0x2B;
            } else if (c == 0x5F) {
                // '_' -> '/'
                base64Data[i] = 0x2F;
            } else {
                base64Data[i] = c;
            }
        }
        // Add '=' padding
        for (uint256 i = len; i < paddedLen; i++) {
            base64Data[i] = 0x3D; // '='
        }

        // Decode standard base64
        result = _decodeBase64(base64Data);
    }

    /// @dev Decodes standard base64 (with padding).
    function _decodeBase64(bytes memory data) private pure returns (bytes memory) {
        uint256 len = data.length;
        if (len == 0) return "";
        require(len % 4 == 0, "JwtValidator: invalid base64 length");

        // Count padding characters to determine output length
        uint256 padding = 0;
        if (data[len - 1] == 0x3D) padding++;
        if (len >= 2 && data[len - 2] == 0x3D) padding++;

        uint256 outputLen = (len / 4) * 3 - padding;
        bytes memory result = new bytes(outputLen);

        uint256 j = 0;
        for (uint256 i = 0; i < len; i += 4) {
            uint256 a = _base64CharToValue(data[i]);
            uint256 b = _base64CharToValue(data[i + 1]);
            uint256 c = _base64CharToValue(data[i + 2]);
            uint256 d = _base64CharToValue(data[i + 3]);

            uint256 triple = (a << 18) | (b << 12) | (c << 6) | d;

            if (j < outputLen) result[j++] = bytes1(uint8(triple >> 16));
            if (j < outputLen) result[j++] = bytes1(uint8(triple >> 8));
            if (j < outputLen) result[j++] = bytes1(uint8(triple));
        }

        return result;
    }

    /// @dev Maps a base64 character to its 6-bit value.
    function _base64CharToValue(bytes1 c) private pure returns (uint256) {
        uint8 v = uint8(c);
        if (v >= 0x41 && v <= 0x5A) return v - 0x41;       // A-Z -> 0-25
        if (v >= 0x61 && v <= 0x7A) return v - 0x61 + 26;   // a-z -> 26-51
        if (v >= 0x30 && v <= 0x39) return v - 0x30 + 52;   // 0-9 -> 52-61
        if (v == 0x2B) return 62;                            // +
        if (v == 0x2F) return 63;                            // /
        if (v == 0x3D) return 0;                             // = (padding)
        revert MalformedJwt();
    }

    // -----------------------------------------------------------------------
    // JSON parsing (minimal, for well-formed GitHub OIDC JWTs)
    // -----------------------------------------------------------------------

    /// @dev Parses all required claims from the JSON payload.
    function _parseClaims(bytes memory json) private pure returns (JwtClaims memory claims) {
        claims.iss = _extractStringValue(json, "iss");
        claims.repository = _extractStringValue(json, "repository");
        claims.workflow = _extractStringValue(json, "workflow");
        claims.ref_ = _extractStringValue(json, "ref");
        claims.repository_visibility = _extractStringValue(json, "repository_visibility");
        claims.exp = _extractNumberValue(json, "exp");
        claims.iat = _extractNumberValue(json, "iat");
    }

    /// @notice Extracts a JSON string value for a given key.
    /// @dev Searches for `"key":"value"` or `"key": "value"` patterns.
    ///      Does not handle escaped quotes inside values.
    /// @param json The JSON bytes
    /// @param key  The key to search for (without quotes)
    /// @return value The extracted value as bytes
    function _extractStringValue(bytes memory json, bytes memory key) internal pure returns (bytes memory) {
        // Build the search pattern: `"key"`
        bytes memory pattern = abi.encodePacked('"', key, '"');

        uint256 keyPos = _findBytes(json, pattern, 0);
        if (keyPos == type(uint256).max) revert ClaimNotFound(string(key));

        // Move past the key and find the colon
        uint256 pos = keyPos + pattern.length;
        pos = _skipWhitespace(json, pos);
        if (pos >= json.length || json[pos] != 0x3A) revert MalformedJwt(); // ':'
        pos++;

        // Skip whitespace after colon
        pos = _skipWhitespace(json, pos);
        if (pos >= json.length || json[pos] != 0x22) revert MalformedJwt(); // '"'
        pos++; // skip opening quote

        // Find the closing quote
        uint256 valueStart = pos;
        while (pos < json.length && json[pos] != 0x22) {
            pos++;
        }
        if (pos >= json.length) revert MalformedJwt();

        // Extract the value
        uint256 valueLen = pos - valueStart;
        bytes memory value = new bytes(valueLen);
        for (uint256 i = 0; i < valueLen; i++) {
            value[i] = json[valueStart + i];
        }
        return value;
    }

    /// @notice Extracts a JSON number value for a given key.
    /// @dev Searches for `"key":number` or `"key": number` patterns.
    /// @param json The JSON bytes
    /// @param key  The key to search for (without quotes)
    /// @return value The extracted numeric value
    function _extractNumberValue(bytes memory json, bytes memory key) internal pure returns (uint256) {
        // Build the search pattern: `"key"`
        bytes memory pattern = abi.encodePacked('"', key, '"');

        uint256 keyPos = _findBytes(json, pattern, 0);
        if (keyPos == type(uint256).max) revert ClaimNotFound(string(key));

        // Move past the key and find the colon
        uint256 pos = keyPos + pattern.length;
        pos = _skipWhitespace(json, pos);
        if (pos >= json.length || json[pos] != 0x3A) revert MalformedJwt(); // ':'
        pos++;

        // Skip whitespace after colon
        pos = _skipWhitespace(json, pos);

        // Parse the number (unsigned integer)
        uint256 value = 0;
        bool found = false;
        while (pos < json.length) {
            uint8 c = uint8(json[pos]);
            if (c >= 0x30 && c <= 0x39) {
                value = value * 10 + (c - 0x30);
                found = true;
                pos++;
            } else {
                break;
            }
        }
        if (!found) revert MalformedJwt();
        return value;
    }

    // -----------------------------------------------------------------------
    // Internal string utilities
    // -----------------------------------------------------------------------

    /// @dev Finds the first occurrence of `needle` in `haystack` starting from `startPos`.
    ///      Returns type(uint256).max if not found.
    function _findBytes(
        bytes memory haystack,
        bytes memory needle,
        uint256 startPos
    ) private pure returns (uint256) {
        uint256 hLen = haystack.length;
        uint256 nLen = needle.length;
        if (nLen == 0 || nLen > hLen) return type(uint256).max;

        uint256 end = hLen - nLen;
        for (uint256 i = startPos; i <= end; i++) {
            bool found = true;
            for (uint256 j = 0; j < nLen; j++) {
                if (haystack[i + j] != needle[j]) {
                    found = false;
                    break;
                }
            }
            if (found) return i;
        }
        return type(uint256).max;
    }

    /// @dev Skips whitespace characters (space, tab, newline, carriage return).
    function _skipWhitespace(bytes memory data, uint256 pos) private pure returns (uint256) {
        while (pos < data.length) {
            bytes1 c = data[pos];
            if (c != 0x20 && c != 0x09 && c != 0x0A && c != 0x0D) break;
            pos++;
        }
        return pos;
    }
}
