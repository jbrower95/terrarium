// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "forge-std/Test.sol";
import "../src/JwtValidator.sol";

/// @dev Harness to expose JwtValidator's internal/private functions for testing.
contract JwtValidatorHarness {
    function base64UrlDecode(bytes memory data) external pure returns (bytes memory) {
        return JwtValidator.base64UrlDecode(data);
    }

    function extractStringValue(bytes memory json, bytes memory key) external pure returns (bytes memory) {
        return JwtValidator._extractStringValue(json, key);
    }

    function extractNumberValue(bytes memory json, bytes memory key) external pure returns (uint256) {
        return JwtValidator._extractNumberValue(json, key);
    }

    function extractKid(bytes memory jwt) external pure returns (string memory) {
        return JwtValidator.extractKid(jwt);
    }

    function extractClaims(bytes memory payloadB64) external pure returns (JwtValidator.JwtClaims memory) {
        return JwtValidator.extractClaims(payloadB64);
    }
}

contract JwtValidatorTest is Test {
    JwtValidatorHarness h;

    function setUp() public {
        h = new JwtValidatorHarness();
    }

    // -----------------------------------------------------------------------
    // base64UrlDecode
    // -----------------------------------------------------------------------

    function test_base64UrlDecode_empty() public view {
        bytes memory result = h.base64UrlDecode("");
        assertEq(result.length, 0);
    }

    function test_base64UrlDecode_standardChars() public view {
        // "SGVsbG8" is base64url for "Hello"
        bytes memory result = h.base64UrlDecode("SGVsbG8");
        assertEq(string(result), "Hello");
    }

    function test_base64UrlDecode_withPadding_needed() public view {
        // "YQ" is base64url for "a" (needs 2 padding chars)
        bytes memory result = h.base64UrlDecode("YQ");
        assertEq(string(result), "a");
    }

    function test_base64UrlDecode_onePadding_needed() public view {
        // "YWI" is base64url for "ab" (needs 1 padding char)
        bytes memory result = h.base64UrlDecode("YWI");
        assertEq(string(result), "ab");
    }

    function test_base64UrlDecode_noPadding_needed() public view {
        // "YWJj" is base64url for "abc" (no padding needed, length % 4 == 0)
        bytes memory result = h.base64UrlDecode("YWJj");
        assertEq(string(result), "abc");
    }

    function test_base64UrlDecode_dashAndUnderscore() public view {
        // base64url uses '-' for '+' and '_' for '/'
        // Standard base64 "ab+c/d==" would be "ab-c_d" in base64url
        // Let's test with a known value: bytes [0x69, 0xBF, 0x9C, 0xFD]
        // Standard base64: "ab+c/Q==" -> base64url: "ab-c_Q"
        bytes memory result = h.base64UrlDecode("ab-c_Q");
        // Verify the dash and underscore were properly converted
        // "ab+c/Q==" decodes to [0x69, 0xBF, 0x9C, 0xFD]
        assertEq(result.length, 4);
        assertEq(uint8(result[0]), 0x69);
        assertEq(uint8(result[1]), 0xBF);
        assertEq(uint8(result[2]), 0x9C);
        assertEq(uint8(result[3]), 0xFD);
    }

    function test_base64UrlDecode_longerString() public view {
        // "SGVsbG8gV29ybGQ" is base64url for "Hello World"
        bytes memory result = h.base64UrlDecode("SGVsbG8gV29ybGQ");
        assertEq(string(result), "Hello World");
    }

    function test_base64UrlDecode_jsonPayload() public view {
        // Base64url encode of '{"sub":"1234567890"}'
        // Standard base64: eyJzdWIiOiIxMjM0NTY3ODkwIn0=
        // Base64url (no padding): eyJzdWIiOiIxMjM0NTY3ODkwIn0
        bytes memory result = h.base64UrlDecode("eyJzdWIiOiIxMjM0NTY3ODkwIn0");
        assertEq(string(result), '{"sub":"1234567890"}');
    }

    function test_base64UrlDecode_invalidChar_reverts() public {
        // '!' is not a valid base64 character
        vm.expectRevert(JwtValidator.MalformedJwt.selector);
        h.base64UrlDecode("!!!");
    }

    // -----------------------------------------------------------------------
    // _extractStringValue
    // -----------------------------------------------------------------------

    function test_extractStringValue_simple() public view {
        bytes memory json = '{"name":"alice"}';
        bytes memory result = h.extractStringValue(json, "name");
        assertEq(string(result), "alice");
    }

    function test_extractStringValue_withSpaces() public view {
        bytes memory json = '{"name" : "alice"}';
        bytes memory result = h.extractStringValue(json, "name");
        assertEq(string(result), "alice");
    }

    function test_extractStringValue_multipleKeys() public view {
        bytes memory json = '{"iss":"github","repo":"owner/repo","ref":"main"}';
        assertEq(string(h.extractStringValue(json, "iss")), "github");
        assertEq(string(h.extractStringValue(json, "repo")), "owner/repo");
        assertEq(string(h.extractStringValue(json, "ref")), "main");
    }

    function test_extractStringValue_missingKey_reverts() public {
        bytes memory json = '{"name":"alice"}';
        vm.expectRevert(abi.encodeWithSelector(JwtValidator.ClaimNotFound.selector, "missing"));
        h.extractStringValue(json, "missing");
    }

    function test_extractStringValue_emptyValue() public view {
        bytes memory json = '{"key":""}';
        bytes memory result = h.extractStringValue(json, "key");
        assertEq(result.length, 0);
    }

    function test_extractStringValue_withNewlines() public view {
        bytes memory json = '{\n  "name"\n  :\n  "alice"\n}';
        bytes memory result = h.extractStringValue(json, "name");
        assertEq(string(result), "alice");
    }

    function test_extractStringValue_withTabs() public view {
        bytes memory json = '{"name"\t:\t"alice"}';
        bytes memory result = h.extractStringValue(json, "name");
        assertEq(string(result), "alice");
    }

    function test_extractStringValue_urlValue() public view {
        bytes memory json = '{"iss":"https://token.actions.githubusercontent.com"}';
        bytes memory result = h.extractStringValue(json, "iss");
        assertEq(string(result), "https://token.actions.githubusercontent.com");
    }

    // -----------------------------------------------------------------------
    // _extractNumberValue
    // -----------------------------------------------------------------------

    function test_extractNumberValue_simple() public view {
        bytes memory json = '{"exp":1700000000}';
        uint256 result = h.extractNumberValue(json, "exp");
        assertEq(result, 1700000000);
    }

    function test_extractNumberValue_withSpaces() public view {
        bytes memory json = '{"exp" : 1700000000}';
        uint256 result = h.extractNumberValue(json, "exp");
        assertEq(result, 1700000000);
    }

    function test_extractNumberValue_zero() public view {
        bytes memory json = '{"val":0}';
        uint256 result = h.extractNumberValue(json, "val");
        assertEq(result, 0);
    }

    function test_extractNumberValue_missingKey_reverts() public {
        bytes memory json = '{"exp":123}';
        vm.expectRevert(abi.encodeWithSelector(JwtValidator.ClaimNotFound.selector, "missing"));
        h.extractNumberValue(json, "missing");
    }

    function test_extractNumberValue_multipleNumbers() public view {
        bytes memory json = '{"iat":1000,"exp":2000}';
        assertEq(h.extractNumberValue(json, "iat"), 1000);
        assertEq(h.extractNumberValue(json, "exp"), 2000);
    }

    function test_extractNumberValue_trailingComma() public view {
        // Number followed by comma should stop at the comma
        bytes memory json = '{"exp":12345,"iat":67890}';
        assertEq(h.extractNumberValue(json, "exp"), 12345);
    }

    function test_extractNumberValue_trailingBrace() public view {
        // Number at end of object should stop at the brace
        bytes memory json = '{"exp":99999}';
        assertEq(h.extractNumberValue(json, "exp"), 99999);
    }

    // -----------------------------------------------------------------------
    // extractKid
    // -----------------------------------------------------------------------

    function test_extractKid_simple() public view {
        // Build a mock JWT: base64url(header).base64url(payload).signature
        // Header: {"kid":"my-key-id","alg":"RS256"}
        // base64url: eyJraWQiOiJteS1rZXktaWQiLCJhbGciOiJSUzI1NiJ9
        bytes memory jwt = "eyJraWQiOiJteS1rZXktaWQiLCJhbGciOiJSUzI1NiJ9.cGF5bG9hZA.c2ln";
        string memory kid = h.extractKid(jwt);
        assertEq(kid, "my-key-id");
    }

    function test_extractKid_differentKid() public view {
        // Header: {"alg":"RS256","kid":"abc123"}
        // base64url: eyJhbGciOiJSUzI1NiIsImtpZCI6ImFiYzEyMyJ9
        bytes memory jwt = "eyJhbGciOiJSUzI1NiIsImtpZCI6ImFiYzEyMyJ9.cGF5bG9hZA.c2ln";
        string memory kid = h.extractKid(jwt);
        assertEq(kid, "abc123");
    }

    function test_extractKid_noDot_reverts() public {
        vm.expectRevert(JwtValidator.MalformedJwt.selector);
        h.extractKid("nodothere");
    }

    // -----------------------------------------------------------------------
    // extractClaims
    // -----------------------------------------------------------------------

    function test_extractClaims_fullPayload() public view {
        // Build a JSON payload with all required claims, then base64url encode it.
        // Payload: {"iss":"https://token.actions.githubusercontent.com","repository":"owner/repo","workflow":"deploy.yml","ref":"refs/heads/main","repository_visibility":"public","exp":1700000000,"iat":1699999000}
        // base64url encoded (verified):
        bytes memory payloadB64 = "eyJpc3MiOiJodHRwczovL3Rva2VuLmFjdGlvbnMuZ2l0aHVidXNlcmNvbnRlbnQuY29tIiwicmVwb3NpdG9yeSI6Im93bmVyL3JlcG8iLCJ3b3JrZmxvdyI6ImRlcGxveS55bWwiLCJyZWYiOiJyZWZzL2hlYWRzL21haW4iLCJyZXBvc2l0b3J5X3Zpc2liaWxpdHkiOiJwdWJsaWMiLCJleHAiOjE3MDAwMDAwMDAsImlhdCI6MTY5OTk5OTAwMH0";

        JwtValidator.JwtClaims memory claims = h.extractClaims(payloadB64);

        assertEq(string(claims.iss), "https://token.actions.githubusercontent.com");
        assertEq(string(claims.repository), "owner/repo");
        assertEq(string(claims.workflow), "deploy.yml");
        assertEq(string(claims.ref_), "refs/heads/main");
        assertEq(string(claims.repository_visibility), "public");
        assertEq(claims.exp, 1700000000);
        assertEq(claims.iat, 1699999000);
    }
}
