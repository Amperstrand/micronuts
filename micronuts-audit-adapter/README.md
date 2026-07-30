# micronuts-audit-adapter

HTTP-to-CBOR-RPC bridge that exposes the Micronuts demo mint (`micronuts-mint`)
over a small REST surface. It spawns `mint_server` as a subprocess and
translates each HTTP request into a CBOR `MintRpcRequest` frame carried over
the subprocess's stdio (one hex line per frame), then maps the returned
`MintRpcResponse` back to JSON.

The bridge exists so external audit/CI tooling (and future wallet clients) can
poke the demo mint over plain HTTP without linking the mint library or speaking
CBOR themselves. The mint stays a stdio-only process; the adapter owns the wire
translation.

## Scaffold scope

Only `GET /v1/info` (NUT-06) is wired. Additional endpoints (NUT-01 keys,
NUT-02 keysets, NUT-03 swap, NUT-04 mint, NUT-05 melt, NUT-07 checkstate) land
in follow-up tasks.

## Build

From the workspace root:

```bash
cargo build -p micronuts-audit-adapter
```

The adapter shells out to `target/debug/mint_server`, so build that first (or
point at a different path via `MICRONUTS_MINT_BIN`):

```bash
cargo build -p micronuts-mint --bin mint_server
```

## Run

```bash
cargo run -p micronuts-audit-adapter
```

Defaults:

| Setting | Default | Override |
|---------|---------|----------|
| Listen address | `127.0.0.1:3030` | `MICRONUTS_ADAPTER_PORT` |
| Mint subprocess | `target/debug/mint_server` | `MICRONUTS_MINT_BIN` |

Example:

```bash
MICRONUTS_ADAPTER_PORT=4000 cargo run -p micronuts-audit-adapter
```

## Endpoints

### `GET /v1/info`

Returns NUT-06 mint info as JSON.

```bash
curl -s http://127.0.0.1:3030/v1/info | jq
```

```json
{
  "name": "Micronuts Demo Mint",
  "pubkey": "...",
  "version": "micronuts-mint/0.1.0",
  "description": "Micronuts demo mint (no real Lightning)",
  "contact": [],
  "nuts": {
    "supported": [0, 1, 2, 3, 4, 5, 6]
  }
}
```

If the mint subprocess is unavailable or the framing breaks, the adapter
responds `503 Service Unavailable` with:

```json
{ "code": "MINT_UNAVAILABLE", "error": "<details>" }
```

## Architecture

```
HTTP client ──HTTP──▶ audit-adapter (axum, port 3030)
                         │
                         │ MintRpcRequest (CBOR) → hex → stdin line
                         ▼
                     mint_server subprocess (micronuts-mint)
                         │
                         │ hex line → CBOR → MintRpcResponse (stdout)
                         ▼
                    audit-adapter ──JSON──▶ HTTP client
```

All subprocess access is serialized through a single mutex because the
stdio framing protocol is line-oriented and stateful per request id. The
subprocess is spawned with `kill_on_drop(true)` so it never outlives the
adapter.

The framing protocol is documented in
[docs/MINT-WALLET-DEMO.md](../docs/MINT-WALLET-DEMO.md).
