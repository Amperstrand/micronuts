# Cashu Core Lite - CDK Porting Status

This document tracks the implementation status of Cashu NUTs in `cashu-core-lite` compared to the upstream [CDK](https://github.com/cashubtc/cdk) reference implementation.

## Implementation Summary

| NUT | Status | CDK Parity | Notes |
|-----|--------|-----------|-------|
| NUT-00 | ✅ Complete | Partial | Core types only, no DHKE |
| NUT-01 | ✅ Complete | Partial | Keyset types, no rotation |
| NUT-02 | ✅ Complete | Full | Keyset ID derivation |
| NUT-03 | ✅ Complete | Full | Swap request/response types |
| NUT-04 | ✅ Complete | Partial | Mint types, demo shortcuts |
| NUT-05 | ✅ Complete | Partial | Melt types, demo shortcuts |
| NUT-06 | ✅ Complete | Partial | Info types, minimal |
| NUT-07 | ✅ Complete | Partial | State check, in-memory only |
| NUT-08 | ❌ Missing | - | Fee return |
| NUT-09 | ❌ Missing | - | Restore (uses NUT-13) |
| NUT-10 | ❌ Missing | - | Spending conditions |
| NUT-11 | ❌ Missing | - | Pay-to-script |
| NUT-12 | ❌ Missing | - | DLEQ proofs |
| NUT-13 | ✅ Complete | Full | Deterministic secrets/blinders |
| NUT-14 | ❌ Missing | - | HTLCs |
| NUT-15 | ❌ Missing | - | Multipart tokens |

## Detailed Implementation Status

### NUT-00: Notation, ID, and Units ✅

**File:** `src/nuts/nut00.rs`

| Component | Status | CDK Equivalent |
|-----------|--------|----------------|
| `Proof` struct | ✅ | `cdk::nuts::nut00::Proof` |
| `BlindedMessage` struct | ✅ | `cdk::nuts::nut00::BlindedMessage` |
| `BlindSignature` struct | ✅ | `cdk::nuts::nut00::BlindSignature` |
| `decompose_amount()` | ✅ | `cdk::amount::split()` |
| CBOR encode/decode | ✅ | Uses `minicbor` (CDK uses `serde`) |

**Missing vs CDK:**
- No `SecretKey` type (we use raw bytes)
- No DHKE (Diffie-Hellman) support
- No `DleqProof` struct (NUT-12 dependency)

**Tests:** 1 test (`test_decompose_amount`)

---

### NUT-01: Mint Public Keys ✅

**File:** `src/nuts/nut01.rs`

| Component | Status | CDK Equivalent |
|-----------|--------|----------------|
| `KeyPair` struct | ✅ | `cdk::nuts::nut01::KeyPair` |
| `KeySet` struct | ✅ | `cdk::nuts::nut01::KeySet` |
| `KeysResponse` struct | ✅ | `cdk::nuts::nut01::KeysResponse` |

**Missing vs CDK:**
- No keyset rotation/expiration tracking
- No `CurrencyUnit` enum

**Tests:** None (type-only module)

---

### NUT-02: Keyset ID Derivation ✅

**File:** `src/nuts/nut02.rs`

| Component | Status | CDK Equivalent |
|-----------|--------|----------------|
| `derive_keyset_id()` | ✅ | `cdk::nuts::nut02::derive_keyset_id()` |

**Algorithm Match:** Full - SHA-256 of sorted, compressed public keys

**Tests:** None

---

### NUT-03: Swap (Split) ✅

**File:** `src/nuts/nut03.rs`

| Component | Status | CDK Equivalent |
|-----------|--------|----------------|
| `SwapRequest` struct | ✅ | `cdk::nuts::nut03::SwapRequest` |
| `SwapResponse` struct | ✅ | `cdk::nuts::nut03::SwapResponse` |

**Missing vs CDK:**
- No swap fee calculation
- No private route swap logic

**Tests:** None

---

### NUT-04: Mint Tokens (Bolt11) ✅

**File:** `src/nuts/nut04.rs`

| Component | Status | CDK Equivalent |
|-----------|--------|----------------|
| `MintQuoteRequest` | ✅ | `cdk::nuts::nut04::MintQuoteBolt11Request` |
| `MintQuoteResponse` | ✅ | `cdk::nuts::nut04::MintQuoteResponse` |
| `MintRequest` | ✅ | `cdk::nuts::nut04::MintBolt11Request` |
| `MintResponse` | ✅ | `cdk::nuts::nut04::MintResponse` |
| `state` module | ✅ | `UNPAID`, `PAID`, `ISSUED` |

**Demo Shortcuts:**
- `request` field accepts any string (no real Lightning invoice validation)
- `expiry` set far in future

**Tests:** None

---

### NUT-05: Melt Tokens (Bolt11) ✅

**File:** `src/nuts/nut05.rs`

| Component | Status | CDK Equivalent |
|-----------|--------|----------------|
| `MeltQuoteRequest` | ✅ | `cdk::nuts::nut05::MeltQuoteBolt11Request` |
| `MeltQuoteResponse` | ✅ | `cdk::nuts::nut05::MeltQuoteResponse` |
| `MeltRequest` | ✅ | `cdk::nuts::nut05::MeltBolt11Request` |
| `MeltResponse` | ✅ | `cdk::nuts::nut05::MeltResponse` |
| `state` module | ✅ | `UNPAID`, `PENDING`, `PAID` |

**Demo Shortcuts:**
- `fee_reserve` always 0
- `payment_preimage` is dummy hex

**Tests:** None

---

### NUT-06: Mint Information ✅

**File:** `src/nuts/nut06.rs`

| Component | Status | CDK Equivalent |
|-----------|--------|----------------|
| `MintInfo` struct | ✅ | `cdk::nuts::nut06::MintInfo` |
| `ContactInfo` struct | ✅ | `cdk::nuts::nut06::ContactInfo` |
| `NutSupport` struct | ⚠️ Minimal | CDK has per-NUT settings objects |

**Missing vs CDK:**
- No per-NUT settings (e.g., NUT-04/05 payment methods)
- No `motd` (message of the day)
- No `icon_url`

**Tests:** None

---

### NUT-07: Token State Check ✅

**File:** `src/nuts/nut07.rs`

| Component | Status | CDK Equivalent |
|-----------|--------|----------------|
| `CheckStateRequest` | ✅ | `cdk::nuts::nut07::CheckStateRequest` |
| `CheckStateResponse` | ✅ | `cdk::nuts::nut07::CheckStateResponse` |
| `ProofState` struct | ✅ | `cdk::nuts::nut07::ProofState` |
| `state` module | ✅ | `UNSPENT`, `SPENT`, `PENDING` |

**Limitations:**
- In-memory state only (no persistence)
- No witness data validation

**Tests:** None

---

### NUT-13: Deterministic Secrets & Blinders ✅

**File:** `src/nuts/nut13.rs`

| Component | Status | CDK Equivalent |
|-----------|--------|----------------|
| `derive_secret()` | ✅ | `cdk::nuts::nut13::derive_secret()` |
| `derive_blinder()` | ✅ | `cdk::nuts::nut13::derive_blinder()` |
| `keyset_id_to_u32()` | ✅ | Helper for legacy paths |
| `hmac_sha256()` | ✅ | RFC 2104 compliant |

**Algorithm Match:** Full - HMAC-SHA256 KDF for v01+ keysets

**CDK Reference:** [nut13.rs](https://github.com/cashubtc/cdk/blob/main/crates/cdk/src/nuts/nut13.rs)

**Test Vectors:** Verified against [13-tests.md](https://github.com/cashubtc/nuts/blob/main/tests/13-tests.md)

**Tests:** 12 unit tests (all passing)

| Test | Description |
|------|-------------|
| `test_hmac_sha256_basic` | HMAC consistency check |
| `test_derive_secret_deterministic` | Same inputs = same output |
| `test_derive_blinder_deterministic` | Same inputs = same output |
| `test_secret_vs_blinder_different` | 0x00 vs 0x01 derivation type |
| `test_different_counters_different_secrets` | Counter uniqueness |
| `test_different_seeds_different_secrets` | Seed uniqueness |
| `test_keyset_id_to_u32` | Hex to u32 conversion |
| `test_keyset_id_to_u32_invalid` | Error handling |
| `test_derive_secret_invalid_keyset_id` | Input validation |
| `test_hex_decode_keyset_id` | Hex decoding |
| `test_derive_secret_v2_official_test_vectors` | Official spec vectors |
| `test_derive_blinder_v2_official_test_vectors` | Official spec vectors |

---

### Crypto Module ✅

**File:** `src/crypto.rs`

| Function | Status | CDK Equivalent |
|----------|--------|----------------|
| `hash_to_curve()` | ✅ | `cdk::crypto::hash_to_curve()` |
| `blind_message()` | ✅ | `cdk::crypto::blind_message()` |
| `unblind_signature()` | ✅ | `cdk::crypto::unblind_signature()` |
| `sign_message()` | ✅ | `cdk::crypto::sign_message()` |
| `verify_signature()` | ✅ | `cdk::crypto::verify_signature()` |

**Algorithm Match:** Full - Uses same `Secp256k1_HashToCurve_Cashu_` domain separator

**Tests:** 2 tests in `tests/crypto.rs`

---

## Missing NUTs (Priority Order)

### High Priority (Core Wallet Functionality)

| NUT | Description | Blocked By | Effort |
|-----|-------------|------------|--------|
| NUT-09 | Restore | NUT-13 ✅ | Medium |
| NUT-08 | Fee Return | - | Low |
| NUT-12 | DLEQ Proofs | - | Medium |

### Medium Priority (Advanced Features)

| NUT | Description | Dependencies | Effort |
|-----|-------------|--------------|--------|
| NUT-10 | Spending Conditions | - | Medium |
| NUT-11 | Pay-to-Script | NUT-10 | High |

### Lower Priority (Specialized)

| NUT | Description | Dependencies | Effort |
|-----|-------------|--------------|--------|
| NUT-14 | HTLCs | - | High |
| NUT-15 | Multipart | - | Medium |

---

## Test Coverage Comparison

### CDK Test Pattern

CDK uses comprehensive test coverage:
- Unit tests per NUT module
- Integration tests with mock mints
- Property-based testing with `proptest`
- Test vectors from official NUT specs

### Our Current Coverage

| Module | Unit Tests | Integration | Test Vectors |
|--------|-----------|-------------|--------------|
| nut00 | 1 | ❌ | ❌ |
| nut01 | 0 | ❌ | ❌ |
| nut02 | 0 | ❌ | ❌ |
| nut03 | 0 | ❌ | ❌ |
| nut04 | 0 | ❌ | ❌ |
| nut05 | 0 | ❌ | ❌ |
| nut06 | 0 | ❌ | ❌ |
| nut07 | 0 | ❌ | ❌ |
| nut13 | 12 | ❌ | ✅ |
| crypto | 2 | ❌ | Partial |

### CI Status

**Current CI:** `.github/workflows/rust-ci.yml`

```yaml
# Runs:
- cargo test -p cashu-core-lite --features std
- cargo test -p micronuts-mint
- cargo test -p micronuts-fips-bridge
- cargo run -p micronuts-mint --bin demo
- cargo build -p firmware --target thumbv7em-none-eabihf
```

**Missing:**
- No test vector validation in CI
- No coverage reports
- No embedded target tests (hardware only)

---

## Recommendations

### Immediate Actions

1. **Add test vectors** for all NUTs with official test data
2. **Increase unit test coverage** for nut00-nut07
3. **Add CI step** to verify test vectors against spec

### Medium Term

1. **Implement NUT-09** (Restore) - now unblocked by NUT-13 ✅
2. **Implement NUT-08** (Fee Return) - required for melt operations
3. **Implement NUT-12** (DLEQ) - enables trustless verification

### Documentation

1. Add inline documentation linking to CDK source files
2. Document demo shortcuts clearly
3. Add migration guide for users coming from CDK

---

## CDK Files Reference

| CDK File | Our File | Parity |
|----------|----------|-------|
| `cdk/src/nuts/nut00.rs` | `nuts/nut00.rs` | Partial |
| `cdk/src/nuts/nut01.rs` | `nuts/nut01.rs` | Partial |
| `cdk/src/nuts/nut02.rs` | `nuts/nut02.rs` | Full |
| `cdk/src/nuts/nut03.rs` | `nuts/nut03.rs` | Full |
| `cdk/src/nuts/nut04.rs` | `nuts/nut04.rs` | Partial |
| `cdk/src/nuts/nut05.rs` | `nuts/nut05.rs` | Partial |
| `cdk/src/nuts/nut06.rs` | `nuts/nut06.rs` | Partial |
| `cdk/src/nuts/nut07.rs` | `nuts/nut07.rs` | Partial |
| `cdk/src/nuts/nut13.rs` | `nuts/nut13.rs` | Full |
| `cdk/src/crypto.rs` | `crypto.rs` | Full |

---

## Last Updated

2025-04-05
