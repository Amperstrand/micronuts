# Micronuts Wallet — UX contract

The product contract for the wallet surfaces (host, wasm, and the F469
device). Research basis: `docs/RESEARCH-2026-09-12-cashu-wallet-ux.md`,
CDK current code (zread 2026-09-12), cashubtc/wallet `docs/product/`
(PRODUCT.md, copy-guidance.md — MIT), bitcoin.design daily-spending.
This file is the source of truth for wording, states, and rules; screens
that disagree with it are wrong.

## Product identity

A mature Cashu wallet running on unusual hardware. The user discovers
the unusual parts (device, scanner, offline-ness, security boundaries) —
never the protocol internals. Brand register, adapted from
cashubtc/wallet to an appliance: **quiet · precise · appliance-native**.

Anti-references (must not look like): gamified crypto apps, neon-on-black
"hacker" aesthetic, hero-metric dashboards, card grids, faux-terminal
chrome. The boot splash may keep personality; the wallet communicates
competence.

## Vocabulary

| User-facing | Never (internal only) |
|---|---|
| ecash, sats, Lightning | proofs, blind signatures, outputs |
| mint | keyset (except advanced info) |
| Receive / Send / Pay / Top up | mint quote, melt quote, swap, token creation |
| Waiting for recipient | pending token, unclaimed send |
| Recover / Reclaim | rollback, undo_spend |
| Add a mint (trust it) | NUT-04/05/07/09/12/13, reconciliation |

Rail naming follows cashubtc/wallet history titles: "Ecash received",
"Lightning received", and outgoing counterparts.

## Core flows and transaction states

Flows are named by user intent; internal steps (health check, quote
fetch, issuance) run automatically where safe.

```
ReceiveEcash:  Input → Inspecting → Review → Receiving → Received
               (Inspecting auto on scan/paste; Review shows amount+mint+fee)
PayLightning:  Input → Resolving → Review → Paying → Paid
ReceiveLightning: Amount → Invoice → Waiting → Paid→Issuing → Received
SendEcash:     Amount → Review → Preparing → ReadyForTransfer(QR)
               → AwaitingClaim → Claimed | Reclaimable
```

Failure taxonomy on every flow: `recoverable` (retry offered, money
safe), `terminal` (already-spent, invalid token, checksum), `offline`
(mint unreachable — money safe, try later), `expired` (invoice),
`paid-but-not-issued` (Lightning settled, issuance pending — a
money-safety state, never a generic failure). `SendEcash.AwaitingClaim`
must not be reported as "sent" in history; claim detection/reclaim is
Milestone-4 domain work.

## Confirmation rules

Every spending operation passes an explicit Review: amount, fee (see
below), mint, payment type, and expiry where relevant. A scan resolves
an intent and populates Review — it never authorizes spending.
Single explicit confirm control per flow (Receive / Send / Pay), never
double-confirmations.

## Amounts and fees

- Format: thousands-separated integer + unit — `12,450 sats`
  (bitcoin.design units guidance). One shared formatter; no screen
  picks its own. Tabular figures where available; monospace only as a
  deliberate numeric style, never as aesthetic.
- Zero-fee wording (cashubtc/wallet copy-guidance, adopted verbatim):
  prospective review row → **"No fee"**; settled receipt with zero fee →
  **omit the row**; quote reserve → numeric **"Up to N sats"** under a
  "Max fee" label.
- Success language: terminal screen "Payment received"; history row
  factual kind-first ("Ecash received"); transient inline confirmations
  may vary.

## Errors

State what happened, whether money is safe, and the next action
("Try again" / "Show QR again" / "Reclaim"). Raw transport/protocol
errors never reach the normal UI. Color never carries meaning alone
(icon/label pairs). Red is reserved for errors and destructive actions;
confirmed incoming may be green; outgoing is neutral.

## Mint trust

Explicit, per cashu.me wording: mints hold the bitcoin backing your
ecash; only use mints you trust. Scanning or receiving a token from an
unknown mint offers add-and-trust as a distinct confirmed step — never
auto-trust.

## Recovery

Seed copy states exactly what it derives and restores (NUT-13
deterministic secrets; NUT-09 re-fetch) and its limits (spent-state and
tokens already handed over are not recoverable; mint availability
required). "Anyone with these words can spend your ecash."

## Hardware adaptation (480×800 touch appliance)

- Touch targets ≥ ~88 px on the device; primary actions in the lower
  half (thumb reach).
- QR-first for inputs; typing is never required for core flows
  (host-assisted mint add remains the fallback).
- Offline is a normal state, not an error: Home shows it calmly and
  keeps flows usable where possible.
- Scanner is a first-class input; camera/GM65 differences stay behind
  the scan seam.

## CDK relationship

CDK (current) is the behavioral reference: quote-polling is wallet
machinery, exact-amount sends pre-swap denominations, proof states
(Unspent/Reserved/Pending/Spent), quote state machine, automatic
transaction recording. Semantics are adopted **through
`cashu-core-lite`** (the project's embedded CDK-light) — never
re-implemented in Slint callbacks or `micronuts-wallet`. Known
deliberate divergence today: melt input selection is
largest-first-in-wallet + pre-swap to exact denomination (MCU/engine
simplicity; documented in the research doc §3). Every new divergence
gets a table row: CDK behavior / Micronuts behavior / reason / safety
consequences / tests.

## Intentional differences from mobile wallets

Single-active-mint (not aggregated balances), no keyboard-first flows,
no fiat framing, QR hand-over instead of share sheets, host-assisted
mint configuration on device. These are hardware-shaped choices, not
omissions to quietly fix.
