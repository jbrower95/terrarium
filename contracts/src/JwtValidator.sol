// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

/// @title JwtValidator
/// @notice Library for decoding and verifying JWTs with RSA-SHA256 signatures.
/// @dev This is a minimal implementation. JWT format: base64url(header).base64url(payload).base64url(signature)
///      The header contains {"kid":"...","alg":"RS256",...} which identifies the signing key.
///      Verification uses RSASSA-PKCS1-v1_5 with SHA-256 via the RIP-7212 precompile or fallback.
library JwtValidator {
    /// @notice Extracts the "kid" field from a JWT's header (the first dot-delimited segment).
    /// @param jwt The raw JWT bytes (ASCII encoded: header.payload.signature)
    /// @return kid The key ID string from the JWT header
    function extractKid(bytes memory jwt) internal pure returns (string memory kid) {
        // Find the first '.' to isolate the header segment
        uint256 firstDot = _indexOf(jwt, 0x2e, 0);
        require(firstDot > 0, "JwtValidator: invalid JWT format");

        // Decode the base64url header
        bytes memory headerJson = _base64UrlDecode(_slice(jwt, 0, firstDot));

        // Extract "kid" value from the JSON header.
        // We do a simple search for "kid":"<value>" — sufficient for well-formed JWT headers.
        kid = _extractJsonString(headerJson, '"kid"');
        require(bytes(kid).length > 0, "JwtValidator: kid not found in header");
    }

    /// @notice Verifies an RSA-SHA256 JWT signature against the provided public key.
    /// @param jwt The raw JWT bytes (ASCII)
    /// @param n The RSA modulus
    /// @param e The RSA public exponent
    /// @return valid True if the signature is valid
    function verifySignature(
        bytes memory jwt,
        bytes memory n,
        bytes memory e
    ) internal view returns (bool valid) {
        // Split JWT into signed data (header.payload) and signature
        uint256 firstDot = _indexOf(jwt, 0x2e, 0);
        uint256 secondDot = _indexOf(jwt, 0x2e, firstDot + 1);
        require(secondDot > firstDot, "JwtValidator: invalid JWT format");

        bytes memory signedData = _slice(jwt, 0, secondDot);
        bytes memory signatureB64 = _slice(jwt, secondDot + 1, jwt.length - secondDot - 1);
        bytes memory signature = _base64UrlDecode(signatureB64);

        // Hash the signed portion with SHA-256
        bytes32 digest = sha256(signedData);

        // Verify RSA PKCS#1 v1.5 signature
        valid = _rsaVerify(digest, signature, n, e);
    }

    // ─── Internal Helpers ────────────────────────────────────────────

    /// @dev Find the index of `needle` byte in `data` starting from `startIdx`.
    ///      Returns data.length if not found.
    function _indexOf(bytes memory data, bytes1 needle, uint256 startIdx) private pure returns (uint256) {
        for (uint256 i = startIdx; i < data.length; i++) {
            if (data[i] == needle) return i;
        }
        return data.length;
    }

    /// @dev Slice `length` bytes from `data` starting at `start`.
    function _slice(bytes memory data, uint256 start, uint256 length) private pure returns (bytes memory) {
        require(start + length <= data.length, "JwtValidator: slice out of bounds");
        bytes memory result = new bytes(length);
        for (uint256 i = 0; i < length; i++) {
            result[i] = data[start + i];
        }
        return result;
    }

    /// @dev Decode a base64url-encoded byte array (no padding required).
    function _base64UrlDecode(bytes memory input) private pure returns (bytes memory) {
        // Calculate padding needed
        uint256 padded = input.length;
        while (padded % 4 != 0) padded++;

        bytes memory std = new bytes(padded);
        for (uint256 i = 0; i < input.length; i++) {
            bytes1 c = input[i];
            if (c == 0x2d) std[i] = 0x2b;       // '-' -> '+'
            else if (c == 0x5f) std[i] = 0x2f;   // '_' -> '/'
            else std[i] = c;
        }
        for (uint256 i = input.length; i < padded; i++) {
            std[i] = 0x3d; // '='
        }

        return _base64Decode(std);
    }

    /// @dev Standard base64 decode (with padding).
    function _base64Decode(bytes memory input) private pure returns (bytes memory) {
        require(input.length % 4 == 0, "JwtValidator: invalid base64 length");

        uint256 decodedLen = (input.length / 4) * 3;
        if (input.length >= 1 && input[input.length - 1] == 0x3d) decodedLen--;
        if (input.length >= 2 && input[input.length - 2] == 0x3d) decodedLen--;

        bytes memory output = new bytes(decodedLen);
        uint256 outIdx = 0;

        for (uint256 i = 0; i < input.length; i += 4) {
            uint256 sextet0 = _base64CharValue(input[i]);
            uint256 sextet1 = _base64CharValue(input[i + 1]);
            uint256 sextet2 = (input[i + 2] == 0x3d) ? 0 : _base64CharValue(input[i + 2]);
            uint256 sextet3 = (input[i + 3] == 0x3d) ? 0 : _base64CharValue(input[i + 3]);

            uint256 triple = (sextet0 << 18) | (sextet1 << 12) | (sextet2 << 6) | sextet3;

            if (outIdx < decodedLen) output[outIdx++] = bytes1(uint8((triple >> 16) & 0xFF));
            if (outIdx < decodedLen) output[outIdx++] = bytes1(uint8((triple >> 8) & 0xFF));
            if (outIdx < decodedLen) output[outIdx++] = bytes1(uint8(triple & 0xFF));
        }

        return output;
    }

    /// @dev Get the 6-bit value for a base64 character.
    function _base64CharValue(bytes1 c) private pure returns (uint256) {
        uint8 v = uint8(c);
        if (v >= 0x41 && v <= 0x5a) return v - 0x41;       // A-Z
        if (v >= 0x61 && v <= 0x7a) return v - 0x61 + 26;   // a-z
        if (v >= 0x30 && v <= 0x39) return v - 0x30 + 52;   // 0-9
        if (v == 0x2b) return 62;                             // +
        if (v == 0x2f) return 63;                             // /
        revert("JwtValidator: invalid base64 char");
    }

    /// @dev Extract a JSON string value given a key like "kid". Expects "key":"value" format.
    function _extractJsonString(bytes memory json, bytes memory key) private pure returns (string memory) {
        // Find key in JSON
        uint256 keyIdx = _findBytes(json, key);
        if (keyIdx >= json.length) return "";

        // Move past key, skip colon and optional whitespace, find opening quote
        uint256 cursor = keyIdx + key.length;
        while (cursor < json.length && (json[cursor] == 0x3a || json[cursor] == 0x20)) {
            cursor++;
        }
        // Expect opening quote
        if (cursor >= json.length || json[cursor] != 0x22) return "";
        cursor++; // skip opening quote

        // Find closing quote
        uint256 valueStart = cursor;
        while (cursor < json.length && json[cursor] != 0x22) {
            cursor++;
        }

        return string(_slice(json, valueStart, cursor - valueStart));
    }

    /// @dev Find the starting index of `needle` in `haystack`. Returns haystack.length if not found.
    function _findBytes(bytes memory haystack, bytes memory needle) private pure returns (uint256) {
        if (needle.length > haystack.length) return haystack.length;
        uint256 limit = haystack.length - needle.length + 1;
        for (uint256 i = 0; i < limit; i++) {
            bool found = true;
            for (uint256 j = 0; j < needle.length; j++) {
                if (haystack[i + j] != needle[j]) {
                    found = false;
                    break;
                }
            }
            if (found) return i;
        }
        return haystack.length;
    }

    /// @dev RSA PKCS#1 v1.5 verification with SHA-256.
    ///      Uses the modexp precompile (address 0x05) for modular exponentiation.
    function _rsaVerify(
        bytes32 digest,
        bytes memory signature,
        bytes memory n,
        bytes memory e
    ) private view returns (bool) {
        // s^e mod n via the modexp precompile at address 0x05
        // Input format: <baseLen><expLen><modLen><base><exp><mod>
        uint256 baseLen = signature.length;
        uint256 expLen = e.length;
        uint256 modLen = n.length;

        bytes memory input = abi.encodePacked(
            bytes32(baseLen),
            bytes32(expLen),
            bytes32(modLen),
            signature,
            e,
            n
        );

        bytes memory result = new bytes(modLen);

        assembly {
            let success := staticcall(
                gas(),
                0x05,
                add(input, 0x20),
                mload(input),
                add(result, 0x20),
                modLen
            )
            if iszero(success) { revert(0, 0) }
        }

        // Build expected PKCS#1 v1.5 digest info for SHA-256
        // DigestInfo ::= SEQUENCE { algorithm AlgorithmIdentifier, digest OCTET STRING }
        // DER-encoded prefix for SHA-256: 30 31 30 0d 06 09 60 86 48 01 65 03 04 02 01 05 00 04 20
        bytes memory digestInfo = abi.encodePacked(
            hex"3031300d060960864801650304020105000420",
            digest
        );

        // Expected PKCS#1 v1.5 padded message: 0x00 0x01 [0xFF padding] 0x00 [digestInfo]
        // Total length must equal modLen
        uint256 paddingLen = modLen - digestInfo.length - 3;

        // Verify byte-by-byte
        if (result.length != modLen) return false;
        if (uint8(result[0]) != 0x00) return false;
        if (uint8(result[1]) != 0x01) return false;
        for (uint256 i = 0; i < paddingLen; i++) {
            if (uint8(result[2 + i]) != 0xFF) return false;
        }
        if (uint8(result[2 + paddingLen]) != 0x00) return false;
        for (uint256 i = 0; i < digestInfo.length; i++) {
            if (result[3 + paddingLen + i] != digestInfo[i]) return false;
        }

        return true;
    }
}
