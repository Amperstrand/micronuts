#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_probe as _;

#[entry]
fn main() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}
