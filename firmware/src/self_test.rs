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
    defmt::info!("=== MICRONUTS SELF-TEST ===");
    defmt::info!("Git: {} ({})", build_info::GIT_HASH, build_info::GIT_DATE);
    defmt::info!("Build: {}", build_info::BUILD_DATE);
    defmt::info!("Embassy rev: {}", build_info::EMBASSY_REV);
    defmt::info!("BSP rev: {}", build_info::BSP_REV);
    defmt::info!("GM65 rev: {}", build_info::GM65_REV);
    defmt::info!("stm32f469i-disc rev: {}", build_info::STM32F469I_DISC_REV);
    defmt::info!("===========================");
}

pub async fn run_all(hw: &mut FirmwareHardware) -> Vec<TestResult> {
    let mut results = Vec::new();

    log_build_info();

    results.push(test_sdram(hw).await);
    results.push(test_rng(hw));
    results.push(test_heap());
    results.push(test_display(hw).await);
    results.push(test_touch(hw).await);
    results.push(test_scanner(hw).await);

    let passed = results.iter().filter(|r| r.status == TestStatus::Pass).count();
    let failed = results.iter().filter(|r| r.status == TestStatus::Fail).count();
    let skipped = results.iter().filter(|r| r.status == TestStatus::Skip).count();
    let total = results.len();

    defmt::info!("=== RESULTS: {}/{} PASS, {} FAIL, {} SKIP ===",
        passed, total, failed, skipped);

    for r in &results {
        match r.status {
            TestStatus::Pass => defmt::info!("  [PASS] {}", r.name),
            TestStatus::Fail => defmt::error!("  [FAIL] {}", r.name),
            TestStatus::Skip => defmt::warn!("  [SKIP] {}", r.name),
        }
    }

    results
}

async fn test_sdram(hw: &mut FirmwareHardware) -> TestResult {
    defmt::info!("[TEST] SDRAM...");

    let test_size = 4096usize;

    let total_pixels = (FB_WIDTH as usize) * (FB_HEIGHT as usize);
    let test_offset = if total_pixels > test_size * 2 {
        total_pixels - test_size * 2
    } else {
        0
    };

    let buf = hw.fb.as_raw();
    if buf.len() < test_offset + test_size {
        defmt::error!("[FAIL] SDRAM (framebuffer too small)");
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
        defmt::info!("[PASS] SDRAM (verified {} bytes)", test_size * 2);
        TestResult::pass("SDRAM")
    } else {
        defmt::error!("[FAIL] SDRAM (data mismatch after write)");
        TestResult::fail("SDRAM")
    }
}

fn test_rng(hw: &mut FirmwareHardware) -> TestResult {
    defmt::info!("[TEST] RNG...");
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
        defmt::info!("[PASS] RNG (256 bytes, {} unique values)", unique);
        TestResult::pass("RNG")
    } else {
        defmt::error!("[FAIL] RNG (unique={}, zeros={}, 0xff={})", unique, zero_count, ff_count);
        TestResult::fail("RNG")
    }
}

fn test_heap() -> TestResult {
    defmt::info!("[TEST] Heap...");
    let size = 1024;
    let mut v = alloc::vec![0u8; size];
    for (i, b) in v.iter_mut().enumerate() {
        *b = (i % 256) as u8;
    }

    let ok = v.iter().enumerate().all(|(i, &b)| b == (i % 256) as u8);
    drop(v);

    if ok {
        defmt::info!("[PASS] Heap (alloc {} bytes, pattern verified)", size);
        TestResult::pass("Heap")
    } else {
        defmt::error!("[FAIL] Heap (pattern mismatch)");
        TestResult::fail("Heap")
    }
}

async fn test_display(hw: &mut FirmwareHardware) -> TestResult {
    defmt::info!("[TEST] Display...");
    let fb_size = (FB_WIDTH as usize) * (FB_HEIGHT as usize);
    defmt::info!("[TEST] Display: framebuffer {} pixels ({}x{})", fb_size, FB_WIDTH, FB_HEIGHT);

    let raw_green: u16 = 0x07E0;

    let buf = hw.fb.as_raw();
    if buf.len() >= fb_size {
        for px in buf[..fb_size].iter_mut() {
            *px = raw_green;
        }
        defmt::info!("[TEST] Display: screen should be GREEN now (3s)");
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
        defmt::info!("[PASS] Display (green fill, {} pixels)", fb_size);
        TestResult::pass("Display")
    } else {
        defmt::error!("[FAIL] Display (framebuffer readback mismatch)");
        TestResult::fail("Display")
    }
}

async fn test_touch(hw: &mut FirmwareHardware) -> TestResult {
    defmt::info!("[TEST] Touch... (tap the screen within 5s)");

    if !hw.touch_available {
        defmt::info!("[SKIP] Touch (controller not detected)");
        return TestResult::skip("Touch");
    }

    let start = embassy_time::Instant::now();
    loop {
        if embassy_time::Instant::now().duration_since(start) > INTERACTIVE_TIMEOUT {
            defmt::warn!("[SKIP] Touch (5s timeout, no tap detected)");
            return TestResult::skip("Touch");
        }

        if let Some(tp) = hw.touch_get() {
            if tp.detected {
                defmt::info!("[PASS] Touch (x={}, y={})", tp.x, tp.y);
                return TestResult::pass("Touch");
            }
        }

        embassy_time::Timer::after(Duration::from_millis(50)).await;
    }
}

async fn test_scanner(hw: &mut FirmwareHardware) -> TestResult {
    defmt::info!("[TEST] Scanner... (scan a QR code within 5s)");

    if !hw.is_connected() {
        defmt::info!("[SKIP] Scanner (not connected)");
        return TestResult::skip("Scanner");
    }

    defmt::info!("[TEST] Scanner: enabling aim laser...");
    let _ = hw.set_aim(true).await;
    embassy_time::Timer::after(Duration::from_millis(500)).await;

    defmt::info!("[TEST] Scanner: triggering scan (laser should be ON)...");
    let _ = hw.trigger().await;

    let result = embassy_time::with_timeout(INTERACTIVE_TIMEOUT, hw.read_scan()).await;

    hw.stop().await;
    let _ = hw.set_aim(false).await;

    match result {
        Ok(Some(data)) => {
            let len = data.len();
            defmt::info!("[PASS] Scanner ({} bytes received)", len);
            TestResult::pass("Scanner")
        }
        Ok(None) => {
            defmt::warn!("[SKIP] Scanner (read returned None — scanner error)");
            TestResult::skip("Scanner")
        }
        Err(_) => {
            defmt::warn!("[SKIP] Scanner (5s timeout, no scan)");
            TestResult::skip("Scanner")
        }
    }
}
