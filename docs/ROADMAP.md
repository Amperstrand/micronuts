# Micronuts Roadmap — 2026-09-03

Where the work goes next, in priority order, with ownership so parallel
sessions don't collide. Issues carry the acceptance criteria; this file
carries the sequencing rationale. Update both together.

**Current state (all CI-green, 8/8 jobs incl. first-ever Xtensa link):**
host mint prototype with durable state (mint + reserve), upstream
settlement verified end-to-end against testnut (fake) and signut (real
CLN signet, two-cycle restart proof), conformance 104/107 (2026-09-07
matrix: 69 → 88 with #51, 88 → 97 with the melt sign-what-fits parity,
97 → 104 with NUT-20 quote locking + NUT-29 batch + NUT-19 cache).

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
melt expiry/HTLC scenarios and the `amounts_swapped` tamper that later
failed on the KNOWN runner issue (`_change_outputs` over-asking change)
all pass since the melt sign-what-fits parity (c814e5d) — melt amount
validation was NOT loosened. The remaining NUT-20/29/19 crumbs landed
2026-09-07 (see section 4); the matrix now reads 104/107 with the 3
skips being runner-side environment skips.

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
- #44 dependency audit cadence

Conformance crumbs CLOSED 2026-09-07: NUT-20 quote locking (pubkey echo,
BIP-340 gate on `post_mint`), NUT-29 batch endpoints (quote check +
batch mint with 50-quote/1000-output limits), and NUT-19 cache
advertisement (backed by a real HTTP-edge response cache in the adapter)
— matrix 97 → 104/107 (0 failed, 3 runner-side skips).

## Done anchor points

2026-09-02: safety rework + upstream backend + esp32 CI (#49, #50, #52,
#53, #59; audit docs/AUDIT-2026-09-02-mint-prototype.md for the arc).
