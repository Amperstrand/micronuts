# micronuts-satocash

Cashu token custody on a JavaCard: a host-side client for the
[SatoCash applet](https://github.com/Toporin/Satocash-Applet) (Toporin,
AGPL) — a Cashu ecash wallet that lives inside a smartcard secure element.

## What runs where (the bench chain)

```
micronuts (this crate)            ai-legion (bench host)
  SatocashClient ── PC/SC ──▶ pcscd ── libifdvpcd (listens :35963)
                                       ▲ TCP, board connects out
                                       ▼
                          nucula board (ESP32-C3, ccid-firmware-rs
                          wifi-vpcd image) ── PN7160 NFC ──▶ javacard
                                                                    │
                                              SatoCash v0.1 applet ──┘
                                              (AID 5361746F63617368)
```

- **nucula**: runs the `ccid-firmware-rs` wifi-vpcd firmware — a CCID
  smartcard reader. It holds no keys and no wallet; it is transport.
- **javacard**: GP 2.3 card carrying the SatoCash applet. It is the
  wallet: mint URLs (≤16), keysets (8-byte ID + mint + unit, ≤32) and
  ecash proofs (≤128 objects, 68 bytes each on-card: state, keyset,
  amount exponent, 33-byte unblinded public key, 32-byte secret).
  The unblinded key + secret are what a Cashu proof **is** — on-card
  storage puts them in tamper-resistant hardware.
- **micronuts**: the host brain — mint client, UI, and now this card
  client. SatoCash has no mint functionality; minting/melting stays with
  `micronuts-mint`/`micronuts-audit-adapter` on the host.

## Security model

Four commands run in plaintext: `GET_STATUS (0x3C)`,
`SATOCASH_GET_STATUS (0xB0)`, `INIT_SECURE_CHANNEL (0x81)`,
`PROCESS_SECURE_CHANNEL (0x82)`. Everything else — SETUP, PIN, and all
mint/keyset/proof storage — the applet rejects with `SW 9C20` until an
ECDH (secp256k1) + AES-128-CBC secure channel is established and each
inner APDU is encrypted inside `PROCESS_SECURE_CHANNEL`.

Implemented here: the APDU codec, status parsing, the wallet-storage
command encodings (inner payloads), and the PC/SC transport. The
secure-channel wrapper is the next milestone — until it lands, real-card
storage ops correctly surface `9C20`, and the wallet ladder is verified
against a scripted applet in unit tests.

## Status words

`9C04` setup not done · `9C01` no memory left · `9C06` unauthorized ·
`9C0F` invalid parameter · `9C20` secure channel required (see `sw` module).

## Using

```bash
cargo build -p micronuts-satocash --features pcsc --example probe
# where the reader lives (bench: ai-legion):
./probe "Nucula VPCD 00 00"   # exit 0 = applet answered, 2 = not installed
```

Library:

```rust
use micronuts_satocash::{SatocashClient, pcsc::PcscTransport};
let mut c = SatocashClient::new(PcscTransport::connect_by_name("Nucula")?);
let st = c.select()?;                    // applet + protocol version, capacities
let _ = c.get_status()?;                 // same, without re-SELECT
let _ = c.import_mint(b"https://…")?;    // 9C20 on real cards until the channel lands
```

## Tests

```bash
cargo test -p micronuts-satocash              # mock-applet unit ladder
cargo clippy -p micronuts-satocash --all-targets --all-features -- -D warnings
cargo fmt -p micronuts-satocash -- --check

# real-card e2e (host with the reader; bench: ai-legion)
SATOCASH_READER="Nucula VPCD 00 00" \
  cargo test -p micronuts-satocash --features pcsc --test real_card -- --nocapture
```

The real-card test asserts the bench chain end-to-end (SELECT → parsed
v0.1 status, SELECT/GET_STATUS agreement) and that a virgin card enforces
channel-gating (`9C20`). It never runs SETUP — the bench card stays
virgin for the channel milestone.

## Bench notes (ai-legion)

- Reader `Nucula VPCD 00 00/00 01` via pcscd + libifdvpcd; board OTA at
  `192.168.13.230:3232`; serial recovery via the Espressif USB-JTAG CDC.
- Card: GP 2.3, CPLC serial 50972320; GP keys = the committed bench
  triple (`ccid-firmware-rs/tests/soak/soak_02_globalplatform.py:32`).
- Install used: `gp --reader "Nucula VPCD 00 00" <bench triple>
  --install Satocash.cap --bs 200` (NCI transport caps APDUs at ~245 B).
- Failed SCP attempts burn the card's ISD retry counter — never blind-retry.

## Spec-drift auditing (greatspectations)

The encoders are pinned to the applet's own wire documentation:

- `SPEC-wire.md` — verbatim command blocks extracted from
  `Toporin/Satocash-Applet` master @ `ee413cc702f79e53f2238d5efce5cfc94a498a1e`
  by `scripts/extract_wire_spec.py`.
- `specquotes.toml` — greatspectations source `SATO`; each encoder
  carries `// SATO: <verbatim block>` comments; CI (`rust-ci.yml`,
  "Satocash wire-spec drift check") fails if a quote no longer matches
  the spec file. Upstream wire changes surface here first.

```bash
cd micronuts-satocash
python3 scripts/extract_wire_spec.py <Satocash.java> > SPEC-wire.md  # re-pin
greatspectate check --config specquotes.toml --comment-start "// " \
  --comment-continue "//" --comment-aside "// Note:" $(find src -name "*.rs")
```

Rules learned wiring this up (each cost a debugging round):

- Quotes must sit in plain `//` lines directly above the item — a
  `// SATO:` block followed by `///` doc lines silently merges the doc
  text into the quote and never matches.
- A quote is matched as a full normalized span (whitespace-collapsed,
  wrapped `return:` continuations included) — partial blocks fail.
- **Always negative-test a new specquotes config** (corrupt one quote,
  expect exit 1): our first wiring passed vacuously — green while
  checking nothing. That vacuous pass was the audit's first finding.

What greatspectations does NOT cover (and what does):

| Risk | Caught by |
|---|---|
| Upstream applet changes a wire format | `SATO` spec-quote drift gate (above) |
| Our encoder disagrees with the applet TODAY | mock ladder tests + real-card e2e (bench truth) |
| Plausible-but-wrong APDU framing (the 6-byte `Lc=0` form → `6700` + board wedge) | `ccid-pcsc-host::apdu` builder + its forbidden-form regression test + runbook lore (ccid-firmware-rs owns this) |
| Card/stack behavior no document describes | HIL capture: pin captured bytes as fixtures; re-capture on bench sessions |
| Whole-chain regressions without hardware | follow-up: jCardSim running the real applet behind vpcd (the same driver the nucula chain uses) — reference: OpenSC wiki "Smart Card Simulation", jcardsim.org `VSmartCard`; differential-oracle pattern per `jaesharp/simrs` |

## Roadmap

1. Secure channel: ECDH + AES-CBC wrapper (reference: pysatochip) →
   unlocks SETUP, PIN, and real wallet ops on hardware.
2. Proof ingest bridge: mint a token with `micronuts-wallet`, push its
   proofs to the card, export + spend them back (full wallet e2e HIL).
3. Wallet UI integration: card as a proof store alongside the local one.
