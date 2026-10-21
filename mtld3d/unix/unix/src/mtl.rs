//! The one place the Metal layer's objc2 crates come from (D3D9 spec §5.1).
//!
//! The product macOS build re-exports objc2's own bindings; the `winemetal` feature (always on off
//! Apple targets) re-exports the `mtld3d-mtl` facade, the same names over winemetal's Unix-call
//! table.

#[cfg(not(mtl_facade))]
pub use ::{block2, objc2, objc2_foundation, objc2_metal, objc2_metal_fx, objc2_quartz_core};
#[cfg(mtl_facade)]
pub use mtld3d_mtl::{block2, objc2, objc2_foundation, objc2_metal, objc2_metal_fx, objc2_quartz_core};
#[cfg(mtl_facade)]
pub use mtld3d_mtl::table::mark_wow64;

/// A 32-bit process's first thunk (lib.rs): nothing to mark when Metal is objc2's own.
#[cfg(not(mtl_facade))]
pub const fn mark_wow64() {}
