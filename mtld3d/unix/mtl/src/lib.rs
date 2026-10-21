//! MacNeutron: the subset of objc2's bindings that mtld3d's Metal layer uses, under objc2's own names, over
//! winemetal's Unix-call table (DXMT's `winemetal.so`, which the VM's guest forwards to the Mac's Metal).
//!
//! mtld3d-unix reaches Metal through `crate::mtl`, which re-exports objc2's crates on the product macOS build and
//! these modules under its `winemetal` feature (the Linux build always): the Metal layer's code is upstream's.

#![allow(non_snake_case)]

mod meta;
pub mod table;
mod wmt;

pub mod block2;
pub mod objc2;
pub mod metal;
pub mod metal_fx;
pub mod quartz_core;
pub mod foundation;

/// objc2-metal's names.
pub mod objc2_metal {
    pub use crate::metal::*;
}
/// objc2-metal-fx's names.
pub mod objc2_metal_fx {
    pub use crate::metal_fx::*;
}
/// objc2-quartz-core's names.
pub mod objc2_quartz_core {
    pub use crate::quartz_core::*;
}
/// objc2-foundation's names.
pub mod objc2_foundation {
    pub use crate::foundation::*;
}

/// winemetal's slots the facade calls, with their function names (tests check the table against them).
pub use wmt::SLOTS;
