mod blit;
mod buffer;
#[cfg(not(mtl_facade))]
mod capture;
#[cfg(mtl_facade)]
#[path = "metal/capture_wm.rs"]
mod capture;
mod clear_quad;
mod command;
pub mod depth_transfer;
mod device;
mod frame_signal;
mod gamma;
mod gpu_time;
pub mod handle;
#[cfg(not(mtl_facade))]
mod macdrv;
#[cfg(mtl_facade)]
#[path = "metal/macdrv_wm.rs"]
mod macdrv;
mod null_texture;
mod pipeline;
mod present;
mod presenter;
mod record;
mod sampler;
mod shader;
pub mod submission;
mod texture;
mod transient;
mod upload_quad;
mod upscale;

pub use blit::ensure_blit_pipeline;
pub use buffer::{create_buffers, destroy_buffer};
pub use capture::{capture_layer_loaded, log_metal_tools, start_capture, stop_capture};
pub use clear_quad::ensure_clear_quad_pipeline;
pub use command::{BlitArgs, blit_texture_to_buffer, submit_frame, wait_for_gpu_retire};
pub use device::{create_command_queue, default_device_info, destroy_command_queue};
pub use frame_signal::{FrameSignal, FrameWait, PACED_PRESENT_CAP};
pub use gamma::set_gamma_ramp;
pub use macdrv::{
    LayerAttachRequest, PresentPacing, attach_metal_layer, declare_latency_critical_activity,
    detach_metal_layer, present_client_surface, retire_metal_view, set_cursor_overlay,
    set_display_sync_enabled,
};
pub use mtld3d_shared::perf::init_tracking_enabled;
pub use pipeline::{create_render_pipeline, destroy_render_pipeline};
pub use presenter::{set_wait_policy, wait_for_present_idle};
pub use record::DeviceRecord;
pub use sampler::{create_sampler_state, destroy_sampler_state};
pub use shader::{compile_shader_library, destroy_function, destroy_library};
pub use texture::{
    OPAQUE_BLACK, TRANSPARENT_BLACK, TextureClearBatch, clear_new_color_textures,
    create_backbuffer, create_color_target, create_depth_stencil_state, create_depth_texture,
    create_msaa_companion, create_texture_slice_view, create_textures, destroy_depth_stencil_state,
    destroy_texture,
};
pub use upload_quad::ensure_upload_pipeline;
pub use upscale::is_supported as upscale_is_supported;

/// MacNeutron: `len` bytes of zeroed pages Metal can be made over, or null (the `AllocPages` thunk).
///
/// macOS (the facade's loopback build, and the product build, which never asks): anonymous pages, 16 KiB-aligned as
/// the kernel maps them on Apple Silicon. The VM's guest: winemetal's own allocator (M2b).
#[must_use]
pub fn alloc_pages(len: usize) -> *mut u8 {
    // SAFETY: an anonymous private mapping; nothing else names the range.
    let ptr = unsafe {
        libc::mmap(core::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_ANON | libc::MAP_PRIVATE, -1, 0)
    };
    if ptr == libc::MAP_FAILED { core::ptr::null_mut() } else { ptr.cast() }
}

/// MacNeutron: gives back `alloc_pages`' pages (the `FreePages` thunk).
pub fn free_pages(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        // SAFETY: a mapping alloc_pages made of `len` bytes, no longer used (the PE side frees a PageBox once
        // Metal no longer reads it).
        unsafe { libc::munmap(ptr.cast(), len) };
    }
}
