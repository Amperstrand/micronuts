#![no_std]

extern crate alloc;

#[cfg(feature = "defmt")]
#[macro_export]
macro_rules! fw_info { ($($arg:tt)*) => { defmt::info!($($arg)*) } }

#[cfg(not(feature = "defmt"))]
#[macro_export]
macro_rules! fw_info { ($($arg:tt)*) => { {} } }

#[cfg(feature = "defmt")]
#[macro_export]
macro_rules! fw_warn { ($($arg:tt)*) => { defmt::warn!($($arg)*) } }

#[cfg(not(feature = "defmt"))]
#[macro_export]
macro_rules! fw_warn { ($($arg:tt)*) => { {} } }

#[cfg(feature = "defmt")]
#[macro_export]
macro_rules! fw_error { ($($arg:tt)*) => { defmt::error!($($arg)*) } }

#[cfg(not(feature = "defmt"))]
#[macro_export]
macro_rules! fw_error { ($($arg:tt)*) => { {} } }

pub mod boot_splash;
pub mod boot_splash_assets;
pub mod build_info;
pub mod hardware_impl;
pub mod qr;
pub mod self_test;
