# Micronuts Roadmap — 2026-09-03

Where the work goes next, in priority order, with ownership so parallel
sessions don't collide. Issues carry the acceptance criteria; this file
carries the sequencing rationale. Update both together.

**Current state (all CI-green, 8/8 jobs incl. first-ever Xtensa link):**
host mint prototype with durable state (mint + reserve), upstream
settlement verified end-to-end against testnut (fake) and signut (real
CLN signet, two-cycle restart proof), conformance 88/107 (2026-09-07
matrix; 69 → 74 with #51 L0+L1, 74 → 88 with #51 L2+L3).

## 1. NUT-10/11/14 spending conditions — #51 (mint line) — ARC COMPLETE

The conformance long pole. Layered plan lives on the issue (L0 secret
model → L1 P2PK+sigflags → L2 locktime → L3 HTLC); **L0+L1 landed
2026-09-07** (NUT-10 Secret model + P2PK witness enforcement before
`claim_proofs`, differential tests vs cashu 0.18 in
`micronuts-mint/tests/p2pk_differential.rs`; 19 matrix scenarios fixed;
the 14 now-failing expiry/HTLC spends passed only vacuously before).
**L2+L3 landed 2026-09-07, closing the arc**: locktime refund pathways
(primary stays available post-expiry — refund is additional, boundary
`locktime < now` strict as upstream) and NUT-14 HTLC (receiver preimage
pathway, sender/refund pathway after expiry, both SIG_INPUTS and SIG_ALL),
all clock-gated through the injectable `MintClock` with frozen-clock
boundary tests; differential tests in `p2pk_differential.rs` +
`htlc_differential.rs`; 14 more matrix scenarios fixed (88/107). The 4
melt expiry/HTLC scenarios that still fail do so on the KNOWN runner
issue (`_change_outputs` over-asks change → our spec-strict
AmountMismatch is CORRECT; melt amount validation deliberately NOT
loosened — pending a separate leniency decision). Known non-#51 matrix
crumbs: melt AMOUNT_MISMATCH path (4, pre-existing), NUT-20/29/19 (~7),
and one runner no-op tamper (`p2pk_sigall_output_amounts_swapped_fail`
re-prefixes `0` onto a `02/03` hex key — a no-op; document upstream
before touching).

## 2. Security findings from FIPS gate-2 — #54, #55, #57, #56, #58 (hardware/FIPS line, parallel session's queue)

Filed 2026-09-02 by the gate-2 review (`39147f0`). They target the
WALLET/firmware/responder stack, not the host mint service, so they
don't block #51 — but #54 (wallet swap verifies nothing — forged blind
signatures accepted) and #55 (FrameDecoder OOB panic) are P1 and belong
before any hardware money handling. Keyset rotation (see 4) mitigates
#56's thin-air-minting class on the mint side.

## 3. Device bring-up — #60 + STATUS-AND-TEST-PLAN (embedded line)

NVS persistence phase 3 (design ready in PERSISTENCE-DESIGN.md; extract
the store trait when the second backend lands), then first flash-on-
hardware run of `micronuts-esp32-mint`. `#61` (CSRST boot-hang bound)
is a prerequisite for reliable field boots.

## 4. Hardening backlog (mint line, post-#51)

- Keyset rotation + multiple keysets (audit F10; defuses #56 properly)
- fee_reserve from a real backend (F9)
- Async-melt poller — re-resolving parked/ambiguous upstream melts
  (the documented follow-up in upstream.rs)
- Conformance crumbs: NUT-20 quote locking, NUT-29 batch, NUT-19 cache
  (~7 scenarios after #51 lands → matrix ≈ 102/109)
- #44 dependency audit cadence

## Done anchor points

2026-09-02: safety rework + upstream backend + esp32 CI (#49, #50, #52,
#53, #59; audit docs/AUDIT-2026-09-02-mint-prototype.md for the arc).
