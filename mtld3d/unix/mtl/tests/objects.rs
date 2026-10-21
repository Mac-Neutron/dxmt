//! Objects, resources.
//!
//! State objects, libraries and pipelines through the facade (M2a plan, Task 2).

mod common;

use core::ffi::{c_char, c_int, c_void};

use mtld3d_mtl::{
    objc2_foundation::NSString,
    objc2_metal::{
        MTLBuffer, MTLCompareFunction, MTLCompileOptions, MTLDepthStencilDescriptor, MTLDevice, MTLFunction,
        MTLLanguageVersion, MTLLibrary, MTLPixelFormat, MTLRenderPipelineDescriptor, MTLResource, MTLResourceOptions,
        MTLSamplerAddressMode, MTLSamplerDescriptor, MTLSamplerMinMagFilter, MTLStencilDescriptor,
        MTLStencilOperation, MTLStorageMode, MTLTexture, MTLTextureDescriptor, MTLTextureSwizzle,
        MTLTextureSwizzleChannels, MTLTextureType, MTLTextureUsage, MTLVertexDescriptor, MTLVertexFormat,
    },
};
#[cfg(target_os = "linux")]
use mtld3d_mtl::objc2_metal::{MTLCommandBuffer, MTLCommandQueue};
#[cfg(target_vendor = "apple")]
use mtld3d_mtl::table::Table;

#[cfg(target_vendor = "apple")]
#[repr(C)]
struct DlInfo {
    fname: *const c_char,
    fbase: *mut c_void,
    sname: *const c_char,
    saddr: *mut c_void,
}

#[cfg(target_vendor = "apple")]
unsafe extern "C" {
    fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
}

/// macOS's winemetal.so names each slot's function.
///
/// The VM guest's forwards every slot through one stub each.
#[cfg(target_vendor = "apple")]
#[test]
fn each_slot_is_the_function_the_facade_means() {
    let _ = common::device();
    let path = std::env::var("MTLD3D_WINEMETAL").unwrap();
    let nm = std::process::Command::new("nm").args(["-a", &path]).output().expect("nm");
    let symbols: rustc_hash::FxHashMap<String, usize> = String::from_utf8_lossy(&nm.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let address = usize::from_str_radix(words.next()?, 16).ok()?;
            let _kind = words.next()?;
            Some((words.next()?.to_owned(), address))
        })
        .collect();
    let table = Table::get().unwrap();
    for &(slot, name) in mtld3d_mtl::SLOTS {
        let address = table.slot_address(slot);
        let mut info = DlInfo { fname: core::ptr::null(), fbase: core::ptr::null_mut(), sname: core::ptr::null(), saddr: core::ptr::null_mut() };
        // SAFETY: a lookup of an address in a loaded image.
        assert_ne!(unsafe { dladdr(address as *const c_void, &raw mut info) }, 0, "slot {slot}");
        let offset = address - info.fbase as usize;
        let expected = symbols.get(&format!("_{name}")).unwrap_or_else(|| panic!("{path} has no {name} (slot {slot})"));
        assert_eq!(offset, *expected, "slot {slot} is not {name}");
    }
}

/// The VM guest's memory (D3D9 M2b).
///
/// 16 KiB pages a buffer is made over, zeroed, written and read back through it.
#[cfg(target_os = "linux")]
#[test]
fn winemetal_memory_makes_no_copy_buffers() {
    let device = common::device();
    let pages = mtld3d_mtl::memory::alloc(65536);
    assert!(!pages.is_null(), "slot 153 gave no memory");
    assert_eq!(pages as usize % 16384, 0, "not on a 16 KiB page");
    // SAFETY: 65536 bytes winemetal just handed out.
    let bytes = unsafe { core::slice::from_raw_parts_mut(pages, 65536) };
    assert!(bytes.iter().all(|&b| b == 0), "not zeroed");
    bytes[40000] = 0x5a;
    // SAFETY: the pages are live until freed below, after the buffer is gone.
    let buffer = unsafe {
        device.newBufferWithBytesNoCopy_length_options_deallocator(
            core::ptr::NonNull::new(pages.cast()).unwrap(),
            65536,
            MTLResourceOptions::StorageModeShared,
            None,
        )
    }
    .expect("a buffer over the pages");
    assert_eq!(buffer.contents().as_ptr().cast::<u8>(), pages);
    drop(buffer);
    mtld3d_mtl::memory::free(pages);
}

/// The VM guest's winemetal.so counts its context's live handles (`mn_wm_stats`).
#[cfg(target_os = "linux")]
fn guest_live_handles() -> u64 {
    #[repr(C)]
    #[derive(Default)]
    struct Stats {
        live: u64,
        deferred: u64,
        failures: u64,
    }
    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }
    let path = std::ffi::CString::new(std::env::var("MTLD3D_WINEMETAL").unwrap()).unwrap();
    // SAFETY: the library the facade loaded (the same handle again); RTLD_NOW.
    let lib = unsafe { dlopen(path.as_ptr(), 2) };
    // SAFETY: a lookup in it.
    let f = unsafe { dlsym(lib, c"mn_wm_stats".as_ptr()) };
    assert!(!f.is_null(), "the guest winemetal.so has no mn_wm_stats");
    // SAFETY: int mn_wm_stats(struct mn_wm_stats *).
    let f: unsafe extern "C" fn(*mut Stats) -> c_int = unsafe { core::mem::transmute(f) };
    let mut stats = Stats::default();
    // SAFETY: a struct of its layout.
    assert_eq!(unsafe { f(&raw mut stats) }, 0);
    stats.live
}

/// In the VM's guest, an autoreleased return goes with the pool it was made in.
///
/// With none open it would live until the process ends, on both sides.
#[cfg(target_os = "linux")]
#[test]
fn autoreleased_returns_go_with_their_pool() {
    let device = common::device();
    let queue = device.newCommandQueue().expect("a queue");
    let before = guest_live_handles();
    // Committed, as mtld3d's are: a queue holds at most 64 buffers not yet completed, and the host's Metal waits for
    // one to complete before it makes another (an uncommitted buffer never does).
    for _ in 0..1000 {
        mtld3d_mtl::objc2::rc::autoreleasepool(|_| {
            let cb = queue.commandBuffer().expect("a command buffer");
            cb.commit();
            cb.waitUntilCompleted();
        });
    }
    // Other tests run beside this one and hold handles of their own meanwhile, and the retirement threads' last
    // releases may still be queued on their rings (the VM host has one executor for every ring): up to 10 s for them.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut after = guest_live_handles();
    while after >= before + 100 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
        after = guest_live_handles();
    }
    assert!(after < before + 100, "{} handles left of 1000 command buffers", after.saturating_sub(before));
}

#[test]
fn a_device_answers_its_queries() {
    let device = common::device();
    assert!(!device.name().to_rust().is_empty());
    assert_ne!(device.registryID(), 0);
    assert!(device.hasUnifiedMemory());
    assert!(device.supportsTextureSampleCount(4));
    assert!(device.supports32BitFloatFiltering());
}

#[test]
fn a_shared_buffer_has_its_length_and_contents() {
    let device = common::device();
    let buffer = device.newBufferWithLength_options(4096, MTLResourceOptions::StorageModeShared).expect("a buffer");
    assert_eq!(buffer.length(), 4096);
    assert_eq!(buffer.storageMode(), MTLStorageMode::Shared);
    let contents = buffer.contents().as_ptr().cast::<u32>();
    // SAFETY: the buffer's own 4096 bytes.
    unsafe {
        contents.add(1000).write(0xdead_beef);
        assert_eq!(contents.add(1000).read(), 0xdead_beef);
    }
}

#[test]
fn a_texture_and_its_view_read_back_as_described() {
    let device = common::device();
    let desc = MTLTextureDescriptor::new();
    desc.setTextureType(MTLTextureType::Type2D);
    desc.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    // SAFETY: sizes Metal accepts.
    unsafe {
        desc.setWidth(64);
        desc.setHeight(32);
        desc.setMipmapLevelCount(3);
    }
    desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead | MTLTextureUsage::PixelFormatView);
    desc.setStorageMode(MTLStorageMode::Private);
    let texture = device.newTextureWithDescriptor(&desc).expect("a texture");
    assert_eq!((texture.width(), texture.height(), texture.depth()), (64, 32, 1));
    assert_eq!((texture.mipmapLevelCount(), texture.sampleCount(), texture.arrayLength()), (3, 1, 1));
    assert_eq!(texture.textureType(), MTLTextureType::Type2D);
    assert_eq!(texture.pixelFormat(), MTLPixelFormat::BGRA8Unorm);
    assert_eq!(texture.storageMode(), MTLStorageMode::Private);
    assert!(texture.usage().contains(MTLTextureUsage::RenderTarget));
    let swizzle = MTLTextureSwizzleChannels {
        red: MTLTextureSwizzle::Blue,
        green: MTLTextureSwizzle::Green,
        blue: MTLTextureSwizzle::Red,
        alpha: MTLTextureSwizzle::One,
    };
    // SAFETY: a view of the texture's own levels and slice.
    let view = unsafe {
        texture.newTextureViewWithPixelFormat_textureType_levels_slices_swizzle(
            MTLPixelFormat::BGRA8Unorm_sRGB,
            MTLTextureType::Type2D,
            mtld3d_mtl::objc2_foundation::NSRange::new(1, 2),
            mtld3d_mtl::objc2_foundation::NSRange::new(0, 1),
            swizzle,
        )
    }
    .expect("a view");
    assert_eq!((view.width(), view.height(), view.mipmapLevelCount()), (32, 16, 2));
    assert_eq!(view.pixelFormat(), MTLPixelFormat::BGRA8Unorm_sRGB);
    assert_eq!(view.swizzle().red, MTLTextureSwizzle::Blue);
}

#[test]
fn partial_descriptors_take_metal_defaults() {
    let device = common::device();
    let sampler = MTLSamplerDescriptor::new();
    sampler.setMinFilter(MTLSamplerMinMagFilter::Linear);
    sampler.setSAddressMode(MTLSamplerAddressMode::Repeat);
    assert!(device.newSamplerStateWithDescriptor(&sampler).is_some());
    let stencil = MTLStencilDescriptor::new();
    stencil.setStencilCompareFunction(MTLCompareFunction::Equal);
    stencil.setDepthStencilPassOperation(MTLStencilOperation::IncrementClamp);
    let depth = MTLDepthStencilDescriptor::new();
    depth.setDepthCompareFunction(MTLCompareFunction::LessEqual);
    depth.setDepthWriteEnabled(true);
    depth.setFrontFaceStencil(Some(&stencil));
    assert!(device.newDepthStencilStateWithDescriptor(&depth).is_some());
}

const SOURCE: &str = r"
#include <metal_stdlib>
using namespace metal;
struct In { float2 pos [[attribute(0)]]; float4 color [[attribute(1)]]; };
struct Out { float4 pos [[position]]; float4 color; };
vertex Out vs(In in [[stage_in]]) { Out o; o.pos = float4(in.pos, 0, 1); o.color = in.color; return o; }
fragment float4 fs(Out in [[stage_in]]) { return in.color; }
";

fn library(device: &mtld3d_mtl::objc2::runtime::ProtocolObject<dyn MTLDevice>) -> mtld3d_mtl::objc2::Retained<mtld3d_mtl::objc2::runtime::ProtocolObject<dyn MTLLibrary>> {
    let options = MTLCompileOptions::new();
    options.setLanguageVersion(MTLLanguageVersion::Version2_4);
    options.setPreserveInvariance(true);
    device.newLibraryWithSource_options_error(&NSString::from_str(SOURCE), Some(&options)).expect("a library")
}

#[test]
fn a_source_library_with_options_gives_its_functions() {
    let device = common::device();
    let library = library(&device);
    let vs = library.newFunctionWithName(&NSString::from_str("vs")).expect("vs");
    assert_eq!(vs.name().to_rust(), "vs");
    assert!(library.newFunctionWithName(&NSString::from_str("absent")).is_none());
}

#[test]
fn a_pipeline_reads_a_vertex_descriptor() {
    let device = common::device();
    let library = library(&device);
    let desc = MTLRenderPipelineDescriptor::new();
    desc.setVertexFunction(library.newFunctionWithName(&NSString::from_str("vs")).as_deref());
    desc.setFragmentFunction(library.newFunctionWithName(&NSString::from_str("fs")).as_deref());
    // SAFETY: index < 8.
    unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) }.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    let vertex = MTLVertexDescriptor::new();
    // SAFETY: indices < 31.
    unsafe {
        let pos = vertex.attributes().objectAtIndexedSubscript(0);
        pos.setFormat(MTLVertexFormat::Float2);
        pos.setOffset(0);
        pos.setBufferIndex(1);
        let color = vertex.attributes().objectAtIndexedSubscript(1);
        color.setFormat(MTLVertexFormat::UChar4Normalized_BGRA);
        color.setOffset(8);
        color.setBufferIndex(1);
        vertex.layouts().objectAtIndexedSubscript(1).setStride(12);
    }
    desc.setVertexDescriptor(Some(&vertex));
    assert!(device.newRenderPipelineStateWithDescriptor_error(&desc).is_ok());
    // Without the vertex descriptor the [[stage_in]] function can't link: the descriptor reached Metal.
    desc.setVertexDescriptor(None);
    assert!(device.newRenderPipelineStateWithDescriptor_error(&desc).is_err());
}

#[test]
fn a_compile_error_carries_its_text() {
    let device = common::device();
    let error = device.newLibraryWithSource_options_error(&NSString::from_str("this is not MSL"), None).expect_err("an error");
    assert!(error.localizedDescription().to_rust().contains("error"), "{error}");
}
