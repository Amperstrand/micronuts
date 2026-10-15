# Parity-audit session report — 2026-10-15 (nfc-differential-parity-night)

What we **did** vs what we **audited**, with evidence links. Session scope: bring
the Amperstrand/nucula fork's wallet onto the bench as a first-class labgrid
test target, drive micronuts↔nucula feature parity from a cited baseline, and
run the rig's money flows against a live micronuts mint on real hardware.

Bench: ai-legion (.208). Devices: m5stick (M5StickC Plus + MFRC522), nucula
(zeugmaster production PCB, ESP32-C3 + PN7160), ACR1252U (untouched beyond rig
preflight — CE rules respected, no CE scenario consumed). Mint:
micronuts-audit-adapter (FakeWallet) on `192.168.13.208:3338`, started from
the durable `rig/scripts/start-mint.sh`.

## 1. What we DID

### Amperstrand/nucula — branch `sisyphus/test-framework` → PR #13
| Commit | Change |
|---|---|
| 4a1f919 | rig: retarget to the .208 bench (coordinator, labgrid-client, mint URL env-overridable) |
| 606ef80 | rig(locks): per-board flock names |
| 22b4aa9 | rig(soak): `wstat` added to the snapshot cadence |
| e133cab | rig: accept Cargo.lock cdk 0.18.0→0.18.1 drift |
| 05448e4 | rig(session): durable start-mint.sh + full rig inventory doc (`docs/rig-inventory-2026-10-15.md`) |
| 41df1c8 | fix(console): guard USB-Serial-JTAG console on SoC capability — classic-esp32 link fix (latent since the m5stick lane existed) |
| 9cbd469 | board(nuculapcb): production-PCB lane, bench-proven PN7160 pinout (SDA4/SCL5/IRQ6/VEN7 from ccid-firmware-rs `pn7160-nci`), DWL drive becomes board-optional |
| fc784dd | fix(http): honor URL scheme — plain-http LAN mints were dragged onto TLS; this single line had blocked every mint op on the bench |
| 6ef8de7 | rig(e2e): money-loop asserts the balance delta, not the console echo (Hades2001 bridge chunk-drop class) |
| 3a1bc52 | rig(soak): invisible-success guard — no more recovery reboots of healthy devices |

Also: `nucula-rig` labgrid place recreated against the current .208 exporters
(acquire/release verified via `labgrid-client who`); issue #1 updated with the
session state; **issue #12** opened for the PN7160-silence anomaly.

### Amperstrand/micronuts — branch `sisyphus/parity-audit` → PR #105
- `docs/PARITY-MATRIX.md` — 49 cited rows (every C row traces to the rig
  inventory §A.1; every micronuts claim verified by file:line in-tree).
- Gap issues filed: **#98–#104** (multi-mint runtime mgmt, NFC reader silicon,
  invoice/claim/melt verbs, BIP-39 restore/wipe, persistence witness,
  NUT-18/23, console polish).

### Amperstrand/ccid-firmware-rs
- Bench environment contributions: shared IDF v5.5.5 tree
  (`/root/esp/esp-idf-v5.5.5`, tools in `~/.espressif` — the fork deliberately
  rides the bench-unified version; the 5.5.1 clone was killed), ufw 3338 open
  on the bench LAN, **issue #199** (esp toolchain naming blocks actdiag C3
  builds on .208).

### Bench state changes
- nucula flashed: fork wallet image (NUCULAPCB lane) — console/WiFi/status
  verified live; NFC frontend blocked by issue #12.
- m5stick flashed: fork wallet image (m5stick lane, incl. the console fix) —
  full money loop proven, then soak target.
- Mint service live on .208:3338 (durable; recipe = start-mint.sh).

## 2. What we AUDITED (results, all evidence on .208 unless linked)

| Area | Check | Result | Evidence |
|---|---|---|---|
| rig host tests | `cargo test` | **10/10** (incl. new flock test) | lane2 log |
| payer units | `cargo test --features payer` | **17/17** | lane2 log |
| payer vs live mint | 2 ignored tests | **2/2** — 1-sat token (374 B) embeds the .208 mint URL | lane2 log |
| Money loop e2e | `console_relay_money_loop` on m5stick hardware | **GREEN** — receive → NUT-12-verified swap → balance (+1) | rig test run, 2026-10-15 ~16:2x |
| Soak v1 | 4 h intended | **ABORT cycle 5** → root-caused to invisible-success + needless reboot (not a wallet bug) | `rig/results/soak-131626.jsonl` |
| Soak v2 (running) | money loops, wstat/heap/tasks snapshots, reboot-persistence every 25th | **in flight, 0 fails** — see appendix | `rig/results/soak-132250.jsonl` |
| Fork wallet on production PCB | boot, WiFi, console, `status`, `mint info` | **working** (mint info served live from the Rust mint) | session console captures |
| PN7160 on production PCB | `nci_init` probe | **silent** — probe-timeout/SDA-stuck class; power-cycle + bisect pending | fork issue #12 |
| Parity baseline | cited matrix | 49 rows: **14 HAVE / 17 PARTIAL / 11 MISSING / 7 BY-DESIGN** | docs/PARITY-MATRIX.md |

## 3. Performance observed (soak JSONL, m5stick console path)
- Money-loop cycle time: **1.6–6.5 s** across amounts 1/2/5/21/42 (first 23
  cycles; medians in the appendix when the soak closes).
- Console-transport ceiling encountered at the 42-sat amount (~2.7 KB token
  line) — the documented Hades2001 bridge class, now absorbed by the
  invisible-success guards rather than misattributed to the wallet.
- Mint-side quote→settle (FakeWallet auto-settle): bounded by the payer's 20 s
  wait; never the bottleneck in observed cycles.

## 4. Parity scorecard (from docs/PARITY-MATRIX.md)
- **Strong**: NUT-00..13 coverage in `cashu-core-lite` is essentially complete;
  micronuts is *ahead* on receive-path DLEQ enforcement (every credited proof
  NUT-12-verified) and on the fail-stop persistence design.
- **The real gaps** (issues filed): runtime multi-mint management (#98), NFC
  reader silicon behind the existing protocol/mocks (#99), interactive
  invoice/claim/melt verbs (#100), BIP-39 restore/wipe (#101), runtime
  persistence witnesses (#102), NUT-18/23 (#103), console polish (#104).
- Cross-stack proof this session: the C wallet and the Rust mint interoperate
  end-to-end over the LAN (V4 tokens, keysets, NUT-12 swap) — the parity
  target is a real wire-level peer, not a paper spec.

## 5. Open threads (future sessions)
1. **Phone-wallet NFC e2e** (fork issue #1 Session-2): a real phone replaces
   the ACR stand-in; characterize HCE/NDEF per wallet app.
2. **Boltcard relay + one-CE-scenario regression**: needs the card physically
   placed + one ACR power cycle (not done — no card placed this session).
3. **PN7160-silence bisect** (fork #12): USB power-cycle test, ccid actdiag
   chip-alive ladder (blocked on ccid #199), then port
   `bringup_pad_hold_clear`/`ven_cycle_extended` if the chip is alive.
4. **Tri-lateral witness experiments** (2 active + 1 monitor): designed but
   not run — requires both reader frontends healthy + a placed card; the
   witness-degradation design (m5stick-CCID + Escape 0xD0 polling) is
   documented in the session plan.
5. **esp toolchain naming on .208** (ccid #199) — unblocks 3.
6. Dependabot flags 1 moderate vulnerability on the fork's default branch
   (not session scope; surfaced by pushes).

## 6. Bench restore (final state)
Appendix updated at session close.
