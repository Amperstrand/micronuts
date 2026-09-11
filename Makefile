# Micronuts bench/HIL entry points (host side). The sanctioned cargo
# battery lives in AGENTS.md; these are the QR-rig gates.
.PHONY: hil-place test-qr-scanin

# (Re)create the micronuts-qr-rig labgrid place after coordinator restarts.
hil-place:
	bash tools/hil/labgrid-place.sh

# QR scan-in feasibility gate: BenchLock -> place -> backup/flash/restore,
# scanner status, single-frame roundtrip, UR token scan-in (see
# tools/hil/bringup.py). --keep skips the image restore for iteration.
test-qr-scanin:
	python3 tools/hil/bringup.py
