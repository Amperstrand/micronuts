# Hardware Test Results — 2026-09-03

**Hardware**: STM32F469I-Discovery, ST-LINK/V2.1 (`/dev/ttyACM1`) + USB OTG FS cable on CN5 (CDC `16c0:27dd`)
**Firmware**: main @ 2053c74 (release, default build with defmt-log)
**Driver**: `scripts/test_hw_swap_gate.sh` (3 consecutive greens + 1 post-clippy green)

## What was verified on silicon

| Check | Result | Notes |
|---|---|---|
| Boot: SDRAM/heap/display/touch | ✅ | RTT log, no panics |
| Self-test battery | ✅ 7/9 | same 2 skips as 2026-07-30 (touch, scan — need hands) |
| Self-test: crypto blinding | ✅ | hash_to_curve/blind/sign round-trip on the 180 MHz M4F |
| Self-test: FrameDecoder round-trip | ✅ | exercises the chunking-invariance fix (ae78814) in-memory |
| USB CDC enumeration | ✅ | cable connected; ~18 s splash timeout must elapse first |
| Swap flow generate→sign→export | ✅ | device DLEQ-verifies the demo mint (#54 machinery) |
| Gate verification of device export | ✅ | 21 sats Open, replay rejected — string-convention secrets (d90c8dd) + DLEQ + V4 encoding end to end |
| Garbage flood (1030 B) | ✅ | device still answers after (#55 W2 class) |
| Resync-within-chunk on the wire | ✅ | bad header + valid frame in ONE write → answered (ae78814) |
| Split-write frame | ✅ | decodes across USB write boundaries |

## Findings fixed this session

- `host-mint-tool export` printed tokens with STANDARD base64 instead of
  the cashuB wire alphabet (base64URL, no padding) — its output had never
  been parseable by a conformant wallet. Found by round-tripping the
  device export through the offline gate.

## Operational notes

- **USB enumeration waits for the boot splash timeout** (~60 s wall time,
  measured 2026-09-03 after the compressed-C reflash: 540 SDRAM-rendered
  frames cost far more than their 33 ms tick). No touch needed — the
  STATUS-AND-TEST-PLAN note "needs touch" predates the timeout.
- Earlier in the evening the CDC was absent (cable not yet connected):
  zero USB bus events, not a firmware issue. #34/#37 (PHY reset fixes)
  are in this build and enumeration worked once the cable was attached.
- RTT (`probe-rs attach --probe 0483:374b <elf>`) is the remote console:
  attach dumps the defmt ring without disturbing USB (reads are brief
  halts; the device is idle at attach time anyway).
