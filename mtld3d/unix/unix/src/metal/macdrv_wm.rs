//! MacNeutron: `macdrv`'s entry points under the winemetal facade (D3D9 spec §5.1).
//!
//! The window code macOS's Wine has (view parking and adoption, client surfaces, the `WineMetalLayer` bypass, the
//! cursor overlay, the screen-parameter filter, EDR following) has no counterpart where the facade runs: a view is
//! winemetal's (`CreateMetalViewFromHWND`), made at attach and released when the device goes, the layer is sRGB,
//! the hardware cursor is the game's, and the panel rate is taken as 60 Hz (winemetal reports none). The attachment
//! registry is the same file as macOS's.

use std::sync::Arc;

use mtld3d_shared::{
    MetalHandle, SetCursorOverlayParams,
    mtl::{ClientSurfaceRequest, ColorSpacePolicy, SoftwareCursorPolicy},
    mtl_handle::{CAMetalLayerKind, MTLDeviceKind, NSViewKind},
};

use crate::mtl::{
    objc2::runtime::ProtocolObject,
    objc2_metal::{MTLDevice, MTLPixelFormat},
    objc2_quartz_core::{CAMetalLayer, CGSize, create_metal_view, release_metal_view},
};

#[path = "macdrv/attachment.rs"]
pub mod attachment;

use attachment::{AttachFlags, AttachLatches, Attachment};

/// The rate a vsync request caps presents at: winemetal reports no panel rate.
const PANEL_HZ: f64 = 60.0;

/// The guest's vsync ask plus the user's frame-rate ceiling (as macOS's).
pub struct PresentPacing {
    pub vsync_requested: bool,
    pub max_fps: u32,
}

fn min_present_duration(pacing: &PresentPacing) -> f64 {
    let vsync = if pacing.vsync_requested { 1.0 / PANEL_HZ } else { 0.0 };
    let cap = if pacing.max_fps > 0 { 1.0 / f64::from(pacing.max_fps) } else { 0.0 };
    vsync.max(cap)
}

fn pack_pacing(pacing: &PresentPacing) -> u64 {
    (u64::from(pacing.max_fps) << 1) | u64::from(pacing.vsync_requested)
}

/// Everything the PE side's `AttachMetalLayer` carries (as macOS's).
pub struct LayerAttachRequest {
    pub hwnd: u64,
    pub width: u32,
    pub height: u32,
    pub pacing: PresentPacing,
    pub hdr_enable: bool,
    pub color_space: ColorSpacePolicy,
    pub backing_scale_sink_ptr: u64,
    pub cursor_kick_sink_ptr: u64,
    pub client_surface_request_sink_ptr: u64,
    pub software_cursor: SoftwareCursorPolicy,
}

/// What attach answers the PE side (as macOS's).
#[derive(Clone, Copy)]
pub struct DisplayCaps {
    pub backing_scale: u32,
    pub software_cursor_active: bool,
}

/// A view and its layer for `request.hwnd`, sized to the back buffer, registered as macOS's attach registers one.
pub fn attach_metal_layer(
    device_handle: MetalHandle<MTLDeviceKind>,
    request: LayerAttachRequest,
) -> Option<(MetalHandle<NSViewKind>, MetalHandle<CAMetalLayerKind>, DisplayCaps)> {
    if request.hwnd == 0 || device_handle.is_null() {
        return None;
    }
    // SAFETY: a live device handle from the PE side.
    let device = unsafe { ProtocolObject::<dyn MTLDevice>::from_handle(device_handle.raw()) };
    let Some((view, layer)) = create_metal_view(request.hwnd, device) else {
        log::warn!(target: crate::LOG_TARGET, "present: winemetal made no view for window {:#x}", request.hwnd);
        return None;
    };
    layer.setDevice(Some(device));
    layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    layer.setOpaque(true);
    layer.setFramebufferOnly(true);
    layer.setDisplaySyncEnabled(false);
    layer.setDrawableSize(CGSize::new(f64::from(request.width.max(1)), f64::from(request.height.max(1))));
    layer.set_srgb();
    let scale = layer.contentsScale().round().clamp(1.0, 8.0) as u32;
    let layer_handle = crate::mtl::objc2::rc::Retained::into_raw(layer) as usize;
    let latches = AttachLatches {
        hwnd: request.hwnd,
        flags: AttachFlags::empty(),
        color_space: request.color_space,
        pacing_bits: pack_pacing(&request.pacing),
        backing_scale: scale,
        backing_scale_sink: usize::try_from(request.backing_scale_sink_ptr).unwrap_or(0),
        cursor_kick_sink: usize::try_from(request.cursor_kick_sink_ptr).unwrap_or(0),
        client_surface_request_sink: usize::try_from(request.client_surface_request_sink_ptr).unwrap_or(0),
        client_surface: 0,
    };
    let att = attachment::register(view as usize, layer_handle, &latches);
    att.set_min_present_duration(min_present_duration(&request.pacing));
    log::info!(
        target: crate::LOG_TARGET,
        "present: winemetal view {view:#x} layer {layer_handle:#x} for window {:#x}, {}x{} (sRGB, {} Hz assumed)",
        request.hwnd, request.width, request.height, PANEL_HZ
    );
    // SAFETY: handles winemetal just made; the layer's +1 rides with the view until retire.
    let handles = unsafe { (MetalHandle::new(view), MetalHandle::new(layer_handle as u64)) };
    Some((handles.0, handles.1, DisplayCaps { backing_scale: scale, software_cursor_active: false }))
}

/// Unregisters the view's record (the view itself goes at `retire_metal_view`).
pub fn detach_metal_layer(view_handle: MetalHandle<NSViewKind>) -> Option<Arc<Attachment>> {
    let att = attachment::unregister(usize::try_from(view_handle.raw()).ok()?)?;
    super::gamma::detach(&att);
    Some(att)
}

/// Releases the view and the layer reference attach kept.
pub fn retire_metal_view(view_handle: MetalHandle<NSViewKind>, record: Option<&Attachment>) {
    if view_handle.is_null() {
        return;
    }
    if let Some(att) = record {
        // SAFETY: the +1 attach kept on the layer.
        drop(unsafe { crate::mtl::objc2::rc::Retained::from_raw(att.layer() as *mut CAMetalLayer) });
    }
    release_metal_view(view_handle.raw());
}

/// No client surface under the facade.
pub fn present_client_surface(_view_handle: MetalHandle<NSViewKind>, _reasons: ClientSurfaceRequest) {}

/// No App Nap activity: the facade's process is the VM's guest (or a loopback test), not a Mac app.
pub fn declare_latency_critical_activity() {}

/// The hardware cursor is the game's under the facade: never taken.
pub fn set_cursor_overlay(_params: &SetCursorOverlayParams, _pixels: Option<&[u8]>) -> bool {
    false
}

/// The pacing a Reset asks for.
pub fn set_display_sync_enabled(layer_handle: MetalHandle<CAMetalLayerKind>, pacing: &PresentPacing) {
    if let Some(att) = usize::try_from(layer_handle.raw()).ok().and_then(attachment::find_by_layer) {
        att.set_pacing_bits(pack_pacing(pacing));
        att.set_min_present_duration(min_present_duration(pacing));
    }
}

/// A host time in seconds as nanoseconds (the facade's presented times are seconds too).
pub fn host_seconds_to_ns(secs: f64) -> u64 {
    if secs <= 0.0 { 0 } else { (secs * 1e9) as u64 }
}

/// No cursor overlay to poll.
pub fn poll_from_present() {}

/// The drawable keeps attach's size: winemetal gives no layer bounds to follow.
pub fn sync_drawable_size(_layer: &CAMetalLayer) {}
