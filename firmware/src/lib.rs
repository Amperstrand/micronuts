#![no_std]

extern crate alloc;

#[cfg(all(feature = "defmt-log", feature = "uart-log"))]
compile_error!("features `defmt-log` and `uart-log` are mutually exclusive");

#[cfg(not(any(feature = "defmt-log", feature = "uart-log")))]
compile_error!("enable exactly one logging feature: `defmt-log` or `uart-log`");

pub mod boot_splash;
pub mod boot_splash_assets;
pub mod build_info;
pub mod hardware_impl;
pub mod qr;
pub mod self_test;

#[cfg(feature = "uart-log")]
pub mod uart_log;

#[cfg(feature = "uart-log")]
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => ($crate::uart_log::write_log(format_args!($($arg)*)));
}

#[cfg(feature = "uart-log")]
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => ($crate::uart_log::write_log(format_args!("[WARN] {}", format_args!($($arg)*))));
}

#[cfg(feature = "uart-log")]
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => ($crate::uart_log::write_log(format_args!("[ERROR] {}", format_args!($($arg)*))));
}

#[cfg(not(feature = "uart-log"))]
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => (defmt::info!($($arg)*));
}

#[cfg(not(feature = "uart-log"))]
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => (defmt::warn!($($arg)*));
}

#[cfg(not(feature = "uart-log"))]
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => (defmt::error!($($arg)*));
}
