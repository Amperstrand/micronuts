# ESP32 TollGate Client — design plan (2026-09-26)

Owner pivot: the live TollGate lane is the Wi-Fi DUT (AP3915i rig);
tollgate-on-S3 is deprioritized (tollgate-s3-rs issues #2: display
framebuffer alloc; option-114 bind). Instead: **a paying TollGate client
on ESP32** — a headless device that joins a captive-portal network and
pays its way through with on-device ecash. Micronuts already has the
primitives (`cashu-core-lite`, `micronuts-wallet-core`) and the chassis
(`micronuts-esp32-wallet`: WiFi station, HTTP transport, console,
hardware-verified receive flows).

## Shape: a feature of `micronuts-esp32-wallet`, not a new crate

Sibling crates here are per-device, not per-feature. The client is a new
`src/tollgate.rs` module + console commands in the existing loop,
reusing `wifi.rs` (station join), the wallet `engine` (spend), and the
esp-idf-svc HTTP stack already pinned house-style.

## Protocol facts (bench-verified on the rig DUT — carry these verbatim)

1. **Token submission is a RAW BODY POST** to the TollGate backend on
   tcp/2121 with `Content-Type: text/plain`. A JSON wrapper
   (`{"token": ...}`) breaks backend prefix parsing — this is the #1
   integration gotcha, documented across the physical-router-test-
   automation corpus.
2. **Pre-auth network reality under nodogsplash**: DNS passes tcp/53
   only; UDP DNS from a gated client fails. Probes must be IP-literal.
   The backend port 2121 is reachable from WiFi clients by design
   (users_to_router allow) — so discovery + payment need no portal HTML
   at all.
3. **Discovery**: take the gateway from the DHCP lease, `GET /` on
   :2121 — the backend identifies itself and its config (pricing) there.
   No captive-portal detection dance required for a headless client.
4. **Idempotence**: an already-authenticated MAC stays authed (NDS
   session). The client probes internet FIRST and only pays when
   gated.
5. **Post-payment verification**: IP-literal HTTP fetch (e.g. the
   anycast resolver that answers 301 on :80) — do not rely on DNS
   resolution until authed, and never rely on a hostname that the
   gated path would block.
6. Router-side quirks (NDS wedge after backend restart, auth-mark
   repair) are NOT client concerns; the client just retries once with
   a small backoff.

## State machine (`tollgate.rs`)

```
join <ssid>           -> WifiSta (open network: empty passphrase)
discover              -> DHCP gw IP, GET http://<gw>:2121/  (identify + price)
already-authed?       -> IP-literal probe; if open, done (no spend)
spend                 -> engine.send_token(amount, None)   // exact token string
submit                -> POST http://<gw>:2121/ raw body, text/plain
verify                -> IP-literal HTTP probe; assert 2xx-redirect path
report                -> console line + LED state (reuse board LED)
```

Console commands: `tollgate join <ssid>` · `tollgate pay [sats]` ·
`tollgate status` (engine balance + last flow state).

Token funding: the wallet already receives tokens on hardware
(`receive_token`, P2PK/HTLC flows verified). The client therefore
composes: fund the wallet (existing receive path) → `tollgate pay`
(spends from balance). No on-device minting needed for v1.

## Verification venue

The bench rig DUT (see conwrt-bench registry; symbolic here — no bench
identifiers in a public repo) runs a live TollGate with a fakewallet
mint and premined tokens: a full HIL is join → pay → internet-unlocked,
asserted over the serial console. Extend `tools/hil` with a
`tollgate-client` lane when the firmware flow is green.

## Implementation order

1. `tollgate.rs` skeleton + console cmds (join/status; no radio logic
   beyond existing wifi module) — builds with the `esp` toolchain.
2. Discovery + raw-body POST against the bench DUT (the two protocol
   facts above are the acceptance criteria).
3. Spend integration (`send_token` → POST → verify) end-to-end.
4. Bench HIL lane + (optional) Xtensa cross-build job mirroring the
   esp32-mint CI guard.
