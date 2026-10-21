//! GPU capture under the winemetal facade: none.
//!
//! A capture is the host's (a trace path in the guest names nothing on the Mac), so the hotkey's
//! start and stop do nothing here.

use mtld3d_shared::{MetalHandle, mtl_handle::MTLDeviceKind};

/// No capture.
pub fn start_capture(_device_handle: MetalHandle<MTLDeviceKind>) {
    mtld3d_shared::log_once_warn!(target: crate::LOG_TARGET, "capture: GPU captures are the host's under winemetal");
}

/// No capture layer.
pub const fn capture_layer_loaded() -> bool {
    false
}

/// No Metal tools to report.
pub const fn log_metal_tools() {}

/// No capture to stop.
pub const fn stop_capture() {}
