//! Micronuts Firmware
//!
//! Cashu hardware wallet POC for STM32F469I-Discovery

#![no_std]

pub mod usb;

// Re-export BSP types we need
pub use stm32f469i_disc::*;
