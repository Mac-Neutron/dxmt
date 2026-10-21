// MacNeutron: the winemetal facade's builds (the loopback and the Linux one) leave out macOS's window code, the
// cursor overlay and the crash reporter's Mach half; what only those call is `cfg(not(mtl_facade))`.

use core::ffi::c_void;

use mtld3d_shared::Thunks;
use strum::{EnumCount, VariantArray};

#[cfg(not(target_os = "linux"))]
mod crash;
#[cfg(target_os = "linux")]
#[path = "crash_linux.rs"]
mod crash;
mod draw;
mod encoder;
mod encoder_service;
mod handlers;
#[cfg(not(mtl_facade))]
mod hud_state;
#[cfg(mtl_facade)]
#[path = "hud_state_wm.rs"]
mod hud_state;
mod log_file;
#[cfg(not(mtl_facade))]
mod main_thread_checker;
#[cfg(mtl_facade)]
#[path = "main_thread_checker_wm.rs"]
mod main_thread_checker;
mod metal;
mod mtl;
mod shader_prewarm;
mod shader_programs;
mod stack_page;
mod translation;

/// `log` target used by every call inside this crate.
///
/// d3d9.dll fires a one-shot `InitLogger` thunk on load — see
/// `handlers::init_logger_handler` — which registers `env_logger`; all
/// other handlers log through that backend. A handler that runs before
/// that thunk would silently no-op, which is why the PE side dispatches
/// `InitLogger` from its own `DllMain` before any other call.
const LOG_TARGET: &str = "mtld3d::unix";

#[unsafe(no_mangle)]
pub static __wine_unix_call_funcs: [UnixCallFn; Thunks::COUNT] = DISPATCH_TABLE;

#[unsafe(no_mangle)]
pub static __wine_unix_call_wow64_funcs: [UnixCallFn; Thunks::COUNT] = WOW64_DISPATCH_TABLE;

type UnixCallFn = unsafe extern "C" fn(*mut c_void) -> i32;

const DISPATCH_TABLE: [UnixCallFn; Thunks::COUNT] = build_dispatch_table::<false>();


/// Every native Rust allocation goes through snmalloc, the allocator the PE side uses.
///
/// Process-wide resource: a dylib has one global allocator. The native encoder,
/// submit and worker threads allocate and free per packet, and the default
/// macOS allocator cost the encoder about 5.5 us per packet more than snmalloc
/// in a matched streaming measurement. Only Rust's allocation calls change:
/// snmalloc exports `sn_rust_*` entry points, not `malloc` or `free`, so memory
/// Objective-C or C allocate is still freed by them, and no buffer this crate
/// hands to Metal is freed by Metal (`bytesNoCopy` wrappers carry no
/// deallocator).
#[global_allocator]
static ALLOCATOR: snmalloc_rs::SnMalloc = snmalloc_rs::SnMalloc;

/// Wrap a handler in an `@autoreleasepool` so every dispatch call drains on return.
///
/// The pool catches any autoreleased Apple objects (most visibly
/// `MTLCommandBuffer`). Wine's unix-call dispatcher does not set up a
/// pool, so without this wrap autoreleased objects live until thread exit
/// and pin every resource they encoded — `bytesNoCopy` pages stay wired
/// and `newBufferWithBytesNoCopy:` eventually returns nil. Each macro
/// invocation defines a uniquely-scoped `extern "C"` wrapper so wrapping
/// every handler is two new tokens at the call site. The thunks of
/// `macdrv::run_on_main_thread_sync` and `macdrv::run_on_main_thread_async`
/// are its main-thread twins: every closure the layer dispatches there runs
/// in a pool of its own for the same reason.
macro_rules! arp {
    ($wow64:ident, $inner:path) => {{
        extern "C" fn arp_wrap<const WOW64: bool>(args: *mut c_void) -> i32 {
            // A 32-bit process's entries mark it before anything else (see `WOW64_DISPATCH_TABLE`).
            if WOW64 {
                mtl::mark_wow64();
            }
            mtld3d_shared::crumb!(stringify!($inner), args as usize as u64);
            crate::mtl::objc2::rc::autoreleasepool(|_| $inner(args))
        }
        arp_wrap::<$wow64> as UnixCallFn
    }};
}

/// A 32-bit process's table.
///
/// The 64-bit one's handlers, each of which first marks the process 32-bit for the winemetal facade
/// (`mtl::mark_wow64`): the mark has to come before the first Metal call (the first entry's
/// autorelease pool), which loads winemetal's table, and the first thunk a process makes isn't
/// always `InitLogger` (a call at process start came first in the VM's guest, and the process got
/// winemetal's 64-bit table and memory its i386 side can't reach).
const WOW64_DISPATCH_TABLE: [UnixCallFn; Thunks::COUNT] = build_dispatch_table::<true>();

const fn dispatch<const W: bool>(code: Thunks) -> UnixCallFn {
    match code {
        Thunks::CreateShaderProgram => arp!(W, shader_programs::create_handler),
        Thunks::CancelShaderProgram => arp!(W, shader_programs::cancel_handler),
        Thunks::SubmitEncoderFrame => arp!(W, encoder_service::submit_handler),
        Thunks::EncoderControl => arp!(W, encoder_service::control_handler),
        Thunks::CreateEncoder => arp!(W, encoder_service::create_handler),
        Thunks::DestroyEncoder => arp!(W, encoder_service::destroy_handler),
        Thunks::InitLogger => arp!(W, handlers::init_logger_handler),
        Thunks::GetDeviceInfo => arp!(W, handlers::get_device_info_handler),
        Thunks::CreateCommandQueue => arp!(W, handlers::create_command_queue_handler),
        Thunks::AttachMetalLayer => arp!(W, handlers::attach_metal_layer_handler),
        Thunks::DestroyCommandQueue => arp!(W, handlers::destroy_command_queue_handler),
        Thunks::CreateBackbuffer => arp!(W, handlers::create_backbuffer_handler),
        Thunks::CreateDepthTexture => arp!(W, handlers::create_depth_texture_handler),
        Thunks::CreateColorTarget => arp!(W, handlers::create_color_target_handler),
        Thunks::BlitTextureToBuffer => arp!(W, handlers::blit_texture_to_buffer_handler),
        Thunks::DestroyResourcesBulk => arp!(W, handlers::destroy_resources_bulk_handler),
        Thunks::WriteLog => arp!(W, handlers::write_log_handler),
        Thunks::OpenLog => arp!(W, handlers::open_log_handler),
        Thunks::SetCursorOverlay => arp!(W, handlers::set_cursor_overlay_handler),
        Thunks::DetachMetalLayer => arp!(W, handlers::detach_metal_layer_handler),
        Thunks::SetPresentWaitPolicy => arp!(W, handlers::set_present_wait_policy_handler),
        Thunks::PresentClientSurface => arp!(W, handlers::present_client_surface_handler),
        Thunks::AllocPages => arp!(W, handlers::alloc_pages_handler),
        Thunks::FreePages => arp!(W, handlers::free_pages_handler),
    }
}

const fn build_dispatch_table<const W: bool>() -> [UnixCallFn; Thunks::COUNT] {
    extern "C" fn unimplemented_thunk(_args: *mut c_void) -> i32 {
        unimplemented!("Called unimplemented thunk.")
    }

    let mut table = [unimplemented_thunk as UnixCallFn; Thunks::COUNT];
    let variants = Thunks::VARIANTS;
    let mut i = 0;
    while i < variants.len() {
        table[variants[i] as usize] = dispatch::<W>(variants[i]);
        i += 1;
    }
    table
}
