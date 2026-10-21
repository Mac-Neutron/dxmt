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

/// `len` bytes of zeroed pages Metal can be made over, or null (`AllocPages`).
///
/// macOS (the facade's loopback build, and the product build, which never asks): anonymous pages
/// aligned to the 16 KiB `PAGE_SIZE` a page lease requires; a Wine process's mappings are only 4
/// KiB-aligned, so the mapping is made 16 KiB larger and trimmed. The VM's guest: winemetal's own
/// allocator (M2b).
#[cfg(not(target_os = "linux"))]
#[must_use]
pub fn alloc_pages(len: usize) -> *mut u8 {
    const ALIGN: usize = 16 * 1024;
    let Some(total) = len.checked_add(ALIGN) else { return core::ptr::null_mut() };
    // SAFETY: an anonymous private mapping; nothing else names the range.
    let raw = unsafe {
        libc::mmap(core::ptr::null_mut(), total, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_ANON | libc::MAP_PRIVATE, -1, 0)
    };
    if raw == libc::MAP_FAILED {
        return core::ptr::null_mut();
    }
    let raw = raw as usize;
    let start = raw.next_multiple_of(ALIGN);
    if start > raw {
        // SAFETY: the head of the mapping just made, before `start`: page-aligned.
        unsafe { libc::munmap(raw as *mut libc::c_void, start - raw) };
    }
    let end = start + len;
    if raw + total > end {
        // SAFETY: the tail of the mapping just made, after `end`: page-aligned.
        unsafe { libc::munmap(end as *mut libc::c_void, raw + total - end) };
    }
    start as *mut u8
}

/// `len` bytes from winemetal's allocator, in the VM's guest.
///
/// Both sides map it (a no-copy buffer there must be made over it); the pages are 16 KiB and
/// zeroed (the guest's allocator zeroes what it hands out again).
#[cfg(target_os = "linux")]
#[must_use]
pub fn alloc_pages(len: usize) -> *mut u8 {
    mtld3d_mtl::memory::alloc(len)
}

/// Gives back `alloc_pages`' pages (the `FreePages` thunk).
#[cfg(not(target_os = "linux"))]
pub fn free_pages(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        // SAFETY: a mapping alloc_pages made of `len` bytes, no longer used (the PE side frees a PageBox once
        // Metal no longer reads it).
        unsafe { libc::munmap(ptr.cast(), len) };
    }
}

/// Gives back `alloc_pages`' pages to winemetal.
///
/// Which keeps them until no buffer is made over them.
#[cfg(target_os = "linux")]
pub fn free_pages(ptr: *mut u8, _len: usize) {
    if !ptr.is_null() {
        mtld3d_mtl::memory::free(ptr);
    }
}

/// The Unix side's own `PageBox`es from `alloc_pages` too, in the VM's guest.
///
/// The encoder's staging rows become no-copy buffers, which there must be made over the guest
/// allocator's memory, the host's mapping; any other page fails (the PE side's boxes take the same
/// pages through `PAGES_FROM_UNIX`). Set at the first device, before the encoder's first page.
#[cfg(target_os = "linux")]
pub fn install_page_allocator() {
    fn alloc(len: usize) -> Option<core::ptr::NonNull<u8>> {
        core::ptr::NonNull::new(alloc_pages(len))
    }
    fn free(ptr: core::ptr::NonNull<u8>, len: usize) {
        free_pages(ptr.as_ptr(), len);
    }
    mtld3d_core::page_box::set_page_allocator_source(|| Some(mtld3d_core::page_box::PageAllocator { alloc, free }));
}

/// Native pages on macOS: Metal takes any page-aligned memory there.
#[cfg(not(target_os = "linux"))]
pub const fn install_page_allocator() {}
