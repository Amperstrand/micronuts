# cashu-core-lite

Minimal Cashu library for `no_std + alloc` environments.

## Build

```bash
cargo build
cargo test
```

## Architecture

Core types for Cashu V4 token operations on embedded hardware:

- Token V4 decode (CBOR via `minicbor`)
- Proof structure (amount, keyset_id, secret, signature)
- Blind message generation (`hash_to_curve` + blinding)
- Blind signature unblinding

## Key Constraints

- **no_std + alloc**: All heap allocations go through a global allocator. The micronuts firmware uses `linked_list_allocator` backed by SDRAM.
- **CBOR only**: Supports V4 tokens (CBOR-encoded). V3 JSON tokens are not supported.
- **k256**: Uses `k256` crate for secp256k1 operations (blinding, unblinding). `default-features = false` to avoid std.

## Dependencies

- `k256` — secp256k1 elliptic curve (ecdsa, arithmetic features)
- `sha2` — SHA-256 for hash_to_curve
- `minicbor` — CBOR encoding/decoding with derive macros
- `rand_core` — RNG trait (consumer provides implementation)

## Testing

```bash
cargo test
```
