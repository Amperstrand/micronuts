# host-mint-tool

`host-mint-tool` is a demo mint-signer CLI for the Micronuts hardware wallet.
It talks to the device over USB serial (CDC) and sends the same binary frames
used by the host-side demo protocol.

## Build / install

```bash
cargo build -p host-mint-tool --release
```

Binary name:

```bash
target/release/mint-tool
```

Run from the workspace with:

```bash
cargo run -p host-mint-tool --bin mint-tool -- --help
```

> Note: the `serialport` dependency may not build or run cleanly on macOS in
> some environments. This document is based on source analysis.

## Usage

```bash
mint-tool [OPTIONS] <COMMAND>
```

Global options:

- `-p, --port <PATH>`: serial device path (required for device commands)
- `-b, --baud <BAUD>`: baud rate, default `115200`

## Commands

### `list`

Lists available USB serial devices.

```bash
mint-tool list
```

Example:

```bash
mint-tool list
```

### `generate`

Generates a test token and imports it to the device.

Options:

- `-a, --amount <AMOUNT>`: token amount in sats, default `1000`

Example:

```bash
mint-tool --port /dev/ttyACM0 generate --amount 1000
```

### `blind`

Requests the device to generate blinded outputs.

```bash
mint-tool --port /dev/ttyACM0 blind
```

### `sign`

Requests blinded outputs, signs them with the demo mint, and sends the
signatures back to the device.

```bash
mint-tool --port /dev/ttyACM0 sign
```

### `export`

Requests proofs from the device and prints a Cashu token (`cashuB...`).

```bash
mint-tool --port /dev/ttyACM0 export
```

### `monitor`

Continuously prints received frames until interrupted.

```bash
mint-tool --port /dev/ttyACM0 monitor
```

### `scanner-status`

Queries scanner state and prints connection/initialization status.

```bash
mint-tool --port /dev/ttyACM0 scanner-status
```

### `scan`

Triggers the scanner, waits for QR data, and prints the decoded payload.

```bash
mint-tool --port /dev/ttyACM0 scan
```

## Connection setup

- Use a USB CDC serial device path (for example `/dev/ttyACM0` or similar).
- `list` only returns USB serial devices detected by `serialport`.
- Default baud rate is `115200`.
- The tool disables DTR and RTS after opening the port to avoid resetting the
  STM32 board.
- Serial reads use a 5 second timeout.

## Troubleshooting

- **`--port is required for this command`**: pass `--port <device>` for every
  command except `list`.
- **No device in `list` output**: confirm the board is exposed as USB CDC and
  that your OS created a serial device node.
- **Open/read/write errors**: verify the path, permissions, and baud rate.
- **Unexpected reset on connect**: the CLI already clears DTR/RTS, so repeated
  resets usually indicate a cabling or driver issue.
- **No scan data**: make sure the scanner is pointed at a QR code after
  running `scan`.

## Protocol notes

- `list` filters for USB serial devices only.
- `generate` sends an import-token frame.
- `blind`, `sign`, and `export` exchange binary request/response frames.
- `monitor` prints raw frame command IDs and payload lengths.
- `scanner-status` expects a payload with connection, initialization, and model
  fields.
- `scan` first triggers the scanner, then polls for scan data.
