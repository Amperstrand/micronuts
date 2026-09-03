# micronuts — agent onboarding

Cashu on microcontrollers: `cashu-core-lite` (no_std core) + hardware wallet
firmware + a backend-driven demo/prototype mint. Rust workspace; own git repo
(vendored inside cashu-cf at `src/micronuts/` — keep it a separate repo).

## Layout

| Crate | Role |
|---|---|
| `cashu-core-lite/` | no_std + alloc Cashu core (NUT-00..09/12/13), spec-quote-pinned |
| `micronuts-mint/` | The mint (backend-driven prototype; `LightningBackend` seam) |
| `micronuts-audit-adapter/` | axum JSON↔CBOR-RPC bridge; 13 NUT endpoints |
| `walletport/` | Offline gate validator + fuzz harness |
| `micronuts-app/` + `firmware/` | STM32F469I wallet hardware |
| `host-mint-tool/` | USB-CDC demo signer |
| `micronuts-fips-bridge/` | microfips service-boundary adapter |
| `micronuts-esp32-bridge/` | legacy WiFi→UART bridge (superseded) |
| `micronuts-esp32-mint/` | ESP32 esp-idf std mint front-end (standalone, house-style) |

## Commands (the sanctioned battery — mirrors `.github/workflows/rust-ci.yml`)

```bash
cargo +stable test -p cashu-core-lite --features std -p walletport -p micronuts-mint -p micronuts-fips-bridge
cargo +stable test -p micronuts-mint --features backend-upstream
cargo clippy -p cashu-core-lite -p walletport -p host-mint-tool -p micronuts-mint -p micronuts-fips-bridge -p micronuts-audit-adapter --all-targets -- -D warnings
cargo build -p cashu-core-lite -p walletport --target thumbv7em-none-eabihf
(cd firmware && cargo build --release)
cargo fmt --all --check
```

Do NOT `cargo test --workspace` on host — embassy-executor platform-feature
unification breaks it; the per-package split above is the workaround.

Spec-quote drift: `greatspectate check` per `cashu-core-lite/specquotes.toml`
(verbatim NUT quotes in `// NUT #XX:` comments; nuts clone at
`cashu-core-lite/nuts/`). Note zsh needs explicit file globs, not `$FILES`.

## Runtime knobs (mint_server / audit-adapter)

- `MICRONUTS_UPSTREAM_MINT` — upstream Cashu mint settlement backend
  (rugs02 model); unset = auto-settling FakeWallet.
- `MICRONUTS_RESERVE_BOOTSTRAP_SATS` — reserve bootstrap margin (default 1000).
- `MICRONUTS_UPSTREAM_PAY_TIMEOUT_SECS` — settle-poll window for real
  upstreams that need an external payer (default 5s; signut runs use 240).
- `MICRONUTS_MINT_STATE_FILE` / `MICRONUTS_RESERVE_STATE_FILE` — durable
  state (atomic snapshots; corrupt files REFUSE to boot; cross-era reserve
  snapshots panic). See docs/PERSISTENCE-DESIGN.md.
- Wallet e2e harness (`scripts/e2e_wallet.mjs`): `PAY_CMD` (payer snippet,
  run with `$INVOICE`), `SETTLE_POLL_TRIES`/`SETTLE_POLL_MS`, `MELT_INVOICE`
  + `MELT_AMOUNT`. Signut recipe: pay both the user quote and the printed
  bootstrap invoice via ssh → cln-hub nsenter lightning-cli.

## Rules

1. **Never file PRs/issues on upstream projects without human review** —
   document findings here first (cashu-core-lite/AGENTS.md policy).
2. Upstream `cashu` crate stays a dev/test oracle and (host/esp-idf only)
   runtime crypto provider; the embedded core stays no_std.
3. The `MintService`/CBOR-RPC boundary stays pure `cashu-core-lite` types
   (see micronuts-mint/PARITY.md invariant).
4. Money: any funded testing against external mints follows the cashu-cf
   AGENTS.md taxonomy — testnut/fake = free rein, signet = free rein,
   mainnet = never without explicit owner approval.
5. ESP32 builds run from `micronuts-esp32-mint/` with the `esp` toolchain;
   verify the artifact with `file` (host-binary trap).
6. **Wallet-interop tooling targets cashu-ts v4+ ONLY** (owner directive
   2026-09-02). cashu-ts 3.x is legacy — do not write new tests, scripts, or
   assertions against v3 idioms (`requestMint`, plain-number amounts). The
   e2e harness (`scripts/e2e_wallet.mjs`) hard-fails on majors < 4. Same
   rule noted in cashu-cf (ISSUE-098) and hackathon-tooling.

## Key docs

- `docs/ROADMAP.md` — phase order + ownership for open work
- `docs/AUDIT-2026-09-02-mint-prototype.md` — full audit + gap list
- `micronuts-mint/PARITY.md` — upstream parity map
- `docs/PERSISTENCE-DESIGN.md` — durable state (phases 1-3)
- `docs/STATUS-AND-TEST-PLAN.md` — hardware verification plan
- `docs/MINT-WALLET-DEMO.md` — RPC/wallet demo architecture
