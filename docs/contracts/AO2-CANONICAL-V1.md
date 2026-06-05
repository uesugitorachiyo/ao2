# AO2 Canonical JSON v1

`ao2-canonical-v1` is the content-addressing contract used by AO2 and the
AO2 Control Plane. It is intentionally pinned to the existing implementation:

- JSON object keys sorted lexicographically by Rust `String` order.
- No insignificant whitespace.
- `serde_json::Number::to_string()` number formatting.
- Minimal string escaping for JSON syntax and control characters.
- SHA-256 is computed over the UTF-8 bytes of the canonical string.

This is not RFC 8785/JCS. Do not change the implementation toward JCS without
an explicit content-address migration, because that would rewrite existing
artifact digests and signatures.

Golden vectors live at `tests/fixtures/canonical-json-vectors.json` and are
run in both AO2 and AO2 Control Plane test suites.
