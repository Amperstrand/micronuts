# FIPS Service Interface Spike — `micronuts-fips-bridge` ↔ `microfips-service`

**Date:** 2026-09-02 · **ADR gate 3** (`docs/FIPS-INTEGRATION-ADR.md`) ·
**Verdict: MATCH** — the two envelopes are structurally identical; integration
needs a mechanical type adapter, not a semantic translation.

## What was compared

- `micronuts-fips-bridge` (this repo): `ServiceRequest` / `ServiceReply` /
  `ServiceError` / `ServiceHandler`, `CashuRpcServiceAdapter<S: MintService>`
  (POST `/rpc/mint`), `ServiceHandlerTransport<H>: RpcByteTransport`.
- `microfips-service` (microfips): same-named types plus the pieces micronuts
  deliberately lacks — the **wire codec** (`encode_request` / `decode_request`
  / `encode_response`), a static `Router`, and `FspServiceAdapter<H:
  ServiceHandler>` (dispatches FSP session datagrams into the handler).

## Interface map (field-for-field)

| Concept | microfips-service | micronuts-fips-bridge | Compatible |
|---|---|---|---|
| Request | `ServiceRequest { method, route: &str, payload: &[u8] }` | identical | ✅ |
| Handler | `fn handle(&mut self, req, &mut [u8]) -> Result<ServiceReply, ServiceError>` | identical signature | ✅ |
| Reply | `{ status, content_type, body_len }` over caller buffer | identical | ✅ |
| Methods | `Get/Post/Put/Delete` (u8 wire repr 1–4) | same variants | ✅ |
| Content types | `Binary/Json/Text` (u8 wire repr 0–2) | same variants | ✅ |
| Status | numeric `u16` HTTP codes (200/201/400/404/405/413/500) | enum variants | ✅ 1:1 mapping |
| Errors | messageless enum (`NotFound`, `MethodNotAllowed`, …) | enum + `&'static str` message | ✅ adapter drops messages, keeps statuses |
| Wire format | 8-byte header: version, kind, method, reserved, route_len u16 LE, payload_len u16 LE | none (types only) | micronuts side never sees it — sidecar owns encoding |

## Integration composition

Host-side responder (std, depends on both crates), ~50 lines, no semantic
translation — field-by-field type conversion between the two trait families:

```
FSP datagram → microfips_service::decode_request
             → FipsCashuResponder (microfips ServiceHandler)
             → delegates to micronuts_fips_bridge::CashuRpcServiceAdapter<MintFront>
             → encode_response → FSP datagram back
```

The wallet side is unchanged: it already speaks request/response bytes over
its transport; the ESP32 sidecar (see ADR topology) encodes the microfips
envelope around them.

Existing proof points on each side: microfips `DemoService` implements
`ServiceHandler` end-to-end over FSP (`microfips-http-demo` +
`FspServiceAdapter`); micronuts `CashuRpcServiceAdapter` round-trips real
Cashu RPC (`rpc_roundtrip_works_over_service_handler_transport`).

## Gaps found (none block the ADR gates)

1. **Payload ceiling vs transport frames.** The envelope allows u16 payloads
   (64 KiB), but FMP frames cap at 2048 B on WiFi/UDP transports (768 B on
   the ESP32 L2CAP build). Cashu mint replies (`get_keys` with many keysets,
   multi-proof mints) can exceed 2048 B. Needs either FSP-datagram-level
   segmentation above the frame cap or size-capped/paged RPC at the Cashu
   layer — measure real mint responses before choosing.
2. **Error messages are lossy across the adapter** (microfips errors carry no
   message strings). Statuses survive; acceptable for v1, revisit if wallet
   UX needs mint error text.
3. **Version byte is unclaimed** in the micronuts envelope view — harmless
   today (sidecar owns the codec), but if micronuts ever encodes natively it
   must adopt microfips's `SERVICE_VERSION = 1` layout, not invent a second
   one.

## Next action when gates 1–2 clear

Implement `FipsCashuResponder` + host e2e: wallet → sidecar UART → ESP32 FIPS
leaf → daemon → responder → `micronuts-mint` (demo), asserting a
`get_info`/`get_keys` round trip over the full path. ~1 session.
