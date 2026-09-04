# Micronuts Demo Flow

## First Vertical Slice

### Demo Scenario

1. Import a Cashu V4 token to the device
2. View token details on the display
3. Swap proofs through demo mint (re-blinding)
4. Export new proofs

### Prerequisites

- STM32F469I-Discovery connected via the USB OTG FS cable (CN5, micro-B)
- `arm-none-eabi-objcopy` + `st-flash` installed
- Rust toolchain with `thumbv7em-none-eabihf` target

### Step-by-Step

#### 1. Flash Firmware

```bash
cd firmware && cargo build --release
arm-none-eabi-objcopy -O binary target/thumbv7em-none-eabihf/release/firmware firmware.bin
st-flash --connect-under-reset write firmware.bin 0x08000000
st-flash --connect-under-reset reset
```

Then wait **~60–100 s** for the boot splash to finish before the CDC
device (VID:PID `16c0:27dd`) enumerates — detect the port by VID:PID,
never by tty number. The verified full battery lives in
`scripts/test_hw_swap_gate.sh`; see
[HARDWARE-TEST-RESULTS-20260903.md](HARDWARE-TEST-RESULTS-20260903.md).

#### 2. Start Host Tool

```bash
cargo run -p host-mint-tool -- /dev/tty.usbmodem*
```

#### 3. Import Token

```
> import cashuBp2V4...
```

Or generate test token:
```
> generate --amount 1000
```

#### 4. Request Blinded Outputs

```
> blind
```

Device generates secrets and blinded outputs.

#### 5. Sign

```
> sign
```

Host signs blinded outputs with demo key.

#### 6. Export

```
> export
```

Device unblinds signatures and exports new token.

### Success Criteria

- [ ] Token decodes on device
- [ ] Display shows mint/unit/amount/proof count
- [ ] Blinded outputs generated correctly
- [ ] Blind signatures received and unblinded
- [ ] New token encodes correctly
- [ ] New token verifies with reference impl

### Debugging

- **USB issues**: `dmesg | tail`
- **Crypto issues**: Compare with `cashu-rs` output
- **Display issues**: Run BSP examples first
- **Memory issues**: Enable SDRAM for large allocs
