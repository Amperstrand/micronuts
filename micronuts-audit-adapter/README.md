# micronuts-audit-adapter

HTTP-to-CBOR-RPC bridge that exposes the Micronuts demo mint (`micronuts-mint`)
over the Cashu REST surface (NUT-01 through NUT-09). It spawns `mint_server`
as a subprocess and translates each HTTP request into a CBOR `MintRpcRequest`
frame carried over the subprocess's stdio (one hex line per frame), then maps
the returned `MintRpcResponse` back to JSON that matches the Cashu HTTP
field-name spec (`B_`, `C_`, `C`, `Ys`, `Y`, `dleq`, …).

The bridge exists so external audit/CI tooling (and future wallet clients) can
poke the demo mint over plain HTTP without linking the mint library or speaking
CBOR themselves. The mint stays a stdio-only process; the adapter owns the wire
translation.

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

All endpoints return JSON. Successful bodies match the Cashu HTTP spec field
names. Error bodies use the NUT-00 `ErrorResponse` shape:
`{"detail": "...", "code": "<CODE>", "error_kind": "<CODE>"}`.

### `GET /v1/info` — NUT-06

```bash
curl -s http://127.0.0.1:3030/v1/info | jq
```

```json
{
  "name": "Micronuts Demo Mint",
  "pubkey": "030dad…",
  "version": "micronuts-mint/0.1.0",
  "description": "In-memory demo Cashu mint for Micronuts development",
  "description_long": "",
  "motd": "",
  "contact": [],
  "nuts": { "supported": [0, 1, 2, 3, 4, 5, 6, 7, 9] }
}
```

### `GET /v1/keys` and `GET /v1/keys/{keyset_id}` — NUT-01

Returns active keyset(s). `keys` is a JSON object mapping the amount as a
**string** to a 33-byte compressed-pubkey hex string (per Cashu HTTP spec).

```json
{
  "keysets": [
    {
      "id": "0022e025867793d1",
      "unit": "sat",
      "keys": {
        "1":   "030dad…",
        "2":   "03991d…",
        "4":   "02a194…",
        "8":   "02308f…",
        "16":  "03838a…",
        "32":  "035c32…",
        "64":  "021e52…",
        "128": "030d20…"
      }
    }
  ]
}
```

`GET /v1/keys/{keyset_id}` filters to one keyset and returns **404
`KEYSET_NOT_FOUND`** when the id does not match an active keyset.

### `GET /v1/keysets` — NUT-02

```json
{
  "keysets": [
    {
      "id": "0022e025867793d1",
      "unit": "sat",
      "active": true,
      "input_fee_ppk": 0
    }
  ]
}
```

### `POST /v1/mint/quote/bolt11` — NUT-04

Request: `{"amount": <u64>, "unit": "sat"}`

```json
{
  "quote":   "0000000000000001",
  "request": "lnbcdemo100sat1micronuts",
  "paid":    true,
  "state":   "PAID",
  "expiry":  18446744073709551615
}
```

Demo shortcut: the quote auto-transitions to `PAID` (no real Lightning).

### `GET /v1/mint/quote/bolt11/{quote_id}` — NUT-04

Same body shape as the POST. Returns **404 `QUOTE_NOT_FOUND`** when the id is
unknown.

### `POST /v1/mint/bolt11` — NUT-04

Request:

```json
{
  "quote":   "0000000000000001",
  "outputs": [
    { "amount": 64, "id": "0022e025867793d1", "B_": "<66-hex compressed point>" },
    { "amount": 32, "id": "0022e025867793d1", "B_": "<66-hex compressed point>" },
    { "amount":  4, "id": "0022e025867793d1", "B_": "<66-hex compressed point>" }
  ]
}
```

Response (signatures include NUT-12 DLEQ proofs when the mint produces them):

```json
{
  "signatures": [
    {
      "amount": 64,
      "id":     "0022e025867793d1",
      "C_":     "<66-hex compressed point>",
      "dleq":   { "e": "<64-hex scalar>", "s": "<64-hex scalar>" }
    }
  ]
}
```

### `POST /v1/swap` — NUT-03

Request: `{"inputs": [<proof>...], "outputs": [<blinded message>...]}`

A proof is `{"amount": <u64>, "secret": "<hex>", "C": "<66-hex>", "id":
"<keyset>"}` (the optional `witness` field is accepted but ignored — the demo
mint's CBOR `Proof` does not carry it). Response body shape matches
`POST /v1/mint/bolt11`.

### `POST /v1/melt/quote/bolt11` — NUT-05

Request: `{"request": "<bolt11 invoice>", "unit": "sat"}`

```json
{
  "quote":       "0000000000000002",
  "amount":      60,
  "fee_reserve": 0,
  "paid":        false,
  "state":       "UNPAID",
  "expiry":      18446744073709551615
}
```

Demo shortcut: the invoice amount is parsed from the dummy
`lnbcdemo<N>sat1…` format; other invoice strings return **500
`PROTOCOL_ERROR`**.

### `GET /v1/melt/quote/bolt11/{quote_id}` — NUT-05

Same body shape as the POST. Returns **404 `QUOTE_NOT_FOUND`** for unknown ids.

### `POST /v1/melt/bolt11` — NUT-05

Request:

```json
{
  "quote":   "0000000000000002",
  "inputs":  [<proof>...],
  "outputs": [<blinded message>...]   // optional change outputs
}
```

Response:

```json
{
  "paid":             true,
  "state":            "PAID",
  "payment_preimage": "0000…",
  "change":           [<blind signature>...]   // null when no outputs were sent
}
```

### `POST /v1/checkstate` — NUT-07

Request: `{"Ys": ["<66-hex>", ...]}` (note the **capital `Ys`**).

```json
{
  "states": [
    { "Y": "<66-hex>", "state": "UNSPENT", "witness": null }
  ]
}
```

### `POST /v1/restore` — NUT-09

Request body accepts either form the spec allows:

- `{"outputs": ["<66-hex Y>", ...]}` — hex Y values
- `{"outputs": [<blinded message>...]}` — full objects; the adapter forwards
  each object's `B_` (or `Y`) field as the lookup key

Response (NUT-09 spec shape):

```json
{
  "outputs": [
    { "Y": "<66-hex>", "promises": [<blind signature>] }
  ]
}
```

Demo limitation: the mint's NUT-09 implementation is stateless, so `outputs`
is always `[]` regardless of input. See
[`cashu-core-lite/src/nuts/nut09.rs`](../cashu-core-lite/src/nuts/nut09.rs).

## Error mapping

The adapter translates `CashuError` variants returned by the mint into HTTP
status codes. All error responses share the NUT-00 `ErrorResponse` body shape:

```json
{ "detail": "<human-readable>", "code": "<CODE>", "error_kind": "<CODE>" }
```

| `CashuError` variant        | HTTP | `code`                  | Reason                                                |
|-----------------------------|------|-------------------------|-------------------------------------------------------|
| `QuoteNotFound`             | 404  | `QUOTE_NOT_FOUND`       | Client asked for a quote id that does not exist.      |
| `KeysetNotFound`            | 404  | `KEYSET_NOT_FOUND`      | `GET /v1/keys/{id}` for an unknown keyset id.         |
| `InvalidAmount`             | 400  | `INVALID_AMOUNT`        | `amount <= 0` or overflow.                            |
| `InvalidProof`              | 400  | `INVALID_PROOF`         | Proof secret failed hex decode / malformed.           |
| `AmountMismatch`            | 400  | `AMOUNT_MISMATCH`       | Swap/mint inputs and outputs don't balance (NUT-03).  |
| `InsufficientInputs`        | 400  | `INSUFFICIENT_INPUTS`   | Melt inputs total less than `amount + fee_reserve`.   |
| `QuoteNotPaid`              | 400  | `QUOTE_NOT_PAID`        | Mint attempted on a quote that is not `PAID`.         |
| `QuoteAlreadyIssued`        | 400  | `QUOTE_ALREADY_ISSUED`  | Mint attempted twice on the same quote.               |
| `Protocol(_)`               | 500  | `PROTOCOL_ERROR`        | Service-side protocol/framing failure.                |
| `Crypto(_)`                 | 500  | `CRYPTO_ERROR`          | Signature verification / DLEQ failure inside the mint.|
| `Transport(_)`              | 500  | `TRANSPORT_ERROR`       | Internal transport failure (should not normally fire).|
| `Unknown(_)`                | 500  | `UNKNOWN`               | Anything not covered above.                           |

Two additional adapter-level responses (not from `CashuError`):

| Condition                                   | HTTP | `code`                  |
|---------------------------------------------|------|-------------------------|
| Subprocess dead / framing broken            | 503  | `MINT_UNAVAILABLE`      |
| Malformed request JSON or missing field     | 400  | `BAD_REQUEST`           |
| RPC returned an unexpected variant          | 500  | `UNEXPECTED_RPC_RESULT` |

### Rationale

- **404** for missing id lookups (REST idiom: the named resource does not exist).
- **400** for all client-side input failures (the request was structurally
  valid but semantically rejected by the mint). These are retryable by the
  client with corrected inputs.
- **500** for service-side failures. A signature-verification failure maps to
  `CRYPTO_ERROR` / 500 rather than 400 because the client cannot tell from
  the response alone whether the proof was malformed or the mint's keyset
  changed; the safe default is to surface it as a server problem.
- **503** only when the mint subprocess itself is unavailable — distinct
  from any business-logic error the mint may return.

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

### JSON ↔ CBOR field-name translation

The Cashu HTTP spec uses field names that differ from the CBOR field names
used internally (`B_` vs `b`, `C_` vs `c`, capital `C` for proofs, `Ys`/`Y`
for checkstate). The adapter performs this translation in both directions:

| HTTP JSON field | CBOR type                          | CBOR field |
|-----------------|------------------------------------|------------|
| `B_`            | `nut00::BlindedMessage`            | `b`        |
| `C_`            | `nut00::BlindSignature`            | `c`        |
| `C`             | `nut00::Proof`                     | `c`        |
| `Ys`            | `nut07::CheckStateRequest`         | `ys`       |
| `Y`             | `nut07::ProofState`                | `y`        |
| `Y` / `B_`      | `nut09::RestoreRequest` (accepted) | `outputs`  |
| `Y`             | `nut09::RestoreOutput`             | `y`        |
| `promises`      | `nut09::RestoreOutput`             | `signature`|
| `keys` (object) | `nut01::KeySet` (array of pairs)   | `keys`     |
| `dleq.e`/`dleq.s` | `nut12::BlindSignatureDleq`      | `e`, `s`   |

Point values (`B_`, `C_`, `C`, `Y`) are always 33-byte compressed secp256k1
points hex-encoded to 66 lowercase characters.
