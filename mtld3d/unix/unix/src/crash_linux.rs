//! The crash handler on Linux (the VM's guest, D3D9 M2b): none.
//!
//! `crash.rs` reads Mach thread state, `dyld`'s images and macOS's `ucontext`; in the guest a fault
//! goes to Wine's handler and FEX's as any other library's does, and Wine's own report names the
//! faulting module. ponytail: no Linux report of our own; one comes when a guest crash needs more
//! than Wine's.

/// Nothing to install on Linux.
pub const fn install() {}
