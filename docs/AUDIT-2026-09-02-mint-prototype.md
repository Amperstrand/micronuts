# Micronuts Audit — 2026-09-02 (mint-prototype readiness)

**Scope**: whole micronuts workspace, with focus on turning `micronuts-mint`
into a real, wallet-interopable Cashu mint prototype (host today, ESP32 next).
**Method**: full local CI battery replication + cross-repo best-practice study
(hackathon-tooling, tollgate-s3-rs, amp-embedded-common, bolty-rs,
ccid-firmware-rs, t-a7670g-tollgate).

---

## 1. Local CI battery (all gates from `rust-ci.yml`, run 2026-09-02 on macOS)

| Gate | Result | Notes |
|---|---|---|
| `cargo fmt --all --check` | ✅ | |
| host tests `cashu-core-lite --features std` | ✅ 151 tests | |
| host tests `walletport` | ✅ 20 tests | incl. upstream-CDK interop |
| host tests `micronuts-mint` | ✅ 30 tests | unit + rpc_loopback + mint_role + e2e |
| host tests `micronuts-fips-bridge` | ✅ 5 tests | |
| demo smoke (`--bin demo`) | ✅ | "Remaining balance: 36 sats", "Demo complete" |
| transport feature matrix (serial / usb-cdc / microfips) | ✅ | 3 builds |
| clippy host `-D warnings` (6 crates) | ✅ | |
| no_std thumb builds (ccl + walletport) | ✅ | `thumbv7em-none-eabihf` |
| firmware release build | ✅ | macOS LLVM handles `wfe/sev` (Linux-only issue) |
| firmware thumb clippy | ⏭ skipped locally | low risk; release build green |
| adapter smoke (`/v1/info`,`/v1/keys`,`/v1/keysets`) | ✅ | live HTTP verified |
| spec-quote drift (greatspectations vs nuts HEAD) | ✅ exit 0 | note: `$FILES` needs explicit globbing under zsh |
| `cargo deny` advisories+bans | ✅ | |

**Conclusion: the tree is green.** Known caveat: `cargo test --workspace`
fails on host (embassy-executor platform-feature unification when
`micronuts-app`/`firmware` are resolved together) — CI's per-package test
split is the sanctioned workaround; do not "fix" by feature-union.

## 2. Best-practice harvest (cross-repo study)

### From hackathon-tooling (private; cloned for study)

- **Payment safety** (`checklists/cashu-payment-safety.md`,
  `patterns/cashu/atomic-proof-dedup.md`, `patterns/cashu/quote-rollback.md`,
  `patterns/cashu/stale-proof-recovery.md`): every intermediate state needs a
  defined rollback; proof dedup must be atomic claim (never read-then-write);
  never release PENDING proofs while the settlement operation is still active.
- **Cashu service checklist** (`checklists/cashu-service.md`): machine-readable
  compatibility contract (NUTs, units, fees, quote states, limits).
- **CI** (`ci-templates/host-tier-push-ci.yml`): cargo-nextest + bounded
  timeouts, hardware-only crates excluded from host tier.
- **ESP32 ops lessons** (`docs/session-learnings/2026-08-24-esp32-flash-ops.md`):
  build from the app dir (a workspace-root green build can be an x86 binary);
  verify artifacts with `file`; custom partition CSV passed to espflash;
  `-Zbuild-std` for Xtensa.
- **Interop**: build an explicit matrix vs `testnut`/`signut`
  (`prompts/workflows/verify-end-to-end.md`); ecosystem-wide NUT failure modes
  documented in `docs/ecosystem/nut-auditor-findings-2026-07.md`.

### From embedded-Rust repos (tollgate-s3-rs, amp-embedded-common, bolty-rs, ccid-firmware-rs)

- **Upstream `cashu` crate (=0.17.3) already builds for
  `xtensa-esp32s3-espidf`** (tollgate-s3-rs `apps/bolty-esp32/Cargo.toml`) —
  no de-std of the mint crypto is required for the ESP32 std path.
- House dependency pins: `esp-idf-sys =0.37.2`, `esp-idf-hal =0.46.2`,
  `esp-idf-svc =0.52.1`, `embuild =0.32`; `.cargo/config.toml` with
  `xtensa-esp32(-s3)-espidf`, ldproxy, `build-std=["std","panic_abort"]`,
  `espidf_time64`; `rust-toolchain.toml` channel `esp`; minimal
  `build.rs` calling `embuild::espidf::sysenv::output()`.
- **lib/bin split** (ccid-firmware-rs) so glue logic is host-testable.
- **Reliability patterns** (amp-embedded-common): dependency-free diagnostics
  struct with golden byte tests; clock trait with host test double for
  deterministic timeout tests; 3-failure reinit tracker.
- **Runtime discipline** (tollgate-s3-rs): 32 KiB stacks for HTTP handlers
  that parse tokens + do secp256k1; crypto/persistence on named worker
  threads, never the main task; typed `WifiError` manager around
  `BlockingWifi`; bounded NVS strings/blobs with explicit commits.
- **CI that actually builds ESP32** (ccid-firmware-rs): espup + cached
  `~/.espressif` + `cargo +esp build` matrix + `ESP_IDF_SDKCONFIG_DEFAULTS`
  exported explicitly.

## 3. Findings & gap list

| # | Severity | Finding | Status |
|---|---|---|---|
| F1 | P0 | `mark_spent` silently accepts double-spends (no error on already-spent Y) | **fixing now** (rework task) |
| F2 | P0 | `verify_proofs` does not check `proof.id` against the active keyset — foreign-keyset proofs with a colliding denomination verify against our keys only if sig matches (low likelihood, wrong invariant) | **fixing now** |
| F3 | P1 | NUT-06 `nuts` field emits `{"supported":[...]}` — spec (and every wallet) expects a map `"4":{"methods":[{"method":"bolt11","unit":"sat"}]}` | **fixing now** |
| F4 | P1 | Swap/melt ignore NUT-08 input fees when `input_fee_ppk > 0` (exact-equality balance check) | **fixing now** (cashu-cf formula `(sum_ppk+999)/1000`) |
| F5 | P1 | Quote lifecycle: mint quotes are born PAID (no UNPAID→PAID transition to observe); melts have no PENDING/FAILED + no rollback of claimed proofs on payment failure | **fixing now** |
| F6 | P1 | NUT-09 restore is a stub returning empty | **fixing now** (B_→signature ledger) |
| F7 | P1 | No Lightning-backend abstraction — invoice/amount/preimage are string hacks at the mint boundary | **fixing now** (`LightningBackend` trait + `FakeWallet`) |
| F8 | P1 | No persistence: spent set, quotes, issued outputs are RAM-only (double-spend across restarts, quotes lost) | open — design: NVS/LittleFS on device, file on host; next milestone |
| F9 | P2 | `fee_reserve` conflated with `input_fee_ppk`; real LN fee reserve unknown until a real backend exists | open (backend trait models it) |
| F10 | P2 | Demo keyset keys from deterministic SHA-256 seed — fine for demo, must NOT back real value on device | open (by-design for now) |
| F11 | P2 | RNG quality audit (firmware) pending — existing issue #1 | open |
| F12 | P2 | `micronuts-esp32-bridge` on esp-idf-svc 0.51 (house = 0.52.1), no `.cargo/config.toml`/sdkconfig/partitions, WiFi creds hardcoded in source | superseded by `micronuts-esp32-mint` scaffold (house-style) |
| F13 | P2 | No ESP32 CI job that cross-builds (ccid pattern exists to copy) | scaffolded with new crate |
| F14 | P3 | `cargo test --workspace` broken on host (embassy platform feature unification) | documented; CI split is sanctioned |
| F15 | P3 | Repo hygiene: was vendored untracked inside cashu-cf; dangling `.beads` symlink removed 2026-09-02 (no reachable data) | done |
| F16 | P1 | **Wallet-interop tooling must target cashu-ts v4+ only** (owner directive 2026-09-02; v3 = legacy, migration tracked in cashu-cf ISSUE-098). The first e2e draft used v3 idioms (`requestMint`) that do not exist in v4 — a silent-drift trap. `scripts/e2e_wallet.mjs` now hard-fails on majors < 4. Note also surfaced during this: NUT-04 quote responses MUST carry the accounting fields (`amount`, `amount_paid`, `amount_issued`, `updated_at`) or cashu-ts v4 throws `AmountError: Unsupported amount input type` — implemented same day. | done (guard + fields) |

## 4. Decisions

1. **Do not port `cdk`/`cdk-common` to the MCU.** `cashu-core-lite` is the
   embedded core (differential-tested vs upstream `cashu` 0.17.3); upstream
   crate remains the oracle. For the ESP32 std target, upstream `cashu`
   compiles (tollgate-s3-rs evidence) — the esp32 mint uses `micronuts-mint`
   (which wraps it) directly.
2. **The audit-adapter stays the JSON↔RPC bridge**; backends and safety live
   inside `micronuts-mint` behind `LightningBackend`, so the CBOR RPC seam and
   the conformance harness keep working unchanged.
3. **Upstream-settlement backend** (the cashu-cf rugs02 model: front mint
   backed by a real upstream mint) comes after the fake-backend prototype is
   proven with a real wallet; testnut (fake) first, signet later.
4. **Money rules** (from cashu-cf AGENTS.md): testnut = fake = free rein;
   signut = signet = free rein; rugs* = mainnet = zero spend without explicit
   owner go. The micronuts prototype only ever targets the first two.

## 5. Verification plan for the prototype (P4)

1. Unit/integration suite green (this repo).
2. `micronuts-audit-adapter` smoke + cashu-audit matrix vs localhost.
3. Real-wallet e2e (cashu-ts script): mint → swap → checkstate → melt against
   the local prototype with FakeWallet backend.
4. Same wallet flow with the prototype backed by testnut (fake upstream) —
   full cycle over real HTTPS.
5. (stretch) signut-backed cycle on signet.

**Results (2026-09-02, host prototype):**

- (1) ✅ 51 mint tests + 20 ccl suites green; fmt/clippy/spec-quote/cargo-deny clean.
- (2) ✅ cashu-audit matrix: **71/109 PASS** (baseline before rework: 67/107).
  Remaining 38: ~35 × NUT-10/11/14 spending conditions (unimplemented, out of
  v1.0 scope by design — PORTING_STATUS), NUT-20 quote locking, NUT-29 batch,
  `mint_quote_uuid_v7` (counter IDs; candidate quick fix), NUT-19 cache.
- (3) ✅ `node scripts/e2e_wallet.mjs` — **E2E-WALLET PASS, 13 assertions**
  with cashu-ts **4.10.0**: quote UNPAID→PAID on poll, mint, swap (21-sat
  split), melt with NUT-08 blank-output change (54 sats), deterministic
  preimage, double-spend rejection.
- (4) ✅ **upstream-backed cycle (testnut)**: same e2e with
  `MICRONUTS_UPSTREAM_MINT=https://testnut.cashu.exchange` —
  **E2E-WALLET PASS, 14 assertions** over real HTTPS: users get REAL testnut
  bolt11 invoices; the reserve bootstraps 1000+ sats from testnut and settles
  melts upstream (upstream preimage asserted). Live unit roundtrip
  (`testnut_live_roundtrip`, `--ignored`) green.
  Upstream-behavior findings (cashu-cf saga v2):
  - melt responses carry `preimage`, not `payment_preimage` (reserve accepts
    both);
  - saga-v2 returns **no melt change to anyone** (verified with a real v4
    wallet) — the reserve selects minimally (ascending) so the kept-overpay
    loss is dust (7 sats on a 21-sat melt), and post-PAID bookkeeping is
    warnings-only (never error after payment);
  - melting an invoice whose mint-quote was already consumed at the same mint
    is refused (internal-settlement guard) — use fresh invoices as melt
    targets.

## 6. Sources

- Local: `rust-ci.yml`, `micronuts-mint/PARITY.md`, `docs/MINT-WALLET-DEMO.md`,
  `docs/STATUS-AND-TEST-PLAN.md`.
- Cross-repo: hackathon-tooling (private clone), tollgate-s3-rs (private
  clone), amp-embedded-common, bolty-rs, ccid-firmware-rs, t-a7670g-tollgate.
- cashu-cf: `src/core/nut02.ts` (fee formula), live `testnut.cashu.exchange/v1/info`
  (NUT-06 shape reference).
