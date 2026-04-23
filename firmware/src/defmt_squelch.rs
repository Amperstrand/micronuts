//! No-op defmt transport stubs for `uart-log` mode.
//!
//! Embassy and gm65-scanner unconditionally depend on `defmt` and emit `defmt::*!`
//! macro calls. When the firmware-level `defmt` feature is not enabled, these calls
//! expand to no-ops at the macro level — but the linker still requires the transport
//! symbols (`_defmt_acquire`, `_defmt_release`, `_defmt_write`, `_defmt_timestamp`,
//! `_defmt_panic`). This module provides discard implementations so the firmware links
//! without `defmt-rtt`.
//!
//! When `defmt-log` IS enabled, `defmt-rtt` provides the real implementations and this
//! module is not compiled.

#![allow(private_interfaces)]

use core::marker::PhantomData;

#[derive(Copy, Clone)]
pub(crate) struct Formatter<'a> {
    _phantom: PhantomData<&'a ()>,
}

#[no_mangle]
pub unsafe extern "Rust" fn _defmt_acquire() {}

#[no_mangle]
pub unsafe extern "Rust" fn _defmt_release() {}

#[no_mangle]
pub unsafe extern "Rust" fn _defmt_write(_bytes: &[u8]) {}

#[no_mangle]
pub unsafe extern "Rust" fn _defmt_timestamp(_f: Formatter<'_>) {}

#[no_mangle]
pub unsafe extern "Rust" fn _defmt_panic() -> ! {
    panic!("defmt::panic!() reached in uart-log mode")
}
