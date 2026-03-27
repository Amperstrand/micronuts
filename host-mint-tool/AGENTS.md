# host-mint-tool

Demo mint signer CLI for host PC. Communicates with the STM32 firmware via USB CDC.

## Build

```bash
cargo build --release
```

## Usage

```bash
# List available serial ports
cargo run --release -- --list

# Connect to device
cargo run --release -- --port /dev/ttyACM0

# Generate test token
cargo run --release -- --generate-token
```

## Protocol

Uses the same USB CDC binary protocol as the firmware. See `firmware/AGENTS.md` for the protocol table.

## Dependencies

- `serialport` — Cross-platform serial communication
- `hex` — Hex encoding/decoding

## Upstream Interaction Policy

**NEVER file PRs or issues on upstream projects (embassy-rs, stm32-rs, etc.) without human review and approval.** AI-generated bug diagnoses can be confidently wrong. Document findings in Amperstrand repos first and let a human decide whether to escalate.
