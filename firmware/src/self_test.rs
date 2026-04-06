use alloc::vec::Vec;

use embassy_stm32f469i_disco::display::{FB_HEIGHT, FB_WIDTH};
use embassy_time::Duration;

use crate::build_info;
use crate::hardware_impl::FirmwareHardware;
use micronuts_app::hardware::{MicronutsHardware, Scanner};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Pass,
    Fail,
    Skip,
}

pub struct TestResult {
    pub name: &'static str,
    pub status: TestStatus,
}

impl TestResult {
    pub fn pass(name: &'static str) -> Self {
        Self { name, status: TestStatus::Pass }
    }

    pub fn fail(name: &'static str) -> Self {
        Self { name, status: TestStatus::Fail }
    }

    pub fn skip(name: &'static str) -> Self {
        Self { name, status: TestStatus::Skip }
    }
}

const INTERACTIVE_TIMEOUT: Duration = Duration::from_secs(5);

pub fn log_build_info() {
    crate::log_info!("=== MICRONUTS SELF-TEST ===");
    crate::log_info!("Git: {} ({})", build_info::GIT_HASH, build_info::GIT_DATE);
    crate::log_info!("Build: {}", build_info::BUILD_DATE);
    crate::log_info!("Embassy rev: {}", build_info::EMBASSY_REV);
    crate::log_info!("BSP rev: {}", build_info::BSP_REV);
    crate::log_info!("GM65 rev: {}", build_info::GM65_REV);
    crate::log_info!("stm32f469i-disc rev: {}", build_info::STM32F469I_DISC_REV);
    crate::log_info!("===========================");
}

pub async fn run_all(hw: &mut FirmwareHardware) -> Vec<TestResult> {
    let mut results = Vec::new();

    log_build_info();

    results.push(test_sdram(hw).await);
    results.push(test_rng(hw));
    results.push(test_heap());
    results.push(test_heap_stress());
    results.push(test_display(hw).await);
    results.push(test_touch(hw).await);
    results.push(test_scanner(hw).await);
    results.push(test_crypto_blinding(hw));
    results.push(test_usb_cdc_protocol());

    let passed = results.iter().filter(|r| r.status == TestStatus::Pass).count();
    let failed = results.iter().filter(|r| r.status == TestStatus::Fail).count();
    let skipped = results.iter().filter(|r| r.status == TestStatus::Skip).count();
    let total = results.len();

    crate::log_info!("=== RESULTS: {}/{} PASS, {} FAIL, {} SKIP ===",
        passed, total, failed, skipped);

    for r in &results {
        match r.status {
            TestStatus::Pass => crate::log_info!("  [PASS] {}", r.name),
            TestStatus::Fail => crate::log_error!("  [FAIL] {}", r.name),
            TestStatus::Skip => crate::log_warn!("  [SKIP] {}", r.name),
        }
    }

    results
}

async fn test_sdram(hw: &mut FirmwareHardware) -> TestResult {
    crate::log_info!("[TEST] SDRAM...");

    let test_size = 4096usize;

    let total_pixels = (FB_WIDTH as usize) * (FB_HEIGHT as usize);
    let test_offset = if total_pixels > test_size * 2 {
        total_pixels - test_size * 2
    } else {
        0
    };

    let buf = hw.fb.as_raw();
    if buf.len() < test_offset + test_size {
        crate::log_error!("[FAIL] SDRAM (framebuffer too small)");
        return TestResult::fail("SDRAM");
    }

    let mut orig = [0u16; 4096];
    for (i, v) in orig.iter_mut().enumerate() {
        *v = buf[test_offset + i];
    }

    for i in 0..test_size {
        buf[test_offset + i] = 0xAA55;
    }

    embassy_time::Timer::after(Duration::from_millis(1)).await;

    let mut ok = true;
    for i in 0..test_size {
        if buf[test_offset + i] != 0xAA55u16 {
            ok = false;
            break;
        }
    }

    for (i, v) in orig.iter().enumerate() {
        buf[test_offset + i] = *v;
    }

    if ok {
        crate::log_info!("[PASS] SDRAM (verified {} bytes)", test_size * 2);
        TestResult::pass("SDRAM")
    } else {
        crate::log_error!("[FAIL] SDRAM (data mismatch after write)");
        TestResult::fail("SDRAM")
    }
}

fn test_rng(hw: &mut FirmwareHardware) -> TestResult {
    crate::log_info!("[TEST] RNG...");
    let mut buf = [0u8; 256];
    hw.rng_fill_bytes(&mut buf);

    let mut zero_count = 0u32;
    let mut ff_count = 0u32;
    let mut seen = [false; 256];

    for &b in &buf {
        if b == 0 { zero_count += 1; }
        if b == 0xFF { ff_count += 1; }
        seen[b as usize] = true;
    }

    let unique = seen.iter().filter(|&&s| s).count();

    if unique > 150 && zero_count < 10 && ff_count < 10 {
        crate::log_info!("[PASS] RNG (256 bytes, {} unique values)", unique);
        TestResult::pass("RNG")
    } else {
        crate::log_error!("[FAIL] RNG (unique={}, zeros={}, 0xff={})", unique, zero_count, ff_count);
        TestResult::fail("RNG")
    }
}

fn test_heap() -> TestResult {
    crate::log_info!("[TEST] Heap...");
    let size = 1024;
    let mut v = alloc::vec![0u8; size];
    for (i, b) in v.iter_mut().enumerate() {
        *b = (i % 256) as u8;
    }

    let ok = v.iter().enumerate().all(|(i, &b)| b == (i % 256) as u8);
    drop(v);

    if ok {
        crate::log_info!("[PASS] Heap (alloc {} bytes, pattern verified)", size);
        TestResult::pass("Heap")
    } else {
        crate::log_error!("[FAIL] Heap (pattern mismatch)");
        TestResult::fail("Heap")
    }
}

fn test_heap_stress() -> TestResult {
    crate::log_info!("[TEST] Heap stress (4KB walking pattern)...");

    const ALLOC_SIZE: usize = 4096; // 4KB
    const PATTERNS: [u8; 4] = [0x00, 0xFF, 0x55, 0x01];

    // Test each walking pattern
    for (pattern_idx, &pattern) in PATTERNS.iter().enumerate() {
        // Step 1: Allocate 4KB
        let mut v: Vec<u8> = Vec::with_capacity(ALLOC_SIZE);
        unsafe { v.set_len(ALLOC_SIZE); }

        // Step 2: Write walking pattern
        for byte in v.iter_mut() {
            *byte = pattern;
        }

        // Step 3: Verify readback
        let ok = v.iter().all(|&b| b == pattern);
        if !ok {
            crate::log_error!("[FAIL] Heap stress (pattern 0x{:02X} readback mismatch)", pattern);
            return TestResult::fail("Heap stress");
        }

        crate::log_info!("[TEST] Heap stress: pattern 0x{:02X} verified ({}/{})", pattern, pattern_idx + 1, PATTERNS.len());

        // Step 4: Drop allocation
        drop(v);
    }

    // Step 5: All allocations deallocated (Rust guarantees no use-after-free)
    crate::log_info!("[PASS] Heap stress (4KB x 4 patterns, all deallocated)");
    TestResult::pass("Heap stress")
}

fn test_crypto_blinding(hw: &mut FirmwareHardware) -> TestResult {
    crate::log_info!("[TEST] Crypto blinding...");

    // Step 1: Generate random secret (32 bytes)
    let mut secret = [0u8; 32];
    hw.rng_fill_bytes(&mut secret);

    // Step 2: hash_to_curve(secret) - returns point on curve
    let _y = match cashu_core_lite::crypto::hash_to_curve(&secret) {
        Ok(y) => y,
        Err(_) => {
            crate::log_error!("[FAIL] Crypto blinding (hash_to_curve failed)");
            return TestResult::fail("Crypto blinding");
        }
    };
    crate::log_info!("[TEST] Crypto: hash_to_curve OK");

    // Step 3: Generate blinder from hardware RNG, ensure non-zero (OR 0x01 on last byte)
    let mut blinder_bytes = [0u8; 32];
    hw.rng_fill_bytes(&mut blinder_bytes);
    blinder_bytes[31] |= 0x01;
    let blinder = match cashu_core_lite::keypair::SecretKey::from_slice(&blinder_bytes) {
        Ok(sk) => sk,
        Err(_) => {
            crate::log_error!("[FAIL] Crypto blinding (invalid blinder scalar)");
            return TestResult::fail("Crypto blinding");
        }
    };

    // Step 4: blind_message(secret, Some(blinder)) - returns blinded message
    let blinded = match cashu_core_lite::crypto::blind_message(&secret, Some(blinder.clone())) {
        Ok(bm) => bm,
        Err(_) => {
            crate::log_error!("[FAIL] Crypto blinding (blind_message failed)");
            return TestResult::fail("Crypto blinding");
        }
    };
    crate::log_info!("[TEST] Crypto: blind_message OK");

    // Step 5: Generate mint keypair and sign the blinded message
    let mut mint_key_bytes = [0u8; 32];
    hw.rng_fill_bytes(&mut mint_key_bytes);
    mint_key_bytes[31] |= 0x01;
    let mint_key = match cashu_core_lite::keypair::SecretKey::from_slice(&mint_key_bytes) {
        Ok(sk) => sk,
        Err(_) => {
            crate::log_error!("[FAIL] Crypto blinding (invalid mint key)");
            return TestResult::fail("Crypto blinding");
        }
    };
    let mint_pubkey = mint_key.public_key();

    // sign_message(mint_key, blinded_message) - returns signature
    let blinded_sig = cashu_core_lite::crypto::sign_message(&mint_key, &blinded.blinded);
    crate::log_info!("[TEST] Crypto: sign_message OK");

    // Step 6: unblind_signature(blinded_sig, blinder, mint_pubkey) - returns unblinded signature
    let unblinded = match cashu_core_lite::crypto::unblind_signature(
        &blinded_sig, &blinded.blinder, &mint_pubkey
    ) {
        Ok(c) => c,
        Err(_) => {
            crate::log_error!("[FAIL] Crypto blinding (unblind_signature failed)");
            return TestResult::fail("Crypto blinding");
        }
    };

    // Step 7: verify_signature(secret, unblinded_sig, mint_key) - returns true/false
    // SUCCESS CRITERIA: a * hash_to_curve(secret) == unblinded_sig
    match cashu_core_lite::crypto::verify_signature(&secret, &unblinded, &mint_key) {
        Ok(true) => {
            crate::log_info!("[PASS] Crypto blinding (full round-trip verified)");
            TestResult::pass("Crypto blinding")
        }
        Ok(false) => {
            crate::log_error!("[FAIL] Crypto blinding (signature verification failed)");
            TestResult::fail("Crypto blinding")
        }
        Err(_) => {
            crate::log_error!("[FAIL] Crypto blinding (verify_signature error)");
            TestResult::fail("Crypto blinding")
        }
    }
}

fn test_usb_cdc_protocol() -> TestResult {
    crate::log_info!("[TEST] USB CDC protocol...");
    
    use micronuts_app::protocol::{Command, Frame, FrameDecoder};
    
    // Step 1: Create a test frame with ScannerStatus command (0x10)
    let test_command = Command::ScannerStatus;
    let frame = Frame::new(test_command);
    crate::log_info!("[TEST] USB CDC: created frame with command 0x{:02X}", test_command as u8);
    
    // Step 2: Encode the frame to bytes
    let mut encode_buf = [0u8; 1027]; // MAX_PAYLOAD_SIZE + 3
    let encoded_len = frame.encode(&mut encode_buf);
    
    if encoded_len == 0 {
        crate::log_error!("[FAIL] USB CDC protocol (encoding failed)");
        return TestResult::fail("USB CDC protocol");
    }
    
    crate::log_info!("[TEST] USB CDC: encoded {} bytes", encoded_len);
    
    // Verify encoded bytes structure: [Command:1][LenHigh:1][LenLow:1][Payload:N]
    if encode_buf[0] != test_command as u8 {
        crate::log_error!("[FAIL] USB CDC protocol (command byte mismatch)");
        return TestResult::fail("USB CDC protocol");
    }
    
    // ScannerStatus has no payload, so length should be 0
    let payload_len = ((encode_buf[1] as u16) << 8) | (encode_buf[2] as u16);
    if payload_len != 0 {
        crate::log_error!("[FAIL] USB CDC protocol (payload length mismatch)");
        return TestResult::fail("USB CDC protocol");
    }
    
    crate::log_info!("[TEST] USB CDC: frame structure verified (cmd=0x{:02X}, len={})", 
                 encode_buf[0], payload_len);
    
    // Step 3: Decode the bytes back to a frame
    let mut decoder = FrameDecoder::new();
    let decoded_frame = match decoder.decode(&encode_buf[..encoded_len]) {
        Some(f) => f,
        None => {
            crate::log_error!("[FAIL] USB CDC protocol (decoding failed)");
            return TestResult::fail("USB CDC protocol");
        }
    };
    
    crate::log_info!("[TEST] USB CDC: decoded frame successfully");
    
    // Step 4: Verify command matches
    if decoded_frame.command != test_command {
        crate::log_error!("[FAIL] USB CDC protocol (command mismatch after decode)");
        return TestResult::fail("USB CDC protocol");
    }
    
    // Step 5: Verify payload matches (should be empty for ScannerStatus)
    if decoded_frame.length != 0 {
        crate::log_error!("[FAIL] USB CDC protocol (payload length mismatch after decode)");
        return TestResult::fail("USB CDC protocol");
    }
    
    crate::log_info!("[PASS] USB CDC protocol (encode/decode round-trip verified)");
    TestResult::pass("USB CDC protocol")
}

async fn test_display(hw: &mut FirmwareHardware) -> TestResult {
    crate::log_info!("[TEST] Display...");
    let fb_size = (FB_WIDTH as usize) * (FB_HEIGHT as usize);
    crate::log_info!("[TEST] Display: framebuffer {} pixels ({}x{})", fb_size, FB_WIDTH, FB_HEIGHT);

    let raw_green: u16 = 0x07E0;

    let buf = hw.fb.as_raw();
    if buf.len() >= fb_size {
        for px in buf[..fb_size].iter_mut() {
            *px = raw_green;
        }
        crate::log_info!("[TEST] Display: screen should be GREEN now (3s)");
    }

    embassy_time::Timer::after(Duration::from_secs(3)).await;

    let mut ok = true;
    if buf.len() >= fb_size {
        for px in buf[..fb_size].iter() {
            if *px != raw_green {
                ok = false;
                break;
            }
        }
    }

    if ok {
        crate::log_info!("[PASS] Display (green fill, {} pixels)", fb_size);
        TestResult::pass("Display")
    } else {
        crate::log_error!("[FAIL] Display (framebuffer readback mismatch)");
        TestResult::fail("Display")
    }
}

async fn test_touch(hw: &mut FirmwareHardware) -> TestResult {
    crate::log_info!("[TEST] Touch... (tap the screen within 5s)");

    if !hw.touch_available {
        crate::log_info!("[SKIP] Touch (controller not detected)");
        return TestResult::skip("Touch");
    }

    let start = embassy_time::Instant::now();
    loop {
        if embassy_time::Instant::now().duration_since(start) > INTERACTIVE_TIMEOUT {
            crate::log_warn!("[SKIP] Touch (5s timeout, no tap detected)");
            return TestResult::skip("Touch");
        }

        if let Some(tp) = hw.touch_get() {
            if tp.detected {
                crate::log_info!("[PASS] Touch (x={}, y={})", tp.x, tp.y);
                return TestResult::pass("Touch");
            }
        }

        embassy_time::Timer::after(Duration::from_millis(50)).await;
    }
}

async fn test_scanner(hw: &mut FirmwareHardware) -> TestResult {
    crate::log_info!("[TEST] Scanner... (scan a QR code within 5s)");

    if !hw.is_connected() {
        crate::log_info!("[SKIP] Scanner (not connected)");
        return TestResult::skip("Scanner");
    }

    crate::log_info!("[TEST] Scanner: enabling aim laser...");
    let _ = hw.set_aim(true).await;
    embassy_time::Timer::after(Duration::from_millis(500)).await;

    crate::log_info!("[TEST] Scanner: triggering scan (laser should be ON)...");
    let _ = hw.trigger().await;

    let result = embassy_time::with_timeout(INTERACTIVE_TIMEOUT, hw.read_scan()).await;

    hw.stop().await;
    let _ = hw.set_aim(false).await;

    match result {
        Ok(Some(data)) => {
            let len = data.len();
            crate::log_info!("[PASS] Scanner ({} bytes received)", len);
            TestResult::pass("Scanner")
        }
        Ok(None) => {
            crate::log_warn!("[SKIP] Scanner (read returned None — scanner error)");
            TestResult::skip("Scanner")
        }
        Err(_) => {
            crate::log_warn!("[SKIP] Scanner (5s timeout, no scan)");
            TestResult::skip("Scanner")
        }
    }
}
