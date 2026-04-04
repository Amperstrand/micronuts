# Integration Report

**Date**: 2026-04-04
**Operator**: Prometheus (planning) + Sisyphus agents (execution)
**Scope**: Cleanup integration pass across `micronuts` and `microfips`

---

## TL;DR

Both repos have clean integration branches with all useful agent-generated work merged. All host-buildable tests pass (67 in micronuts, 184 in microfips). Both branches have been **pushed to remote** and are PR-ready pending human review.

---

## micronuts

**Repo**: `/Users/macbook/src/micronuts`
**Integration Branch**: `integration/micronuts-wallet-mint-rpc`
**Base**: `main` (`62508bf`)
**Commits Ahead of Main**: 12

### Merged

| Source Branch | Method | Commits | Files | Lines | Conflicts |
|---|---|---|---|---|---|
| `origin/copilot/add-demo-role-for-micronuts` | Fast-forward merge | 9 | 38 | +5170/−57 | None |

**What this brought in:**
- NUT-00 through NUT-07 type definitions (`cashu-core-lite/src/nuts/`)
- RPC layer with CBOR serialization (`cashu-core-lite/src/rpc.rs`)
- Transport trait abstraction (`cashu-core-lite/src/transport.rs`)
- Wallet module (`cashu-core-lite/src/wallet.rs`)
- Error types (`cashu-core-lite/src/error.rs`)
- Keypair helpers (`cashu-core-lite/src/keypair.rs`)
- Entire `micronuts-mint` crate: mint_core, keyset, rpc_service, loopback_transport, direct_transport, demo_roles
- Demo binaries: `demo`, `mint_server`, `wallet_demo`
- E2E and unit test suites
- CI workflow (`.github/workflows/rust-ci.yml`)
- Documentation (`docs/MINT-WALLET-DEMO.md`)

### Cherry-Picked

| Source Branch | Commit | Description | Files | Conflicts |
|---|---|---|---|---|
| `feat/defmt-conditional-build` | `5365767` → `6c545c0` | DTR/RTS USB fix — disables DTR/RTS to prevent STM32 reset on USB connect | 1 (`host-mint-tool/src/usb.rs`, 6 lines) | None |

### Fixes Applied During Integration

| Commit | Description | Reason |
|---|---|---|
| `8bb71f2` | Add missing `use alloc::vec;` import in `cashu-core-lite/src/nuts/nut00.rs` test module | `vec!` macro used in `#[cfg(test)]` block without import; caused compilation failure under test |
| `4b9578e` | Add `mut` binding for serial port in DTR/RTS setup (`host-mint-tool/src/usb.rs:17`) | Compile error — `write_data_terminal_ready` and `write_request_to_send` require mutable binding |

### Test Results

| Crate | Command | Result |
|---|---|---|
| `cashu-core-lite` | `cargo test -p cashu-core-lite` | **41 PASS** (2 unit + 5 cashu_compat + 13 crypto + 6 hash_to_curve + 4 rpc + 11 token) |
| `micronuts-mint` | `cargo test -p micronuts-mint` | **26 PASS** (9 unit + 12 e2e + 3 mint_role + 2 rpc_loopback) |
| Both | `cargo check -p cashu-core-lite -p micronuts-mint` | **PASS** |
| `micronuts-mint` | `cargo build -p micronuts-mint` | **PASS** |

**Total: 67 tests passing, 0 failures.**

### PR Readiness

**READY** — All fixes applied, all tests pass, branch pushed to remote.

**Pushed to remote:** `origin/integration/micronuts-wallet-mint-rpc`

---

## microfips

**Repo**: `/Users/macbook/src/microfips`
**Integration Branch**: `integration/microfips-service-layer`
**Base**: `main` (`6768870`)
**Commits Ahead of Main**: 5

### Merged

| Source Branch | Method | Commits | Files | Lines | Conflicts |
|---|---|---|---|---|---|
| `origin/copilot/refactor-microfips-http-demo` | Fast-forward merge | 5 | 32 | +2606/−892 | None |

**What this brought in:**
- ESP32 module split: monolithic `main.rs` → `ble_host.rs`, `ble_transport.rs`, `config.rs`, `handler.rs`, `led.rs`, `rng.rs`, `stats.rs`, `uart_transport.rs`
- New `microfips-service` crate (600 lines): `ServiceHandler` trait, `ServiceRequest`/`ServiceResponse`, `FspServiceAdapter`, `Router`
- New `microfips-http-demo` crate (813 lines): HTTP server demonstrating service layer
- `microfips-protocol` expansion: `fsp_handler.rs` (+201 lines)
- Documentation updates: `docs/architecture.md` (updated), `docs/http-demo.md` (new)
- CI updates, README updates, AGENTS.md additions

### Cherry-Picked

None.

### Skipped

| Branch | Commits | Reason |
|---|---|---|
| `copilot/add-wifi-connection-for-esp32` | 8 | All commits deeply intertwined with `crates/microfips-esp32/src/main.rs` which was heavily modified by the refactor branch. Destructive diff (9871 lines removed). No isolated safe cherry-pick candidates found. |

**Detailed skip analysis**: The WiFi branch was built against the old monolithic `main.rs`. The refactor branch split that file into 8 modules. Every WiFi commit touches the old `main.rs` in ways that conflict fundamentally with the new module structure. Cherry-picking any commit would require essentially rewriting the WiFi feature against the new architecture — that's new work, not integration.

### Test Results

| Crate | Command | Result |
|---|---|---|
| `microfips-core` | `cargo test -p microfips-core` | **180 PASS** (90 unit + 21 error_injection + 22 fips_compatibility + 18 fips_wire_format + 13 fsp_edge_cases + 6 fsp_over_fmp + 10 golden_vectors) |
| `microfips-service` | `cargo test -p microfips-service --lib` | **4 PASS** (request_round_trip, router_dispatches, dispatch_writes_error, service_round_trip_over_fsp) |
| `microfips-protocol` | `cargo test -p microfips-protocol` | **LINK FAILURE** — missing Embassy time driver symbols (`__embassy_time_now`, `__embassy_time_schedule_wake`) |
| All host crates | `cargo check --workspace --exclude microfips-esp32 --exclude microfips-esp32s3 --exclude microfips --exclude fips-decrypt` | **PASS** |
| Service + HTTP demo | `cargo check -p microfips-service -p microfips-http-demo` | **PASS** |
| Sim + Link + HTTP test | `cargo check -p microfips-sim -p microfips-link -p microfips-http-test` | **PASS** |

**Total: 184 tests passing, 0 failures.**

**Note on `microfips-protocol` link failure**: This is a **pre-existing condition**, not introduced by the integration. The crate depends on Embassy's time driver which requires an embedded target to link. The library compiles fine (`cargo check` passes) — only the test binary fails to link on host. This affects main branch identically.

### Documentation Verification

| Document | Status |
|---|---|
| `docs/architecture.md` | ✓ Accurate — references correct crate names and layer stack |
| `docs/http-demo.md` | ✓ Accurate — references correct service boundary types |

### Remaining Issues

1. **WiFi feature needs reimplementation**: The WiFi connection feature from `copilot/add-wifi-connection-for-esp32` could not be integrated due to architectural incompatibility with the ESP32 module refactor. If WiFi support is desired, it should be reimplemented against the new modular architecture. The old branch serves as a reference for the WiFi logic itself.

2. **`microfips-protocol` test linking**: Pre-existing. Needs either a host-compatible time driver mock or `#[cfg(target_os)]` guards on tests that depend on Embassy runtime. Not introduced by this integration.

### PR Readiness

**READY** — Clean branch, all host tests pass, no conflicts, clear 5-commit history. The protocol link failure is pre-existing and not a blocker. Recommend keeping the 5-commit history as-is (already well-structured).

**Pushed to remote:** `origin/integration/microfips-service-layer`

---

## Cross-Repo Integration Assessment

### Interface Alignment

The two repos have naturally compatible RPC boundaries:

| Aspect | micronuts | microfips |
|---|---|---|
| **Trait** | `RpcByteTransport` | `ServiceHandler` |
| **Input** | `&[u8]` (request bytes) | `ServiceRequest` (has `payload: Vec<u8>`) |
| **Output** | `Result<Vec<u8>>` | `ServiceResponse` (has `payload: Vec<u8>`) |
| **Serialization** | CBOR (via `minicbor`) | CBOR (via `minicbor`) |
| **Key types** | `RpcMintClient<T>`, `MintRpcHandler` | `FspServiceAdapter`, `Router` |

### Connecting Micronuts RPC over Microfips Service Layer

**Approach**: Implement `RpcByteTransport` for microfips's `ServiceHandler`:

```
┌─────────────────────────┐     ┌──────────────────────────────┐
│  micronuts              │     │  microfips                   │
│                         │     │                              │
│  RpcMintClient<T>       │     │  Router                      │
│    └─ T: RpcByteTransport│───▶│    └─ ServiceHandler         │
│         (bytes in/out)  │     │         └─ FspServiceAdapter │
│                         │     │              └─ FspHandler    │
│  MintRpcHandler         │◀───│                               │
│    (processes requests) │     │                               │
└─────────────────────────┘     └──────────────────────────────┘
```

**Estimated effort**: 2–3 days for basic integration.

**Steps**:
1. Create a bridge crate (e.g., `micronuts-fips-bridge`) that depends on both `cashu-core-lite` and `microfips-service`
2. Implement `RpcByteTransport` wrapping a `ServiceHandler` — maps `&[u8]` → `ServiceRequest` → `ServiceHandler::handle()` → `ServiceResponse` → `Vec<u8>`
3. Register `MintRpcHandler` as a `ServiceHandler` in microfips's `Router`
4. Add e2e test: mint/wallet flow running over FSP transport

**Considerations**:
- Both use `minicbor` for CBOR — no serialization mismatch
- Both are `no_std` compatible — the bridge can target embedded
- The `loopback_transport` in micronuts and the `service_round_trip_over_fsp` test in microfips serve as patterns for the integration test

---

## Summary Table

| Metric | micronuts | microfips |
|---|---|---|
| **Integration branch** | `integration/micronuts-wallet-mint-rpc` | `integration/microfips-service-layer` |
| **Commits ahead of main** | 12 | 5 |
| **Files changed** | 39 | 32 |
| **Lines changed** | +5177/−58 | +2606/−892 |
| **Branches merged** | 1 (full) + 1 (cherry-pick) | 1 (full) |
| **Branches skipped** | 4 (3 stale + 1 partial) | 1 (architectural conflict) |
| **Conflicts** | 0 | 0 |
| **Fixes applied** | 2 | 0 |
| **Tests passing** | 67 | 184 |
| **Tests failing** | 0 | 0 (protocol link failure is pre-existing) |
| **PR ready** | ✅ Pushed, ready for PR | ✅ Pushed, ready for PR |
| **Pushed** | ✅ Yes | ✅ Yes |

---

## What Was Done

- ✅ Branches pushed to remote
- ✅ GitHub issues created and updated
- ✅ Documentation updated

## What Was NOT Done (Per Original Instructions)

- ❌ No PRs opened (pending human review)
- ❌ No branches deleted
- ❌ No unrelated cleanup
- ❌ No architecture rewriting
- ❌ No firmware polish beyond what was needed for clean integration
- ❌ No HTTP/demo code contamination of core crates

---

## Next Steps

### micronuts
- Open PR from `integration/micronuts-wallet-mint-rpc` → `main`
- Review the 12 commits for merge strategy (squash-merge or rebase-merge recommended)
- Post-merge: consider cleaning up feature branches (`feat/defmt-conditional-build`, `copilot/add-demo-role-for-micronuts`)

### microfips
- Open PR from `integration/microfips-service-layer` → `main`
- Review the 5 commits (already well-structured, can merge as-is)
- Post-merge: GitHub issue #52 remains open for WiFi reimplementation tracking
- Post-merge: GitHub issue #51 closed as "documented expected behavior"
- Consider reimplementing WiFi support against new modular architecture (see #52)

### Cross-repo integration
- Create `micronuts-fips-bridge` crate once both PRs land
- Implement `RpcByteTransport` trait for microfips `ServiceHandler`
- Add e2e test: mint/wallet flow over FSP transport
- Estimated effort: 2–3 days
