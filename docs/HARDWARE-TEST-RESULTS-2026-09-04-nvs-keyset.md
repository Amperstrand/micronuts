# Hardware test results — 2026-09-04 — NVS keyset + state battery (#60 #56)

Board: **M5 Atom `81528A13B6`** (ESP32-PICO-D4, 4 MB flash) standing in for
the planned ESP32-D0WD (not attached to this bench today). Restored to its
fips-lab quiesce image after the battery. Run under the Amperstrand bench
flock via cron (`scripts/test_hw_nvs_keyset.sh`, release build, bench WiFi).

## Two firmware defects the battery caught (compile-only verification had missed both)

1. **Main-task stack overflow.** First flash boot-looped with
   `***ERROR*** A stack overflow in task main` right after the
   provisioning log lines — mint construction (demo keyset derivation +
   seeded re-derivation + init snapshot serialization) needs more than
   the configured 32 KiB. Fix: `CONFIG_ESP_MAIN_TASK_STACK_SIZE=65536`
   (`sdkconfig.defaults`). Notably the crash loop itself already proved
   generation + persistence + reload: boot 1 logged `first boot:
   generated mint keyset seed (NVS)`, boot 2 logged `loaded persisted
   mint keyset seed` (the seed survived the crash reboot).
2. **Debug-build task-watchdog trip.** With 64 KiB the debug build got
   past construction but tripped the task watchdog (`task_wdt: IDLE1`)
   ~5 s in — k256 keyset derivation in an unoptimized build takes >5 s.
   Fix: the battery flashes `--release` (what hardware should run).

Also fixed en route: the injected-snapshot generator invocation
(`nvs_partition_gen.py` needs `file,binary,<path>` rows and a `.bin`
output name).

## Green transcript (2026-09-04 21:57–21:58, release build)

```
PASS: mint board on /dev/serial/by-id/usb-M5STACK_...81528A13B6...
PASS: first boot: generated seed, keyset 00df80cdec833d8a ≠ demo
PASS: /v1/keys serves keyset 00df80cdec833d8a
PASS: reset: same keyset 00df80cdec833d8a after reload
PASS: /v1/keys byte-identical across reset
PASS: populated snapshot restored (1 mint quote, 1 spent); fresh seed generated
PASS: erase: fresh keyset 005e2817db5190e6 (≠ 00df80cdec833d8a, ≠ demo)
ALL PASS: NVS keyset battery green (ids: 00df80cdec833d8a -> 00df80cdec833d8a -> 005e2817db5190e6)
```

What each phase proves:

- **Phase 1** — first-boot seed generation from `esp_fill_random` after
  WiFi association; the served keyset is never the demo keyset
  (`0022e025867793d1`), construction-level + observed.
- **Phase 2** — seed persists across reset; the re-derived keyset id is
  identical; `/v1/keys` is byte-identical (deterministic re-derivation).
- **Phase 2b** — a crafted `MintStateSnapshot` (1 ISSUED mint quote, 1
  spent Y) written into the NVS partition via `nvs_partition_gen.py` +
  `esptool write_flash` boots with `mint state restored: 1 mint quotes,
  0 melt quotes, 1 spent, 0 issued outputs` — the populated-snapshot
  acceptance leg of #60. The injected image carries no seed, so the
  fresh-generation path runs in the same boot (state restored + new
  identity — the two stores are independent keys, as designed).
- **Phase 3** — `erase_region 0x9000 0x10000` forces a fresh identity
  (regeneration path).

## Deferred (with reason)

- **Watchdog-reset mid-mutation** (a #60 acceptance clause): no mutating
  RPC exists on the device yet (POST routes are 501 stubs), so there is
  no operation to interrupt. Mutation durability is host-proven by the
  `StateStore` restart matrix (`tests/persistence.rs` — spent/quote/
  restore/melt-rollback invariants across store instances); NVS per-key
  atomicity (`set_blob` = erase+write+commit) is the ESP-IDF guarantee
  the store rides. Revisit when POST routes land.
