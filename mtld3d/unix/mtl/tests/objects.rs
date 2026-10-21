//! Objects, resources, state objects, libraries and pipelines through the facade (M2a plan, Task 2).

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
    table::Table,
};

#[repr(C)]
struct DlInfo {
    fname: *const c_char,
    fbase: *mut c_void,
    sname: *const c_char,
    saddr: *mut c_void,
}

unsafe extern "C" {
    fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
}

#[test]
fn each_slot_is_the_function_the_facade_means() {
    let _ = common::device();
    let path = std::env::var("MTLD3D_WINEMETAL").unwrap();
    let nm = std::process::Command::new("nm").args(["-a", &path]).output().expect("nm");
    let symbols: std::collections::HashMap<String, usize> = String::from_utf8_lossy(&nm.stdout)
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
