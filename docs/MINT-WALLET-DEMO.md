# Micronuts Wallet ↔ Mint Demo

Minimal viable demo implementing Cashu NUT protocol types and a transport-neutral
wallet/mint architecture for the Micronuts hardware wallet project.

The current milestone moves the host-side demo across a **real serialized RPC
boundary**. The wallet no longer uses the mint via in-process function calls on
the main demo/test path; it now encodes CBOR RPC frames, exchanges bytes, and
decodes the mint response on the other side of a loopback byte transport.

## NUTs Implemented

| NUT | Name | Status | Notes |
|-----|------|--------|-------|
| NUT-00 | Notation, ID, Units | ✅ Full | BlindedMessage, BlindSignature, Proof types; amount decomposition; real secp256k1 crypto |
| NUT-01 | Mint Public Keys | ✅ Full | KeySet, KeysResponse; 8 denominations (1–128 sat) |
| NUT-02 | Keysets and IDs | ✅ Full | KeysetInfo; proper SHA-256 keyset ID derivation per spec |
| NUT-03 | Swap | ✅ Full | Input proof verification, amount balance check, re-signing |
| NUT-04 | Mint Quote + Mint | ✅ Demo | Auto-paid quotes (no real Lightning); real blind signing |
| NUT-05 | Melt Quote + Melt | ✅ Demo | Auto-paid melt (no real Lightning); dummy preimage |
| NUT-06 | Mint Info | ✅ Full | Name, version, supported NUTs |
| NUT-07 | Check State | ✅ Stub | In-memory spent set only; not durable across restarts |

## What Is Mocked

| Component | Real or Mocked | Detail |
|-----------|---------------|--------|
| Wallet-side crypto | **Real** | hash_to_curve, blind, unblind — all real secp256k1 |
| Mint-side signing | **Real** | `C_ = k * B_` using real k256 scalar multiply |
| Proof verification | **Real** | `k * hash_to_curve(secret) == C` verified on swap/melt |
| Lightning invoices | **Mocked** | Dummy strings like `lnbcdemo100sat1micronuts` |
| Payment preimages | **Mocked** | 32 zero bytes hex-encoded |
| Quote state machine | **Simplified** | Mint quotes auto-transition to PAID immediately |
| Spent-proof tracking | **In-memory** | Lost on restart; no double-spend prevention across sessions |
| Persistence | **None** | All state in RAM; no flash, no database |
| Fee calculation | **Hardcoded 0** | `input_fee_ppk = 0` |
| Key derivation | **Deterministic** | `SHA256(seed \|\| "cashu-key" \|\| index)` — not BIP-32/NUT-13 |

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    cashu-core-lite                    │
│  ┌──────────┐  ┌──────────┐  ┌───────────────────┐  │
│  │ crypto.rs│  │keypair.rs│  │  nuts/ (NUT types) │  │
│  │ blind    │  │PublicKey │  │  nut00..nut07      │  │
│  │ unblind  │  │SecretKey │  │  protocol structs  │  │
│  │ sign     │  └──────────┘  └───────────────────┘  │
│  │ verify   │                                        │
│  └──────────┘  ┌──────────┐  ┌───────────────────┐  │
│                │ error.rs │  │  transport.rs      │  │
│                │CashuError│  │  MintClient trait  │  │
│                └──────────┘  └───────────────────┘  │
│                ┌──────────┐  ┌───────────────────┐  │
│                │  rpc.rs  │  │ RpcMintClient<T>  │  │
│                │ CBOR RPC │  │ MintRpcHandler    │  │
│                │ envelopes│  │ RpcByteTransport  │  │
│                └──────────┘  └───────────────────┘  │
│                              ┌───────────────────┐  │
│                              │  wallet.rs        │  │
│                              │  Wallet<T>        │  │
│                              │  mint/swap/melt   │  │
│                              └───────────────────┘  │
└─────────────────────────────────────────────────────┘
                         │
                    uses types from
                         │
┌─────────────────────────────────────────────────────┐
│                   micronuts-mint                     │
│  ┌──────────────┐  ┌──────────────────────────────┐  │
│  │  keyset.rs   │  │  mint_core.rs (DemoMint)     │  │
│  │  DemoKeyset  │  │  get_info, get_keys,         │  │
│  │  8 denoms    │  │  post_mint_quote, post_mint, │  │
│  │  ID derivation│  │  post_swap, post_melt, ...  │  │
│  └──────────────┘  └──────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────┐│
│  │ rpc_service.rs                                   ││
│  │ impl MintService for DemoMint                    ││
│  └──────────────────────────────────────────────────┘│
│  ┌──────────────────────────────────────────────────┐│
│  │ loopback_transport.rs                            ││
│  │ encoded request bytes → handler → response bytes ││
│  └──────────────────────────────────────────────────┘│
│  ┌──────────────────────────────────────────────────┐│
│  │ direct_transport.rs (reference only)             ││
│  │ old in-process shortcut retained for comparison  ││
│  └──────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────┘
```

### Transport Neutrality

The `MintClient` trait in `cashu-core-lite/src/transport.rs` defines how the wallet
communicates with a mint. Implementations can be swapped without changing wallet code:

- **Old `DirectTransport`**: calls `DemoMint` in-process — kept only as a reference
- **Current RPC path**: `RpcMintClient<LoopbackTransport<DemoMint>>`
- **Future serial adapter**: wraps these same CBOR frames inside serial framing
- **Future microfips adapter**: carries the same RPC frames over microfips/FIPS

## Direct Path vs RPC Path

### Old direct in-process path

```text
Wallet<T=DirectTransport>
  -> MintClient methods
  -> DemoMint methods directly
```

This was useful to validate wallet/mint logic, but it did not prove a real wire
boundary.

### New RPC loopback path

```text
Wallet<T=RpcMintClient<LoopbackTransport<DemoMint>>>
  -> MintClient methods
  -> encode MintRpcRequest with minicbor
  -> LoopbackTransport exchanges request bytes
  -> MintRpcHandler decodes bytes and dispatches by NUT operation
  -> DemoMint service methods run
  -> MintRpcHandler encodes MintRpcResponse
  -> RpcMintClient decodes response bytes
```

This is the required precursor to microfips integration because the protocol is
now expressed as a transport-neutral byte exchange instead of an in-process API.

## How to Build

### Wallet Role (library + demo)

```bash
# Build the wallet + mint demo
cargo build -p micronuts-mint

# Run the RPC loopback wallet/mint demo
cargo run -p micronuts-mint --bin demo
```

### Mint Server Role (library)

```bash
# Build just the mint library
cargo build -p micronuts-mint --lib
```

### Core Library

```bash
# Build cashu-core-lite with std features (for host)
cargo build -p cashu-core-lite --features std

# Build cashu-core-lite for no_std (for embedded)
cargo build -p cashu-core-lite
```

### Existing Firmware (unchanged)

```bash
# Build firmware for STM32F469I-Discovery
cargo build -p firmware --release
```

## How to Run the End-to-End Demo

```bash
# Run all tests (unit + integration + e2e)
cargo test -p micronuts-mint

# Run RPC envelope tests in cashu-core-lite
cargo test -p cashu-core-lite --features std --test rpc

# Run byte-level loopback transport tests
cargo test -p micronuts-mint --test rpc_loopback

# Run all wallet/mint checks used by CI
cargo test -p cashu-core-lite --features std -p micronuts-mint
cargo run -p micronuts-mint --bin demo
cargo check -p micronuts-app

# Run only e2e tests
cargo test -p micronuts-mint --test e2e

# Run the demo binary with full output
cargo run -p micronuts-mint --bin demo
```

## Upstream Cashu / CDK Reuse Strategy

- `cashu-core-lite` remains `no_std`, so we do **not** depend on `cdk` or `cashu`
  in production code.
- Instead, Micronuts follows upstream Cashu/CDK request/response shapes and
  mirrors selected helper semantics, especially amount splitting and quote-state
  strings.
- We use the upstream `cashu` crate in **std-only compatibility tests** to keep
  our local implementation aligned without pulling server-oriented dependencies
  into the embedded build.
- Those tests currently validate hash-to-curve, blind/sign/unblind flow,
  quote-state strings, greedy denomination splitting, and now RPC-envelope
  roundtrips against the local wire protocol.
- This gives us reuse and regression protection now, while preserving a clean
  path for future transport adapters and embedded targets.

## Module / Role Layout

### `cashu-core-lite`

- `src/transport.rs`
  - wallet-side `MintClient` trait
- `src/rpc.rs`
  - `MintRpcRequest`
  - `MintRpcResponse`
  - `MintRpcMethod`
  - `MintRpcResult`
  - `MintService`
  - `MintRpcHandler`
  - `RpcByteTransport`
  - `RpcMintClient<T>`

### `micronuts-mint`

- `src/mint_core.rs`
  - `DemoMint` core mint logic
- `src/rpc_service.rs`
  - `impl MintService for DemoMint`
- `src/loopback_transport.rs`
  - host-side byte loopback implementation
- `src/direct_transport.rs`
  - legacy direct path kept as reference only
- `src/bin/demo.rs`
  - wallet-side demo using RPC loopback by default

### Expected Demo Output

```
=== Micronuts Wallet ↔ Mint Demo ===

1. Mint info:
   Name: Micronuts Demo Mint
   Version: micronuts-mint/0.1.0
   Supported NUTs: [0, 1, 2, 3, 4, 5, 6, 7]

2. Active keyset: 0022e025867793d1 (unit: sat)
   Denominations: [1, 2, 4, 8, 16, 32, 64, 128]

3. Keysets: 0022e025867793d1 active, fee_ppk=0

4. Minting 100 sats...
   (auto-paid quote, 3 proofs minted)

5. Swapping 100 sats into [32, 32, 16, 8, 4, 4, 2, 1, 1]...
   (9 new proofs)

6. Melting 64 sats to pay invoice...
   (auto-paid, dummy preimage)

7. Remaining balance: 36 sats

8. Verified remaining proofs via swap

=== Demo complete ===
```

## What Remains for Real Embedded Minting

| Area | Current Demo | Production Target |
|------|-------------|-------------------|
| Key derivation | SHA-256 from fixed seed | BIP-32 from mnemonic (NUT-13) |
| Lightning | Dummy strings | Real LNbits/LND/CLN backend |
| Persistence | None (RAM only) | Flash storage for quotes, spent proofs |
| Double-spend prevention | In-memory set (session-only) | Durable spent-proof database |
| Fees | Hardcoded 0 | Configurable per-keyset fees |
| Multiple keysets | Single hardcoded | Multiple keysets with rotation |
| DLEQ proofs | Not implemented | NUT-12 for public-key verification |
| Restore | Not implemented | NUT-09 for wallet recovery |
| Transport | RPC loopback bytes | serial framing, USB CDC, microfips |

## Next Steps

### Replace Mocked Mint Crypto with Real Mint Crypto

1. The mint-side signing is already real (`k * B_` via k256) — no mocking here
2. Replace deterministic key derivation with BIP-32/NUT-13 mnemonic-based derivation
3. Add DLEQ proofs (NUT-12) for public-key-only verification
4. Add real spent-proof persistence (flash or external storage)

### Carry Request/Response Layer Over Microfips

1. Keep `MintRpcRequest` / `MintRpcResponse` as the payload format
2. Wrap the encoded CBOR bytes inside microfips frames
3. Implement `RpcByteTransport` over microfips send/receive
4. Reuse `RpcMintClient<T>` unchanged on the wallet side
5. Reuse `MintRpcHandler<DemoMint>` unchanged on the mint side

### What Can Replace Today’s Specialized Serial Commands

The eventual replacement for the current specialized serial command set should be:

1. a small serial framing layer
2. carrying `MintRpcRequest` / `MintRpcResponse` CBOR payloads
3. dispatched by `MintRpcHandler`
4. consumed by `RpcMintClient`

That lets serial, USB CDC, and microfips all share the same Cashu RPC payload
layer instead of each transport inventing its own request-specific command set.

### Add Real Lightning Backend

1. Replace `post_mint_quote` auto-approve with LNbits/LND webhook
2. Replace `post_melt` auto-pay with real invoice payment
3. Add payment verification and preimage extraction
4. Add quote expiry and polling
