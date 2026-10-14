# Parity matrix — nucula C firmware feature surface × micronuts

Baseline (C side): Amperstrand/nucula firmware console command tree + NUT
coverage, as enumerated with citations in
`/root/src/worktrees/nucula/test-framework/docs/rig-inventory-2026-10-15.md`
§A.1 (cited below as **A.1**). Every C row below traces to that document.

micronuts side: every `file:line` under "micronuts citation" was verified
by reading this worktree at commit `16c1f6d` (branch `sisyphus/parity-audit`).
Nothing is classified HAVE without such a citation.

Device framing: the nucula counterpart device is `micronuts-esp32-wallet`
(serial console, links `micronuts-wallet-core` + `cashu-core-lite`);
`micronuts-app`/`firmware` is the STM32F469 app surface where relevant.

Classes:

- **HAVE** — capability present and verified at file:line in this worktree.
- **PARTIAL** — part of the capability exists; the note names what is missing.
- **MISSING** — no counterpart found in this worktree.
- **BY-DESIGN** — no counterpart because micronuts hardware/design makes
  the C capability moot; note explains.

Row counts: **49** — HAVE **14** · PARTIAL **17** · MISSING **11** ·
BY-DESIGN **7**.

---

## 1. Wallet console commands (A.1 `main/commands_wallet.cpp`)

| Capability | C status | micronuts status | Class | C citation | micronuts citation | Note |
|---|---|---|---|---|---|---|
| `status` — wifi/nfc/heap/unit/mints/keysets/proofs/totals overview | exists | scattered verbs only: `wifi`, `connect`, `heap`, `balance` + boot probes | PARTIAL | A.1: `cmd_status` :29, reg :599 | `micronuts-esp32-wallet/src/main.rs:221-233` (wifi), `:238-245` (connect), `:247-249` (heap), `:246` (balance), `:99-110` (boot NUT-01/06 probes) | Missing: one consolidated status verb; NFC state, unit, mint list, keyset/proof counts, totals are not reported anywhere |
| `balance` — per-proof listing, subtotals, totals | exists | total sats only | PARTIAL | A.1: `cmd_balance` :71, reg :600 | `main.rs:246` → `micronuts-wallet-core/src/engine.rs:190` (`balance()` sums proofs) | Missing: per-proof display and per-denomination subtotals |
| `unit [<name>]` — show/set persisted default unit | exists | unit is carried (keyset + token mint, sat default), never user-switchable | PARTIAL | A.1: `cmd_unit` :107, reg :601 | `cashu-core-lite/src/nuts/nut02.rs:27` (`KeysetInfo.unit`), `micronuts-wallet-core/src/engine.rs:408` (unit stamped into `TokenV4`) | Missing: console verb + persisted default-unit preference/multi-unit switching |
| `receive <cashu token>` — cashuA/cashuB, swap + balance | exists | full path incl. optional HTLC preimage | HAVE | A.1: `cmd_receive` :151, reg :602 | `main.rs:254-265` → `engine.rs:296-353`; V3+V4 decode `micronuts-wallet-core/src/token_compat.rs:13-19,27`; NUT-03 swap `engine.rs:339-344`; DLEQ verify `engine.rs:345-348` | Accepts both `cashuA`/`cashuB` like C; additionally verifies NUT-12 DLEQ on every received proof |
| `mint [list\|add\|remove\|info]` — runtime multi-mint management | exists | none — single build-time-baked mint URL | MISSING | A.1: `cmd_mint` :200 (add :220, info :249, remove :294), reg :603 | `main.rs:9-12` (`MICRONUTS_MINT_URL` compile-time default, no runtime override) | No list/add/remove/info verbs, no mint storage, no `w=` mint index anywhere in the console loop (`main.rs:206-332`) |
| `invoice <amount> [u= m= w=]` — NUT-04 quote → BOLT11 | exists | engine + kiosk bin have it; wallet console does not | PARTIAL | A.1: `cmd_invoice` :325, reg :604 | engine: `engine.rs:207-218` (`mint_via_invoice`), `:220-228` (`poll_mint_quote`); on-ESP32 invoice display: `micronuts-esp32-wallet/src/bin/terminal_kiosk.rs:207` (`invoice_cycle`) | Missing: `invoice` verb in the esp32-wallet console (money ops in `main.rs` cover receive/send only) |
| `claim <quote_id> [m= w=]` — mint paid quote | exists | engine has it; wallet console does not | PARTIAL | A.1: `cmd_claim` :382, reg :605 | `engine.rs:230-242` (`mint_paid_quote`) | Missing: console verb; also no quote-id input surface on device |
| `melt <request> [u= m= a= w=]` — pay invoice with ecash | exists | engine has full melt; wallet console does not | PARTIAL | A.1: `cmd_melt` :458, reg :606 | `engine.rs:557-569` (`quote_melt`), `:570-` (`melt`) | Missing: console verb on the esp32-wallet device |
| `stickup` — drain all funds as V4 tokens | exists | `send <amount>` composes exact-amount tokens; no drain-all | PARTIAL | A.1: `cmd_stickup` :528, reg :607 | `main.rs:266-273` → `engine.rs:390-410` (`send_token`, V4 wire string) | Missing: single drain-all verb (equivalent is `send <balance>`, but balance is total-only and the two-step is manual) |

## 2. Seed console command (A.1 `main/commands_seed.cpp`)

| Capability | C status | micronuts status | Class | C citation | micronuts citation | Note |
|---|---|---|---|---|---|---|
| `seed [show\|generate\|restore <12 words>\|wipe]` (NUT-13; generate/restore/wipe erase all wallets) | exists | show (hex) + implicit first-boot generate | PARTIAL | A.1: `cmd_seed` :21, usage :101, reg :106 | show: `main.rs:250` → `engine.rs:202` (`seed_hex`); generate-on-first-boot: `main.rs:116-137` | Missing: console re-generate, restore-from-words, wipe verbs (see seed-lifecycle rows below) |

## 3. System console commands (A.1 `main/commands_system.cpp`)

| Capability | C status | micronuts status | Class | C citation | micronuts citation | Note |
|---|---|---|---|---|---|---|
| `nfc [request <amount>\|stop]` — NFC receive flow control | exists | CDC protocol + handler wired; no silicon reader, no request/stop semantics | PARTIAL | A.1: `cmd_nfc` :40 (request :46-67, stop :68-73), reg :511 | `micronuts-app/src/protocol.rs:18-20` (NfcPoll/NfcData/NfcHeal), `command_handler.rs:89-107`; F469 impl stub `firmware/src/hardware_impl.rs:421-436` | Missing: NFC reader hardware driver (stub returns `NotConnected`), amount-request/stop flow control |
| `keypad scan` — 30 s PCF8574 probe | exists | no keypad — different boards | BY-DESIGN | A.1: `cmd_keypad` :168, reg :512 | nearest input-equivalent: F469 capacitive touch, `firmware/src/self_test.rs:469` (`test_touch`) | PCF8574 matrix keypad is atom/m5stick hardware; micronuts devices use FT6X06 touch (F469) / serial console (ESP32) |
| `i2cscan` — bus + pin-pair sweep | exists | none | BY-DESIGN | A.1: `cmd_i2cscan` :289, reg :513 | — | Bench bring-up diagnostic for nucula I2C peripherals; micronuts F469 I2C (touch) is BSP-managed (`firmware/AGENTS.md` hardware setup), no scan surface needed or present |
| `i2crecover <clocks>` — SCL-clock a stuck bus | exists | none | BY-DESIGN | A.1: `cmd_i2crecover` :446, reg :514 | — | Same rationale as `i2cscan`; no micronuts I2C wedge lore exists to recover |
| `i2cdump <sda> <scl> <addr>` — reg dump (atom/m5stick only) | exists | none | BY-DESIGN | A.1: `cmd_i2cdump` :241, reg :516 (`#if` :218/:515) | — | Board-scoped debug tool; no micronuts counterpart board peripheral |
| `nfcdump [pages] [start]` — T4 ISO-DEP NDEF dump / T2 Ultralight pages | exists | NDEF payload read only (via stubbed reader) | PARTIAL | A.1: `cmd_nfcdump` :331 (T4 :355-412, T2 :414-426), reg :517 | `command_handler.rs:100-103` (NfcData → raw NDEF payload), `protocol.rs:19` | Missing: raw page/register dumps, CC/ISO-DEP level access, and any real reader to drive them |
| `display <on\|off\|fill>` — m5stick only | exists | full UI instead of console display verbs | BY-DESIGN | A.1: `cmd_display` :464, reg :520 (`#if` :463) | `firmware/src/self_test.rs:428` (`test_display`); app screens `micronuts-app/src/display.rs` | F469 has a 480×800 app UI (11 screens) + boot self-test display check; console on/off/fill verbs are m5stick-specific |
| `button` — 5 s press window (m5stick only) | exists | touch input instead | BY-DESIGN | A.1: `cmd_button` :488, reg :521 | `firmware/src/hardware_impl.rs:405-413` (touch poll) | F469 FT6X06 touch is the input surface; ESP32 lane has no button console verb either |
| `wstat` — persistence witness: saves/fails/last_err/blob/proofs_loaded + NVS entry stats | exists | blob-size leg only; no witness counters | PARTIAL | A.1: `cmd_wstat` :431, reg :523 | stored-len boot print `main.rs:48-51` → `micronuts-esp32-wallet/src/lib.rs:42-48`; fail-stop errors `lib.rs:66-77` | Missing: save/fail counters, last_err, NVS entry stats, proofs_loaded (grep for `saves\|last_err\|wstat` counters: zero hits in device crates) |
| `reboot` | exists | none | MISSING | A.1: `cmd_reboot` :77, reg :524 | — (no `esp_restart`/reboot handling in `main.rs:206-332`) | Trivial gap; esp-idf `esp_restart()` just not wired to a verb |
| `heap` — free / largest block / min-ever | exists | free only | PARTIAL | A.1: `cmd_heap` :89, reg :525 | `main.rs:247-249` (`esp_get_free_heap_size`) | Missing: largest-free-block and minimum-ever-free watermark |
| `tasks` — per-task prio + stack high-water | exists | none | MISSING | A.1: `cmd_tasks` :99, reg :526 | — | No task-listing verb on the ESP32 console |
| `log <e\|w\|i\|d> [tag]` — runtime log level | exists | none (compile-time esp-idf level) | MISSING | A.1: `cmd_log` :117, reg :527 | — | No runtime log-level/tag verb in the console loop |
| `bench` — crypto primitive benchmark | exists | none (untimed crypto self-exercise only) | MISSING | A.1: `cmd_bench` :138, reg :528 | closest: `firmware/src/self_test.rs:269` (`test_crypto_blinding`, pass/fail not timed) | No on-device perf measurement of k256/CBOR primitives |
| `selftest` — crypto/keyset/unit/json/wallet self-tests (console) | exists | boot-time self-tests on both device lanes | PARTIAL | A.1: `cmd_selftest` :146, reg :529 | F469 boot self-test `firmware/src/self_test.rs:57` (`run_all`: SDRAM :130, RNG :176, heap :211/:231, crypto :269, USB-CDC :357, display :428, touch :469); ESP32 boot smokes `main.rs:53-58` (store round-trip), `:99-110` (NUT-01/06 probes) | Missing: console-triggerable re-run — micronuts self-tests fire at boot only |
| `help` | exists | exists | HAVE | A.1: `main/console.cpp:216` | `main.rs:235-237` | Lists the esp32-wallet verb set |

## 4. NUT coverage (A.1 §B(v): NUT-00/02/03/04/05/10/11/12/13/18/23)

| Capability | C status | micronuts status | Class | C citation | micronuts citation | Note |
|---|---|---|---|---|---|---|
| NUT-00 — base types, blinding, amounts | exists | full | HAVE | A.1 §B(v) (root README Protocol) | `cashu-core-lite/src/nuts/nut00.rs:22,39,59,100`; token V4 `cashu-core-lite/src/token.rs:214,331,351,359`; on-device blind/unblind `firmware/src/self_test.rs:269` | — |
| NUT-02 — keysets/keys | exists | full | HAVE | A.1 §B(v) | `nuts/nut02.rs:21,39,54,134` (v1/v2 id derivation), `:175` (`verify_keyset_id`); transport `cashu-core-lite/src/wallet.rs:60,65`; boot probe `main.rs:100-103` | — |
| NUT-03 — swap | exists | full | HAVE | A.1 §B(v) | `nuts/nut03.rs:19,31`; `wallet.rs:126` (`swap`); deterministic swap `cashu-core-lite/src/persistent.rs:492` | Used on every receive (change-break) and send (exact-amount compose) |
| NUT-04 — mint quotes | exists | full | HAVE | A.1 §B(v) | `nuts/nut04.rs:17-19,24,43,89,105`; `wallet.rs:70,83,96`; engine `engine.rs:207-242` | — |
| NUT-05 — melt quotes | exists | full | HAVE | A.1 §B(v) | `nuts/nut05.rs:18-20,30,42,78,92`; `wallet.rs:149,161,171`; engine `engine.rs:557,570` | — |
| NUT-10 — spend conditions / secret kinds | exists | full | HAVE | A.1 §B(v) | `nuts/nut10.rs:30,64,104`; strict embedded-JSON scanner `nuts/mod.rs:25-26`; witness build `micronuts-wallet-core/src/conditions.rs:92` (`witness_for_secret`), used `engine.rs:323-331` | — |
| NUT-11 — P2PK locking | exists | full (receive-side unlock + identity) | HAVE | A.1 §B(v) | `nuts/nut11.rs:91-296` (conditions/requirements/witness), `:115,202,271`; identity `conditions.rs:69,76`; console `p2pk` verb `main.rs:251` → `engine.rs:358` | micronuts exposes its lock-facing pubkey on the device console |
| NUT-12 — DLEQ proofs | exists | full, enforced on receive | HAVE | A.1 §B(v) | `nuts/nut12.rs:230,278,315`; verification `engine.rs:697-710`, enforced `engine.rs:345-348` | Every credited proof is DLEQ-verified or rolled back |
| NUT-13 — deterministic secrets/blinders from seed | exists | derivation only, no mnemonic entry | PARTIAL | A.1 §B(v); seed cmd A.1 `commands_seed.cpp:21` | `nuts/nut13.rs:106` (`derive_secret`), `:135` (`derive_blinder`), spec vectors `:224-239`; consumed via `persistent.rs:492` | Missing: BIP-39 mnemonic→seed — the 64-byte BIP-39 seed appears only as a test vector (`nut13.rs:210-215`); device seed is raw 32 B (`main.rs:116-137`) |
| NUT-18 — payment requests | exists | none | MISSING | A.1 §B(v) | — (grep `nut18\|NUT-18\|payment_request` across `cashu-core-lite/src`, `micronuts-wallet-core/src`, `micronuts-esp32-wallet/src`, `micronuts-app/src`: zero hits; module list `nuts/mod.rs:7-23` has no nut18) | No payment-request type anywhere in the micronuts device stack |
| NUT-23 — wallet-to-wallet (pure/quote tokens) | exists | none | MISSING | A.1 §B(v) | — (grep `nut23\|NUT-23`: zero hits; `token.rs`/`token_compat.rs` carry no pure/quote-token variant) | Blocks boltcard-style in-person transfer flows |

micronuts-only NUTs (no C row, not counted): NUT-06 info (`wallet.rs:55`),
NUT-07 check-state (`wallet.rs:184`, `engine.rs:367`), NUT-09 restore
(`engine.rs:676`), NUT-14/19/20/29 modules (`nuts/mod.rs:20-23`, mint-side).

## 5. NFC flows

| Capability | C status | micronuts status | Class | C citation | micronuts citation | Note |
|---|---|---|---|---|---|---|
| NFC receive flow — poll tag, read NDEF, extract token, redeem | exists (`nfc request`; proven by rig e2e `relay_token_over_the_air_via_boltcard`) | protocol + handler + QR/NFC differential import path, mock-tested; no silicon | PARTIAL | A.1: `cmd_nfc` :46-67; §A.3 e2e :55 | `protocol.rs:18-20`; `command_handler.rs:89-107`; shared capture-import path `command_handler.rs:111-147` (differential-pair contract :111-117); mock test `command_handler.rs:703` | Missing: an NFC reader driver — F469 impl is a stub returning `NotConnected` (`firmware/src/hardware_impl.rs:421-436`); no ESP32-lane NFC at all |
| NDEF token extraction (bare / `#token=` / `token=` / embedded text) | exists (device-side extraction) | payload classification + token import shared with QR; NDEF record stripping sits behind the (stub) reader | PARTIAL | A.1 §A.2 `ndef_t2t.rs:143` (mirrors device strategies) | `command_handler.rs:123-131` (`decode_qr` classifies CashuV4/V3/UR/plain/binary and imports), `:139-147` (UR assembler + import) | Missing: in-repo NDEF TLV/text-record parser — `nfc_read_ndef` is expected to hand over the payload (`command_handler.rs:100-103`) and the hw impl that would strip NDEF does not exist yet |
| Card emulation / send-direction NFC (boltcard-style present) | PN7160 CE is the XIAO firmware feature; not console-exposed | none | MISSING | A.1 §B(ii) (`nfc` is receive/reader mode only; CE = XIAO firmware) | — (no emulation code path in any micronuts device crate) | Also blocked upstream of micronuts by NUT-18/NUT-23 absence |

## 6. Seed lifecycle

| Capability | C status | micronuts status | Class | C citation | micronuts citation | Note |
|---|---|---|---|---|---|---|
| First-boot seed generation + NVS persistence | exists (`seed generate`) | exists (automatic, hardware RNG) | HAVE | A.1: `commands_seed.cpp:21`, `:15-19` | `main.rs:116-137`: load-or-generate (`esp_fill_random` :127, save `:132-134`), fed to the engine `main.rs:154-155` | micronuts has no explicit verb — generation is first-boot-automatic |
| Seed display (`seed show`) | exists | exists (hex, console) | HAVE | A.1: `cmd_seed` :21 | `main.rs:250` → `engine.rs:202` (`seed_hex`) | C shows 12 words; micronuts shows the raw 32-byte hex seed |
| `seed restore <12 words>` — BIP-39 mnemonic entry | exists | none | MISSING | A.1: `cmd_seed` :21, usage :101 | — (no wordlist/mnemonic code anywhere; BIP-39 appears only as a test-vector comment `nut13.rs:210-215`) | A nucula 12-word wallet cannot be restored onto micronuts hardware today |
| `seed wipe` (and wallet-erase-on-reseed semantics) | exists (generate/restore/wipe erase all wallets :15-19) | none | MISSING | A.1: `commands_seed.cpp:15-19` | — (no wipe/reseed verb in `main.rs:206-332`; no destructive-erase path) | No device path to destroy wallets + seed on demand |

## 7. Persistence / wstat class

| Capability | C status | micronuts status | Class | C citation | micronuts citation | Note |
|---|---|---|---|---|---|---|
| Atomic single-key NVS wallet blob (write+commit one call) | exists | exists | HAVE | A.1: `wstat` :431 (witnesses the saves), §A.5 soak reboot-persistence | `micronuts-esp32-wallet/src/lib.rs:51-77` (`NvsProofStore`, one `set_blob` per save; contract doc :1-13); persist-on-success `cashu-core-lite/src/persistent.rs:242,325,356,403,445,514-522`; store load at boot `persistent.rs:179` | Encodes the nucula#1 lesson explicitly (lib.rs header) |
| Fail-stop blob size bound (refuse unsavable state; anti-silent-RAM-wallet) | implicit in C (the ~790-sat boundary is the failure this prevents) | exists | HAVE | A.1 §A.6 item 9 (`boundary_probe.rs`, ~790-sat boundary) | `lib.rs:66-73` (oversize save returns error), bound set 32 KiB at `main.rs:44` (wallet) and `:152-153` (engine state) | micronuts fails loudly instead of silently degrading — the direct antidote to the C failure class |
| Persistence witness — runtime counters (saves/fails/last_err/proofs_loaded) queryable on demand | exists (`wstat`) | none (blob size at boot only) | MISSING | A.1: `cmd_wstat` :431-444 | — (no counter/stats accumulator in `lib.rs` or `engine.rs`) | Capability view of the §3 `wstat` gap (PARTIAL there via the blob-size leg): the counters themselves exist nowhere |
| Balance survives reboot (NVS-persisted proofs reloaded) | exists (soak reboots + verifies every 25th cycle) | wired + boot-smoked; no on-device reboot-persistence verification | PARTIAL | A.1 §A.5 (`soak.rs:271-297`) | engine store wired `main.rs:152-166`; `PersistentWallet` loads proofs at construction `persistent.rs:179`; boot blob print `main.rs:48-51` | Missing: a reboot-verification harness on device (nucula soak proves it every 25th cycle; micronuts has no equivalent e2e) |
| Persistence-failure boundary probing (blob growth vs NVS capacity) | exists (`boundary_probe`) | obviated by fail-stop design | BY-DESIGN | A.1 §A.5 (`boundary_probe.rs:1-4`), §A.6 item 9 | `lib.rs:66-73` + `main.rs:44` | With a loud 32 KiB fail-stop bound there is no silent boundary to probe; saves that cannot fit refuse loudly at `save()` time |

---

## Counts

| Class | Rows |
|---|---|
| HAVE | 14 |
| PARTIAL | 17 |
| MISSING | 11 |
| BY-DESIGN | 7 |
| **Total** | **49** |

## Top 5 load-bearing gaps

1. **No runtime mint management** (`mint` MISSING) — a single build-time
   `MICRONUTS_MINT_URL` (`main.rs:9-12`) versus C's list/add/remove/info
   with per-command `w=<mint_idx>`. Every money verb on the C side is
   multi-mint; micronuts device is single-mint by construction.
2. **NFC reader silicon** (two PARTIAL rows) — the protocol
   (`NfcPoll/NfcData/NfcHeal`), the shared QR/NFC differential import
   path, and mock tests all exist (`command_handler.rs:89-147`), but the
   only firmware impl is a stub (`hardware_impl.rs:421-436`): the whole
   NFC receive flow cannot run on hardware, and NDEF record stripping is
   unimplemented.
3. **invoice/claim/melt not console-exposed on the device** — the engine
   has the full NUT-04/05 lifecycle (`engine.rs:207-242,557-635`) and the
   kiosk bin even renders invoice QRs on an ESP32
   (`terminal_kiosk.rs:207`), but the wallet console exposes only
   receive/send/p2pk/tollgate: the device cannot top up or pay invoices
   interactively.
4. **BIP-39 mnemonic restore/wipe** (NUT-13 PARTIAL, two MISSING rows) —
   seed is raw 32-byte hex with automatic first-boot generation only
   (`main.rs:116-137`); no 12-word entry, no wipe/erase path. Cross-device
   recovery from a nucula (or any CDK) wallet is impossible.
5. **No persistence witness / on-device reboot-persistence proof** — the
   nucula `wstat` counters + soak reboot verification are exactly the
   instrumentation that caught their ~790-sat silent-corruption class;
   micronuts has the superior fail-stop *design* (`lib.rs:66-73`) but zero
   runtime witnesses or reboot e2e to prove it on silicon.

Runners-up: NUT-18/NUT-23 absence (blocks boltcard-style send-direction and
wallet-to-wallet flows outright) and the missing `tasks`/`log`/`bench`
diagnostics for soak-class campaigns.
