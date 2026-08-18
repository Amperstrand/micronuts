# WalletPort Pattern Port & MCU Edge-Validator — Design

**Date:** 2026-08-18 · **Status:** design complete, implementation not started
**Source pattern:** tmbg #299 `src/tollwallet/port.go` (`WalletPort` interface)
**Target:** cashu-core-lite + a firmware profile for a TollGate edge validator

## 1. Why

tmbg #299 proved the value of the pattern in Go: merchant/lightning code
depends on a library-agnostic interface, wallets (gonuts now, cdk behind a
tag) sit behind it, and review caught real funds-loss bugs at the adapter
seam (Melt-without-Confirm, ephemeral mnemonic, silent overpayment limits).
Porting the *pattern* (not the interface verbatim) to cashu-core-lite gives
the MCU wallet the same discipline — and unlocks the edge validator: a
gate that opens on **offline DLEQ verification** when the backend cannot
reach the mint.

## 2. Interface mapping (tmbg WalletPort → ccl)

tmbg's 14 methods, mapped to cashu-core-lite capabilities:

| tmbg method | ccl capability | edge-validator scope |
|---|---|---|
| `DecodeToken(V3/V4)` | `decode_token` (**V4 CBOR only** — see gap G1) | ✅ core path |
| `Receive(token)` | `PersistentWallet::mint_deterministic` / `restore` (online); **offline: DLEQ-verify only** (§4) | ✅ the money path |
| `GetBalance` / `GetBalanceByMint` / `GetAllMintBalances` | `PersistentWallet::balance` (single-mint → one wallet per mint, mirroring GonutsWallet's map) | ✅ |
| `Send(amount, mint, includeFees)` | `PersistentWallet::spend` (largest-first; overshoot possible — caller swaps for change) | ✅ |
| `SendWithOverpayment(pct, abs)` | **refuse when pct/abs > 0** — same policy tmbg #299 adopted for CdkWallet: no cap enforcement exists, silent ignore is the bug | ✅ (by refusal) |
| `Drain(mint)` | `spend(balance)` | ✅ |
| `RequestMintQuote` / `GetMintQuoteState` / `MintTokens` | `request_mint_quote` / `check_mint_quote` / `mint_deterministic` over a `MintClient` transport | ⛔ online-only profile |
| `MeltToLightning` / `RequestMeltQuote` / `Melt` | transport methods exist; **no Lightning on MCU** | ⛔ return `Unsupported` |
| `Shutdown()` | `Drop` + final `persist` | ✅ |

## 3. Trust model port (from #299 review)

`mintAccepted(mint)` — acceptedMints list, `allowUntrusted` escape hatch,
no swap-to-trusted on the adapter — ports directly as a check in the facade.
Edge validator profile: **strictly accepted mints, `allowUntrusted = false`**,
keysets pinned at provision time (no NUT-01 fetch offline).

## 4. The edge-validator firmware profile

What a gate-open decision actually needs, all present in ccl today:

1. **Decode** the V4 token (`decode_token`).
2. **Verify offline**: `verify_signature` — the public-key DLEQ path —
   against the pinned keyset. This is the feature that makes the product:
   no mint contact required to know the mint signed these proofs.
3. **Value check**: token amount ≥ price per step (integer math).
4. **Replay protection**: local spent-secret ring (bounded, `ProofStore`-
   backed). Honest limitation: offline mode cannot know a token was spent
   at another gate — window bounded by the ring + sync-on-online
   (`post_check_state`, NUT-07) reconciliation.
5. **Persist** the spent secret *before* opening the gate (same
   persist-before-effect ordering as `spend`).

Transports for the online profile: microfips USB CDC / BLE GATT /
L2CAP → implement `MintClient` over `microfips-service` request/response.

## 5. Gaps surfaced by this mapping

- **G1 — V3 tokens**: ccl is CBOR-only by design (AGENTS.md). tmbg accepts
  V3+V4. Edge validator on V3 tokens either needs a small V3-JSON decoder
  (no_std serde_json cost) or an explicit "V4-only gate" policy decision.
- **G2 — NUT-10/11 locked tokens**: tmbg *rejects* locked tokens at
  receive. Parity = reject-by-default on unknown secret formats; ccl must
  add the same check (see study gap table).
- **G3 — swap for change**: offline spend overshoots without a NUT-03
  swap; acceptable for gate-open (exact pricing) but not for refunds.

## 6. Implementation plan

1. `micronuts-walletport` facade crate (workspace): tmbg-shaped trait +
   `CclWallet` adapter + trust check + refusal policies. Pure ccl, no
  firmware deps. (~2 days incl. tests reusing cross-vectors)
2. Offline verify path: decode + DLEQ-verify + spent-ring against
   `MemoryStore`; unit-test with `cashu-cross-vectors.json` artifacts. (~1 day)
3. Firmware bring-up on STM32F469I (micronuts board): QR → token →
   verify → GPIO gate. (~3-4 days, hardware-dependent)

Total ≈ 1 week per the study estimate. The cross-vector and
persistent-wallet test suites are the acceptance harness — no new
crypto, only wiring.
