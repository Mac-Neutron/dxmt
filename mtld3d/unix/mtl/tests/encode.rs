//! Encoders as command lists, and the retirement thread (M2a plan, Task 3).

mod common;

use core::ptr::NonNull;
use std::sync::{Arc, Mutex};

use mtld3d_mtl::{
    block2::RcBlock,
    objc2::{Retained, runtime::ProtocolObject},
    objc2_foundation::NSString,
    objc2_metal::{
        MTLBlitCommandEncoder, MTLBuffer, MTLClearColor, MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandEncoder,
        MTLCommandQueue, MTLCullMode, MTLDevice, MTLLibrary as _, MTLLoadAction, MTLOrigin, MTLPixelFormat,
        MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor, MTLRenderPipelineDescriptor,
        MTLResourceOptions, MTLSamplerDescriptor, MTLSize, MTLStorageMode, MTLStoreAction, MTLTexture,
        MTLTextureDescriptor, MTLTextureUsage, set_status_for_test,
    },
};

type Device = ProtocolObject<dyn MTLDevice>;

const SOURCE: &str = r"
#include <metal_stdlib>
using namespace metal;
struct V { float4 pos [[position]]; float4 color; };
vertex V vs(uint vid [[vertex_id]], constant float2 *pos [[buffer(0)]], texture2d<float> vtex [[texture(0)]],
            sampler vsamp [[sampler(0)]]) {
    V v;
    v.pos = float4(pos[vid], 0, 1);
    v.color = vtex.sample(vsamp, float2(0.5), level(0));
    return v;
}
fragment float4 fs(V in [[stage_in]], texture2d<float> ftex [[texture(0)]], sampler fsamp [[sampler(0)]]) {
    return in.color * ftex.sample(fsamp, float2(0.5));
}
";

fn texture(device: &Device, width: usize, height: usize, usage: MTLTextureUsage) -> Retained<ProtocolObject<dyn MTLTexture>> {
    let desc = MTLTextureDescriptor::new();
    desc.setPixelFormat(MTLPixelFormat::RGBA8Unorm);
    // SAFETY: sizes Metal accepts.
    unsafe {
        desc.setWidth(width);
        desc.setHeight(height);
    }
    desc.setUsage(usage);
    desc.setStorageMode(MTLStorageMode::Shared);
    device.newTextureWithDescriptor(&desc).expect("a texture")
}

fn one_pixel(device: &Device, rgba: [u8; 4]) -> Retained<ProtocolObject<dyn MTLTexture>> {
    let t = texture(device, 1, 1, MTLTextureUsage::ShaderRead);
    let region = MTLRegion { origin: MTLOrigin { x: 0, y: 0, z: 0 }, size: MTLSize { width: 1, height: 1, depth: 1 } };
    // SAFETY: four bytes for a 1x1 RGBA8 texture.
    unsafe { t.replaceRegion_mipmapLevel_slice_withBytes_bytesPerRow_bytesPerImage(region, 0, 0, NonNull::from(&rgba).cast(), 4, 4) };
    t
}

/// Draws `triangles` (a list of three-vertex position sets, each with its cull mode) into a 64x64 target cleared to
/// blue, through vertex bytes, a vertex texture (red) and both sampler binds, and reads it back.
fn draw(device: &Device, triangles: &[([f32; 6], MTLCullMode)]) -> Vec<u8> {
    let library = device.newLibraryWithSource_options_error(&NSString::from_str(SOURCE), None).expect("a library");
    let desc = MTLRenderPipelineDescriptor::new();
    desc.setVertexFunction(library.newFunctionWithName(&NSString::from_str("vs")).as_deref());
    desc.setFragmentFunction(library.newFunctionWithName(&NSString::from_str("fs")).as_deref());
    // SAFETY: index < 8.
    unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) }.setPixelFormat(MTLPixelFormat::RGBA8Unorm);
    let pso = device.newRenderPipelineStateWithDescriptor_error(&desc).expect("a pipeline");
    let sampler = device.newSamplerStateWithDescriptor(&MTLSamplerDescriptor::new()).expect("a sampler");
    let target = texture(device, 64, 64, MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    let red = one_pixel(device, [255, 0, 0, 255]);
    let white = one_pixel(device, [255, 255, 255, 255]);
    let readback = device.newBufferWithLength_options(64 * 64 * 4, MTLResourceOptions::StorageModeShared).expect("a buffer");

    let queue = device.newCommandQueue().expect("a queue");
    let cb = queue.commandBuffer().expect("a command buffer");
    let pass = MTLRenderPassDescriptor::new();
    // SAFETY: index < 8.
    let color = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
    color.setTexture(Some(&target));
    color.setLoadAction(MTLLoadAction::Clear);
    color.setStoreAction(MTLStoreAction::Store);
    color.setClearColor(MTLClearColor { red: 0.0, green: 0.0, blue: 1.0, alpha: 1.0 });
    let encoder = cb.renderCommandEncoderWithDescriptor(&pass).expect("an encoder");
    encoder.setRenderPipelineState(&pso);
    // SAFETY: binds of live objects; the bytes are copied.
    unsafe {
        encoder.setVertexTexture_atIndex(Some(&red), 0);
        encoder.setVertexSamplerState_atIndex(Some(&sampler), 0);
        encoder.setFragmentTexture_atIndex(Some(&white), 0);
        encoder.setFragmentSamplerState_atIndex(Some(&sampler), 0);
    }
    for (positions, cull) in triangles {
        encoder.setCullMode(*cull);
        // SAFETY: 24 bytes of positions, copied; three vertices.
        unsafe {
            encoder.setVertexBytes_length_atIndex(NonNull::from(positions).cast(), 24, 0);
            encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
        }
    }
    encoder.endEncoding();
    let blit = cb.blitCommandEncoder().expect("a blit encoder");
    // SAFETY: the whole target into a buffer of its size.
    unsafe {
        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
            &target, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: 64, height: 64, depth: 1 }, &readback, 0, 256, 256 * 64,
        );
    }
    blit.endEncoding();
    cb.commit();
    cb.waitUntilCompleted();
    assert_eq!(cb.status(), MTLCommandBufferStatus::Completed);
    // SAFETY: the buffer's own bytes, written by the completed blit.
    unsafe { core::slice::from_raw_parts(readback.contents().as_ptr().cast::<u8>(), 64 * 64 * 4) }.to_vec()
}

fn pixel(image: &[u8], x: usize, y: usize) -> [u8; 4] {
    let i = (y * 64 + x) * 4;
    [image[i], image[i + 1], image[i + 2], image[i + 3]]
}

const LEFT: [f32; 6] = [-1.0, -1.0, -1.0, 1.0, -0.1, 0.0];
/// The right half's triangle: whichever face its winding makes it, exactly one of `Front` and `Back` culls it.
const RIGHT_CCW: [f32; 6] = [0.1, 0.0, 1.0, -1.0, 1.0, 1.0];

#[test]
fn a_triangle_through_the_direct_binds_reads_back() {
    let device = common::device();
    let image = draw(&device, &[([-1.0, -1.0, 0.0, 1.0, 1.0, -1.0], MTLCullMode::None)]);
    assert_eq!(pixel(&image, 32, 40), [255, 0, 0, 255], "the triangle, red from the vertex texture");
    assert_eq!(pixel(&image, 1, 1), [0, 0, 255, 255], "the clear");
}

#[test]
fn cull_changes_between_draws_reach_the_gpu() {
    let device = common::device();
    let drawn = draw(&device, &[(LEFT, MTLCullMode::None), (RIGHT_CCW, MTLCullMode::None)]);
    let right_drawn = pixel(&drawn, 56, 32);
    assert_eq!(pixel(&drawn, 8, 32), [255, 0, 0, 255]);
    assert_eq!(right_drawn, [255, 0, 0, 255]);
    // The same right triangle under each cull mode: one of Front and Back removes it, so the second draw's cull
    // reached the encoder (it was not the first draw's None).
    let front = draw(&device, &[(LEFT, MTLCullMode::None), (RIGHT_CCW, MTLCullMode::Front)]);
    let back = draw(&device, &[(LEFT, MTLCullMode::None), (RIGHT_CCW, MTLCullMode::Back)]);
    assert_eq!(pixel(&front, 8, 32), [255, 0, 0, 255]);
    assert_eq!(pixel(&back, 8, 32), [255, 0, 0, 255]);
    let culled = [pixel(&front, 56, 32), pixel(&back, 56, 32)];
    assert!(culled.contains(&[0, 0, 255, 255]), "neither Front nor Back culled the right triangle: {culled:?}");
    assert!(culled.contains(&[255, 0, 0, 255]), "both culled it: {culled:?}");
}

#[test]
fn completion_handlers_run_once_each_in_commit_order() {
    let device = common::device();
    let queue = device.newCommandQueue().expect("a queue");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut last = None;
    for i in 0..100 {
        let cb = queue.commandBuffer().expect("a command buffer");
        let seen = Arc::clone(&seen);
        let handler = RcBlock::new(move |_cb: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| seen.lock().unwrap().push(i));
        // SAFETY: a live block; the facade keeps its own reference.
        unsafe { cb.addCompletedHandler(RcBlock::as_ptr(&handler)) };
        cb.commit();
        last = Some(cb);
    }
    last.unwrap().waitUntilCompleted();
    for _ in 0..200 {
        if seen.lock().unwrap().len() == 100 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(*seen.lock().unwrap(), (0..100).collect::<Vec<_>>());
}

#[test]
fn an_error_status_reaches_its_handler() {
    let device = common::device();
    let queue = device.newCommandQueue().expect("a queue");
    let cb = queue.commandBuffer().expect("a command buffer");
    set_status_for_test(&cb, MTLCommandBufferStatus::Error);
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = Mutex::new(tx);
    let handler = RcBlock::new(move |cb: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
        // SAFETY: the buffer the retirement thread hands over.
        let status = unsafe { cb.as_ref() }.status();
        tx.lock().unwrap().send(status).unwrap();
    });
    // SAFETY: a live block.
    unsafe { cb.addCompletedHandler(RcBlock::as_ptr(&handler)) };
    drop(handler);
    cb.commit();
    let status = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("the handler ran");
    assert_eq!(status, MTLCommandBufferStatus::Error);
}
