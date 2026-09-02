# Upstream-mint settlement backend — design (P3)

**Goal**: let the micronuts mint settle "Lightning" through a real upstream
Cashu mint, exactly the model cashu-cf uses for its minibits-backed envs
(rugs02; ISSUE-053): our mint is the user-facing front; the upstream mint's
tokens are our reserve; upstream mint quotes are our invoice source.

**Targets**: testnut (FakeWallet, fake money, free rein) first; signut
(CLN signet, free rein) as the stretch. NEVER mainnet without owner approval
(cashu-cf AGENTS.md money taxonomy applies).

## Mapping to `LightningBackend`

| Trait method | UpstreamCashuBackend behavior |
|---|---|
| `create_invoice(amount, desc)` | POST upstream `/v1/mint/quote/bolt11` `{amount, unit:"sat"}`; store quote id; return upstream `request` (its bolt11) |
| `is_settled(invoice)` | GET upstream `/v1/mint/quote/bolt11/{id}` → state PAID (poll; FakeWallet settles on first poll) |
| `lookup_amount(invoice)` | Create (and cache by invoice string) an upstream melt quote for the bolt11 → its `amount`; also caches `fee_reserve` |
| `pay_invoice(invoice, amount)` | Melt reserve proofs against the cached upstream melt quote → preimage from upstream response |

## Reserve wallet

- Random 32-byte secrets (rand) per output — NOT NUT-13 deterministic
  (deterministic secrets across restarts would let a wiped store re-create
  already-spent proofs → double-spend against ourselves).
- Blinding via cashu-core-lite (`blind_message`), unblind change, keep proofs.
- In-memory first (host prototype); persistence milestone moves proofs to
  file/NVS — persist-before-use rule from cashu-cf (39-sat loss class).
- Bootstrapping: on first use, mint a configurable reserve (default 1000 sats)
  via an upstream quote; auto top-up when reserve < 2× max melt seen.
- **Era rule** (cashu-cf hard rule #2): never switch upstream URL while
  reserve tokens from the old era are outstanding.

## Fee policy (cashu-cf parity)

- Our melt `fee_reserve` = upstream fee_reserve + margin (never-red):
  `max(1 sat, 0.5%)` default, env-tunable (`CASHU_MELT_FEE_MARGIN_*` in cf).
- Our keyset `input_fee_ppk` independent (0 for prototype).
- Melt margin is mint income → dropped (demo) or ledgered (persistence
  milestone).

## Failure semantics (payment-safety checklist)

- `pay_invoice` failure → upstream melt quote state checked before declaring
  FAILED: PENDING upstream = ambiguous → we must NOT release local proofs
  (cashu-cf ISSUE-061 lesson: timed-out payment can settle later).
  Prototype rule: on upstream PENDING, return Err (melt stays PENDING locally,
  proofs stay claimed) — a poller (host task / device timer) resolves later.
- No stale-proof release while a quote is PENDING (stale-proof-recovery
  pattern).

## Configuration

```
MICRONUTS_UPSTREAM_MINT=https://testnut.cashu.exchange
MICRONUTS_UPSTREAM_UNIT=sat
MICRONUTS_RESERVE_BOOTSTRAP_SATS=1000
```

Feature-gated (`backend-upstream`, std-only — ureq/rustls on host; esp-idf
implements the same trait with its HTTP client later).

## Test ladder

1. Unit: backend against a local mock upstream (axum test server replaying
   testnut response shapes).
2. Live fake: full e2e_wallet.mjs against local mint with
   `MICRONUTS_UPSTREAM_MINT=https://testnut.cashu.exchange` — the wallet sees
   REAL testnut invoices; payment is faked by testnut's FakeWallet on poll.
3. Live signet (stretch): same against signut; real signet sats, near-zero
   value; tx-ledger discipline from cashu-cf applies (log fees/timings).

## Upstream backend — IMPLEMENTED 2026-09-02

Implemented behind the `backend-upstream` feature (host-only; ureq/rustls,
same pins walletport uses):

- `micronuts-mint/src/upstream.rs` — `UpstreamCashuBackend`
  (`LightningBackend` impl) + `upstream_backend_from_env()` + defensive
  JSON helpers (amounts accepted as numbers OR decimal strings).
- `micronuts-mint/src/reserve.rs` — `ReserveWallet`: bootstrap (mint quote →
  poll ≤ 10 × 500 ms → blind → `/v1/mint/bolt11` → unblind), pay (select
  covering 5 % + 1 sat buffer, melt, unblind change back), auto-top-up
  (deficit + bootstrap margin), in-memory proof store with balance logs.
- `mint_server` selects the backend from the environment at startup; if
  `MICRONUTS_UPSTREAM_MINT` is set but the binary lacks the feature it
  exits with a clear error. `demo`/`wallet_demo` stay FakeWallet.

Environment:

| Var | Default | Meaning |
|---|---|---|
| `MICRONUTS_UPSTREAM_MINT` | unset → FakeWallet | Upstream mint base URL |
| `MICRONUTS_UPSTREAM_UNIT` | `sat` | Upstream unit for quotes/keyset selection |
| `MICRONUTS_RESERVE_BOOTSTRAP_SATS` | `1000` | Initial reserve / top-up margin |

Tests: `micronuts-mint/tests/upstream.rs` — std-only TcpListener mock
upstream with real blind-signature crypto (settle-after-2-polls, transient
500, PENDING/FAILED melt modes, garbage-body mode) plus an `#[ignore]`d
`testnut_live_roundtrip` live check (network; run manually with
`-- --ignored`).

Ambiguity caveat (payment safety): an upstream melt response in a
non-terminal state returns `CashuError::Protocol("upstream melt state
ambiguous: …")` and PARKS the selected reserve proofs (they may still be
consumed upstream); a definitive upstream `FAILED` — and, as a prototype
simplification, an HTTP error on the melt POST — returns
`CashuError::PaymentFailed` with proofs retained. Under current mint_core
semantics any `Err` releases the local melt inputs, so a late-settling
upstream payment can strand reserve value: the PENDING-ambiguity poller
(cashu-cf ISSUE-061-style) is the documented follow-up.
