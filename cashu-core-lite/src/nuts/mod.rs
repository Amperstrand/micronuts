//! Cashu NUT protocol types.
//!
//! Each submodule corresponds to a Cashu NUT specification document.
//! These types mirror the request/response shapes from the NUTs and are
//! designed to be transport-neutral (no HTTP, no USB — just data).

pub mod nut00;
pub mod nut01;
pub mod nut02;
pub mod nut03;
pub mod nut04;
pub mod nut05;
pub mod nut06;
pub mod nut07;
pub mod nut13;
