# ADR: FIPS Connectivity for Micronuts

**Status:** Proposed (drafted 2026-09-02 from the microfips #113 close-out
session; amended 2026-09-04 — gate-2 W4 answered: v1 mandate + v2 landed)
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
2. **Security review of micronuts + the new path.** ✅ **done 2026-09-02 —
   PASS WITH FINDINGS** (structured review: 3 hunters + 2 PoC engineers;
   report on #46, findings #54–#58). Wiring precondition: #57 — the
   responder's peer-authorization contract — ✅ **decided 2026-09-04**
   (daemon-side closed-allowlist mandate **and** the peer-context API
   landed; see "Peer authorization" below; #57 closed).
   Real-money graduation blockers: #54 (wallet swap flow performs no
   signature verification — needs NUT-12 DLEQ + pinned keysets) and #56
   (demo-mint keys publicly derivable, PoC'd; this mint never custodies real
   value). The walletport offline gate validator (trust model, pinned
   keysets, refuse-by-default, persist-before-open) was the evaluation
   framework — note its pinning discipline is exactly the fix direction for
   #54. New trust boundaries reviewed: the plaintext UART hop (physical link
   — same class as today's USB; the sidecar additionally becomes a
   persistent cleartext transit holder, and must never log payload bytes),
   and the FIPS responder that terminates the envelope and proxies to the
   mint (sees mint RPC in the clear — mint-facing proxy trust level).

3. **Interface spike — ✅ verified 2026-09-02, no wire dependency on gate 1.**
   See `docs/FIPS-SERVICE-INTERFACE-SPIKE.md`: the envelopes are structurally
   identical (same `ServiceHandler` signature, same request/reply fields,
   1:1 status mapping); integration is a ~50-line mechanical adapter on the
   host responder side. One real gap to size before implementation: envelope
   payloads vs the 2048 B FMP frame cap on WiFi transports.

### Peer authorization (gate-2 W4, #57) — v1 decision: daemon-side closed allowlist

Possession of an FSP session is the only authorization the service layer can
see today (`FspAppHandler::on_fsp_message` gets no peer identity;
`CashuRpcServiceAdapter` gates method+route only), and the fips daemon's ACL
**defaults to allow** (`peers.allow`/`peers.deny`, TCP-wrappers ordering:
allow-match → allow, deny-match → deny, no-match → **allow**). Wired naively,
any LAN peer the daemon forwards for gets a full mint-RPC oracle (W3
thin-air minting, spent-set pollution, unbounded quote growth).

**v1 mandate (operational, zero code):** any daemon hosting a
value-carrying responder deploys with a *closed* ACL — `peers.allow`
containing only the sidecar's provisioned key, `peers.deny` containing
`ALL`. Verified against fips 0.5.0 `src/node/acl.rs`: allow-list match
overrides the deny, so this pins the responder to exactly one peer. The
default-open posture is acceptable only for the demo mint that never
custodies real value. This does not change the leaf-side
`FIPS_PEER_ALLOWLIST` (microfips `fips-identity`, fail-closed env parse) —
that one gates who *our* node answers and stays as-is.

**v2 (landed 2026-09-04, observation-only):** peer identity is now in the
service contract — `FspAppHandler::on_fsp_message(.., peer: PeerContext)`
and `ServiceHandler::on_peer` in microfips-service (microfips #198,
`2094da8`/`b20859b`), carrying `link_pubkey` (Noise-authenticated link
peer, x-only) and `src_addr` (routing-only FSP datagram source). Micronuts
consumes rev `b20859b` (`c829190`): `FipsCashuResponder` overrides
`on_peer` and forwards to `CashuRpcServiceAdapter::observe_peer`, which
records + logs the peer per request (`7c04913`; the bridge crate stays
microfips-dep-free — `PeerInfo` carries plain bytes). **Enforcement
posture is unchanged: policy stays daemon-side per the v1 mandate** — the
responder observes/logs (the #57 "at least log it" bar); per-peer
enforcement in the responder itself is now possible but deliberately not
written yet. Caveat: `link_pubkey` is advisory when OUR node initiates on
the XX wire until the learned-key comparison lands (microfips #203);
under IK and on the responder path it is verified.

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
