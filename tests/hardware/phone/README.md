# Phone HIL — micronuts side (T5)

`tests/hardware/phone/` — the micronuts half of the phone-vs-device
cashu HIL (companion suites live in Amperstrand/nucula
`tests/hil/phone/`; design doc there: `docs/HIL-PHONE-DESIGN.md`).
This folds `docs/PHONE-BRINGUP-RUNBOOK.md`'s "real suite" next-step
into code.

## Target

m5stick (ESP32 + MFRC522, labgrid place `m5stick`) running
`micronuts-esp32-wallet` built with feature `nfc-mfrc522` (PR #108).

## Running

```bash
python3 -m pytest tests/hardware/phone/ -v --hil \
    --timeout=600 --timeout-method=signal --junitxml=hil-junit.xml
# full money leg needs the nucula rig payer checkout:
HIL_NUCULA_ROOT=/path/to/nucula python3 -m pytest ... -v --hil
```

## Shared-board policy (the reason this suite skips)

The m5stick is bench-shared. When it serves another project's image
(the ccid GemPC card matrix as of 2026-10-21), the banner fixture
times out and the suite skips naming that — restoring the wallet image
over a live matrix is owner-scheduled via
`/root/src/worktrees/RESTORE-ECASH-RUNBOOK-v2-20261021.md`, never
opportunistic. When the board is back on wallet firmware (image
`/root/hil-stm32-redo/m5stick-wallet-backup-20261019.bin` + app
`/root/src/worktrees/mn-app4.bin`), the suite runs for real:

- console banner + `help` (proves wallet fw + `nfc` verb present)
- `heap` / `balance` health
- `nfc` verb: phone HCE on the coil → `nfc: tag ATQA=… UID=…`
  (tap detection; `no tag in field` = geometry skip)
- `receive <rig token>` → balance grows (money leg)

M5Stick flash rules (when a reflash IS scheduled): 115200 baud,
explicit port `/dev/serial/by-id/usb-Hades2001_M5stack_49D6163EBE-if00-port0`
(ttyUSB0 is the CYD — silent wrong-board damage),
`IDF_TARGET=esp32 SDKCONFIG=build-m5stick/sdkconfig
SDKCONFIG_DEFAULTS='sdkconfig.defaults;sdkconfig.defaults.m5stick'`,
ESP-IDF v5.5.5 from `/root/esp/esp-idf-v5.5.5` (the other copy has a
broken toolchain PATH).
