# ADR: FIPS Connectivity for Micronuts

**Status:** Proposed (drafted 2026-09-02 from the microfips #113 close-out session)
**References:** `docs/AMPERSTRAND-EMBEDDED-REVIEW.md` ("microfips Overlap"),
microfips `AGENTS.md` (M9 MCU-to-MCU FSP, `microfips-service`,
`microfips-http-demo`/`-http-test`), microfips issue #179 (FMP v1 + noise-xx
forwarding), `micronuts-fips-bridge` (Cashu RPC ↔ service envelope).

## Context

Micronuts is a USB-tethered Cashu wallet: all mint traffic flows
STM32 → USB CDC → `host-mint-tool`. The Amperstrand ecosystem now has a
mature FIPS leaf stack (microfips: Noise IK/XK, FMP, FSP, verified on
USB/BLE/L2CAP/WiFi/ESP-NOW) and micronuts already contains two integration
artifacts: `micronuts-fips-bridge` (310-line Cashu-RPC ↔ HTTP-shaped service
envelope, no fips deps yet) and `micronuts-esp32-bridge` (ESP32-D0WD
WiFi↔UART sidecar hardware, built and flashed). The July 2026 embedded review
flagged the bridge crate as "THE integration point — verify the
ServiceHandler interface matches."

## Decision

Add FIPS connectivity via the **ESP32 sidecar topology**, gated on three
prerequisites, in this order:

### Topology (chosen: sidecar, not co-resident)

```
STM32 wallet ──UART──▶ ESP32 (microfips-esp32 leaf, own identity)
                          │ FIPS mesh (Noise IK link + FSP session)
                          ▼
                     FIPS daemon ──▶ FIPS responder (HTTP-shaped service,
                                    microfips-http-test pattern) ──▶ mint
```

Wallet RPC rides `micronuts-fips-bridge`'s envelope over the UART link; the
ESP32 speaks it into an FSP session. This is microfips M9's data path
(STM32↔ESP32 FSP through a daemon), already hardware-verified end-to-end.

**Rejected: co-resident FIPS on the STM32** (microfips-protocol `Node` inside
the wallet firmware). It collides with the wallet's USB CDC command protocol
on the same port, doubles the protocol stack in a flash/RAM budget that
already carries BSP + display + k256, and co-locates a network stack with
ecash secrets on one MCU for no gain. Revisit only if the sidecar hardware
goes away.

### Gates (all three before any wiring)

1. **Consumer-surface stabilization (microfips #179).** FIPS `next`
   (0.4.0-dev) breaks the wire (Noise XX, FMP v1). Land #179 — FMP v1
   negotiation and `noise-xx` forwarded to the firmware crates — before
   micronuts consumes the stack, or the migration cost lands twice.
2. **Security review of micronuts + the new path.** A wallet gains a network
   adjacency. The `walletport` offline gate validator (trust model, pinned
   keysets, refuse-by-default, persist-before-open) is the evaluation
   framework. New trust boundaries to review explicitly: the plaintext UART
   hop (physical link — same class as today's USB), and the FIPS responder
   that terminates the envelope and proxies to the mint (it sees mint RPC in
   the clear — mint-facing proxy trust level).
3. **Interface spike — ✅ verified 2026-09-02, no wire dependency on gate 1.**
   See `docs/FIPS-SERVICE-INTERFACE-SPIKE.md`: the envelopes are structurally
   identical (same `ServiceHandler` signature, same request/reply fields,
   1:1 status mapping); integration is a ~50-line mechanical adapter on the
   host responder side. One real gap to size before implementation: envelope
   payloads vs the 2048 B FMP frame cap on WiFi transports.

### Identity separation (invariant, independent of topology)

The FIPS node identity belongs to the **device** (the ESP32 sidecar), never
to the wallet or its keys. Wallet secrets (proofs, blinding factors) and
node identity must not share derivation, storage, or code paths. microfips's
deterministic pattern keys are a lab convention for the sidecar, not a
deployment answer — provisioned per-device keys when this leaves the bench.

## Consequences

- Wallet firmware changes stay minimal: one more `MicronutsHardware`
  transport (UART to sidecar) speaking the existing envelope; no FIPS code
  on the STM32.
- The ESP32 sidecar firmware becomes a composition of `microfips-esp32`
  (FIPS leaf) + a UART↔FSP relay — mostly existing parts.
- The FIPS responder (envelope terminator + mint proxy) is the new trusted
  component; `microfips-http-demo` is its embryo and needs hardening +
  its own review before touching anything beyond the demo mint.
- Rough effort after the gates: interface spike ≤1 session; sidecar firmware
  1–2 sessions; end-to-end demo (wallet → sidecar → daemon → responder →
  `micronuts-mint`) ~1 session.

## Open questions

- FSP session termination point: leaf↔responder per-session keys mean the
  daemon relays ciphertext — confirm the responder is the only plaintext
  hop (it is by construction today; re-verify when #179 lands).
- Does the sidecar UART protocol need framing robustness (reconnect,
  checksum) beyond the existing bridge's raw forwarding before it carries
  wallet RPC?
- microSD (unused today) as the wallet's persistence story — orthogonal,
  but it gates any "real money" graduation and should be settled before the
  security review so the review covers the final secret-storage design.
