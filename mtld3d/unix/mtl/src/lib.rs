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

/// The VM guest's memory (D3D9 M2b): what a no-copy buffer there is made over, since the host maps it too.
pub mod memory {
    use crate::{table, wmt};

    /// `len` bytes from winemetal's allocator (slot 153), aligned to the guest's pages; null when it has none.
    #[must_use]
    pub fn alloc(len: usize) -> *mut u8 {
        let mut params = wmt::Memory { length: len as u64, ptr: core::ptr::null_mut() };
        // SAFETY: slot 153 writes the allocation's address, or null.
        let status = unsafe { table::call(wmt::MEMORY_ALLOC, &mut params) };
        if status == 0 { params.ptr.cast() } else { core::ptr::null_mut() }
    }

    /// Gives back memory [`alloc`] returned (slot 154); winemetal keeps it until no buffer is made over it.
    pub fn free(ptr: *mut u8) {
        let mut params = wmt::Memory { length: 0, ptr: ptr.cast() };
        // SAFETY: slot 154 on an allocation of slot 153's.
        unsafe { table::call(wmt::MEMORY_FREE, &mut params) };
    }
}
