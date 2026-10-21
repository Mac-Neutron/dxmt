//! objc2-metal's command queue, buffers and encoders over winemetal.
//!
//! An encoder records its calls as winemetal command nodes (`wmtcmd_*`) and hands the list over in one
//! `encodeCommands` call when it ends (or every 65,536 nodes, the VM's limit per call): per-draw calls never become
//! slot calls. Completion handlers, which winemetal has no form of, run on a retirement thread per queue that waits
//! on each committed buffer in commit order; a drawable's presented handler runs when the buffer that presents it
//! completes, and its presented time is that moment.

use core::{cell::RefCell, ffi::c_void, ptr::NonNull};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, mpsc},
    time::Instant,
};

use super::{
    CFTimeInterval, MTLBlitOption, MTLBuffer, MTLCommandBufferErrorOption, MTLCommandBufferStatus,
    MTLComputePipelineState, MTLCullMode, MTLDepthStencilState, MTLDevice, MTLEvent, MTLIndexType, MTLOrigin,
    MTLPrimitiveType, MTLRenderPassDescriptor, MTLRenderPipelineState, MTLResource, MTLSamplerState, MTLScissorRect,
    MTLSize, MTLTexture, MTLTriangleFillMode, MTLViewport, MTLVisibilityResultMode, device_of, handle_of, label_of,
    retained, set_label_of,
};
use crate::{
    block2::{DynBlock, Kept},
    foundation::{NSError, NSInteger, NSString, NSUInteger},
    meta,
    objc2::{
        self, Retained,
        runtime::{NSObjectProtocol, ProtocolObject},
    },
    table, wmt,
};

pub type MTLCommandBufferHandler = *mut DynBlock<dyn Fn(NonNull<ProtocolObject<dyn MTLCommandBuffer>>)>;
pub type MTLDrawablePresentedHandler = *mut DynBlock<dyn Fn(NonNull<ProtocolObject<dyn MTLDrawable>>)>;

// --- Command lists.

/// The VM's limit of nodes per `encodeCommands` call (`MN_WM_MAX_NODES`).
const MAX_NODES: usize = 65_536;
/// Bytes of nodes and payloads past which a list is handed over before it ends.
const MAX_BYTES: usize = 4 << 20;

#[repr(C)]
#[derive(Clone, Copy)]
struct Node<T> {
    ty: u16,
    reserved: [u16; 3],
    next: u64,
    body: T,
}

/// One encoder's list being built: nodes and their payloads in one 8-byte-aligned buffer, linked when it is handed
/// over (the buffer doesn't move during that call).
#[derive(Default)]
struct Nodes {
    words: Vec<u64>,
    starts: Vec<usize>,
    /// (word index of a node's payload pointer, word index of the payload)
    payloads: Vec<(usize, usize)>,
}

impl Nodes {
    fn push<T: Copy>(&mut self, ty: u16, body: T) -> usize {
        let node = Node { ty, reserved: [0; 3], next: 0, body };
        let start = self.words.len();
        let words = core::mem::size_of::<Node<T>>().div_ceil(8);
        self.words.resize(start + words, 0);
        // SAFETY: the words just reserved hold the node; u64 alignment covers every node's.
        unsafe { self.words.as_mut_ptr().add(start).cast::<Node<T>>().write_unaligned(node) };
        self.starts.push(start);
        start
    }

    /// A node whose body field at byte `pointer_at` (from the body's start) points at a copy of `bytes`.
    fn push_payload<T: Copy>(&mut self, ty: u16, body: T, pointer_at: usize, bytes: &[u8]) {
        let start = self.push(ty, body);
        let at = self.words.len();
        self.words.resize(at + bytes.len().div_ceil(8), 0);
        // SAFETY: the words just reserved hold the copy.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.words.as_mut_ptr().add(at).cast::<u8>(), bytes.len()) };
        self.payloads.push((start + 2 + pointer_at / 8, at));
    }

    fn full(&self) -> bool {
        self.starts.len() >= MAX_NODES || self.words.len() * 8 >= MAX_BYTES
    }

    /// Links the list and hands it to `slot` for `encoder`, then empties it.
    fn flush(&mut self, slot: u32, encoder: u64) {
        if self.starts.is_empty() {
            return;
        }
        let base = self.words.as_mut_ptr();
        for (i, &start) in self.starts.iter().enumerate() {
            let next = self.starts.get(i + 1).map_or(0, |&n| base as u64 + (n * 8) as u64);
            // SAFETY: word start+1 is the node's `next`.
            unsafe { *base.add(start + 1) = next };
        }
        for &(pointer, payload) in &self.payloads {
            // SAFETY: both inside the buffer.
            unsafe { *base.add(pointer) = base.add(payload) as u64 };
        }
        // SAFETY: start 0 is the head; the list stays put for the call.
        let mut params = wmt::ObjCmdNoRet { encoder, cmd_head: unsafe { base.add(self.starts[0]) }.cast() };
        // SAFETY: an encodeCommands slot over a well-formed list.
        unsafe { table::call(slot, &mut params) };
        self.words.clear();
        self.starts.clear();
        self.payloads.clear();
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Raster {
    fill: u8,
    cull: u8,
    bias: f32,
    slope: f32,
    clamp: f32,
}

const METAL_RASTER: Raster = Raster { fill: 0, cull: 0, bias: 0.0, slope: 0.0, clamp: 0.0 };

struct EncoderState {
    slot: u32,
    nodes: Nodes,
    raster: Raster,
}

thread_local! {
    /// The lists of the encoders this thread is recording (Metal's encoders are used from one thread at a time).
    static ENCODERS: RefCell<HashMap<u64, EncoderState>> = RefCell::new(HashMap::new());
}

fn record(encoder: u64, f: impl FnOnce(&mut EncoderState)) {
    ENCODERS.with(|map| {
        let mut map = map.borrow_mut();
        let state = map.get_mut(&encoder).expect("a facade encoder used on a thread other than the one that made it");
        f(state);
        if state.nodes.full() {
            state.nodes.flush(state.slot, encoder);
        }
    });
}

fn begin(encoder: u64, slot: u32) {
    ENCODERS.with(|map| {
        map.borrow_mut().insert(encoder, EncoderState { slot, nodes: Nodes::default(), raster: METAL_RASTER });
    });
}

fn end(encoder: u64) {
    if let Some(mut state) = ENCODERS.with(|map| map.borrow_mut().remove(&encoder)) {
        state.nodes.flush(state.slot, encoder);
    }
    let mut params = wmt::ObjNoRet { handle: encoder };
    // SAFETY: slot 33 ends a live encoder.
    unsafe { table::call(wmt::COMMAND_ENCODER_END_ENCODING, &mut params) };
}

fn bytes_of(bytes: NonNull<c_void>, length: usize) -> &'static [u8] {
    // SAFETY: the caller's bytes, valid for the call that copies them.
    unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), length) }
}

fn handle_or_zero<T: ?Sized>(obj: Option<&T>) -> u64 {
    obj.map_or(0, |o| handle_of(o))
}

// --- Node bodies (`wmtcmd_*` past the header).

#[repr(C)]
#[derive(Clone, Copy)]
struct SetBuffer { buffer: u64, offset: u64, index: u8 }
#[repr(C)]
#[derive(Clone, Copy)]
struct SetBytes { bytes: u64, length: u64, index: u8 }
#[repr(C)]
#[derive(Clone, Copy)]
struct SetObject { object: u64, index: u8 }
#[repr(C)]
#[derive(Clone, Copy)]
struct SetHandle { handle: u64 }
#[repr(C)]
#[derive(Clone, Copy)]
struct SetRasterizer { fill: u8, cull: u8, clip: u8, winding: u8, bias: f32, slope: f32, clamp: f32 }
#[repr(C)]
#[derive(Clone, Copy)]
struct SetVisibility { offset: u64, mode: u8 }
#[repr(C)]
#[derive(Clone, Copy)]
struct Draw { primitive: u8, start: u64, count: u64, instances: u32, base_instance: u32 }
#[repr(C)]
#[derive(Clone, Copy)]
struct DrawIndexed { primitive: u8, index_type: u8, count: u64, buffer: u64, offset: u64, instances: u32, base_vertex: i32, base_instance: u32 }
#[repr(C)]
#[derive(Clone, Copy)]
struct BlendColor { red: f32, green: f32, blue: f32, alpha: f32, stencil_ref: u8 }
#[repr(C)]
#[derive(Clone, Copy)]
struct StencilRef { stencil_ref: u8 }

#[repr(C)]
#[derive(Clone, Copy)]
struct BufferToBuffer { src: u64, src_offset: u64, dst: u64, dst_offset: u64, length: u64 }
#[repr(C)]
#[derive(Clone, Copy)]
struct BufferToTexture { src: u64, src_offset: u64, bytes_per_row: u32, bytes_per_image: u32, size: wmt::Size, dst: u64, slice: u32, level: u16, options: u16, origin: wmt::Origin }
#[repr(C)]
#[derive(Clone, Copy)]
struct TextureToBuffer { src: u64, slice: u32, level: u16, options: u16, origin: wmt::Origin, size: wmt::Size, dst: u64, offset: u64, bytes_per_row: u32, bytes_per_image: u32 }
#[repr(C)]
#[derive(Clone, Copy)]
struct TextureToTexture { src: u64, src_slice: u32, src_level: u32, src_origin: wmt::Origin, src_size: wmt::Size, dst: u64, dst_slice: u32, dst_level: u32, dst_origin: wmt::Origin }

#[repr(C)]
#[derive(Clone, Copy)]
struct ComputePso { pso: u64, threadgroup_size: wmt::Size }
#[repr(C)]
#[derive(Clone, Copy)]
struct Dispatch { size: wmt::Size }

const BLIT_BUFFER_TO_BUFFER: u16 = 1;
const BLIT_BUFFER_TO_TEXTURE: u16 = 2;
const BLIT_TEXTURE_TO_BUFFER: u16 = 3;
const BLIT_TEXTURE_TO_TEXTURE: u16 = 4;
const BLIT_GENERATE_MIPMAPS: u16 = 5;
const BLIT_BUFFER_TO_TEXTURE_OPTION: u16 = 10;
const BLIT_TEXTURE_TO_BUFFER_OPTION: u16 = 11;
const COMPUTE_DISPATCH: u16 = 1;
const COMPUTE_SET_PSO: u16 = 3;
const COMPUTE_SET_BUFFER: u16 = 4;
const COMPUTE_SET_BYTES: u16 = 7;
const COMPUTE_SET_TEXTURE: u16 = 8;

fn origin(o: MTLOrigin) -> wmt::Origin {
    wmt::Origin { x: o.x as u64, y: o.y as u64, z: o.z as u64 }
}
fn size(s: MTLSize) -> wmt::Size {
    wmt::Size { width: s.width as u64, height: s.height as u64, depth: s.depth as u64 }
}

// --- Encoders.

/// objc2's `MTLCommandEncoder`.
pub unsafe trait MTLCommandEncoder: NSObjectProtocol {
    fn device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        device_of(handle_of(self))
    }
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn setLabel(&self, label: Option<&NSString>) {
        set_label_of(handle_of(self), label);
    }
    fn endEncoding(&self) {
        end(handle_of(self));
    }
    /// Debug groups only show in a capture, which the facade doesn't take.
    fn pushDebugGroup(&self, _string: &NSString) {}
    fn popDebugGroup(&self) {}
}

/// objc2's `MTLRenderCommandEncoder`.
pub unsafe trait MTLRenderCommandEncoder: MTLCommandEncoder {
    fn setRenderPipelineState(&self, pipeline_state: &ProtocolObject<dyn MTLRenderPipelineState>) {
        let pso = pipeline_state.handle();
        record(handle_of(self), |s| { s.nodes.push(wmt::RENDER_SET_PSO, SetHandle { handle: pso }); });
    }
    unsafe fn setVertexBytes_length_atIndex(&self, bytes: NonNull<c_void>, length: NSUInteger, index: NSUInteger) {
        let data = bytes_of(bytes, length);
        record(handle_of(self), |s| {
            s.nodes.push_payload(wmt::RENDER_SET_VERTEX_BYTES, SetBytes { bytes: 0, length: length as u64, index: index as u8 }, 0, data);
        });
    }
    unsafe fn setVertexBuffer_offset_atIndex(&self, buffer: Option<&ProtocolObject<dyn MTLBuffer>>, offset: NSUInteger, index: NSUInteger) {
        let buffer = handle_or_zero(buffer);
        record(handle_of(self), |s| {
            s.nodes.push(wmt::RENDER_SET_VERTEX_BUFFER, SetBuffer { buffer, offset: offset as u64, index: index as u8 });
        });
    }
    unsafe fn setVertexTexture_atIndex(&self, texture: Option<&ProtocolObject<dyn MTLTexture>>, index: NSUInteger) {
        let texture = handle_or_zero(texture);
        record(handle_of(self), |s| { s.nodes.push(wmt::RENDER_SET_VERTEX_TEXTURE, SetObject { object: texture, index: index as u8 }); });
    }
    unsafe fn setVertexSamplerState_atIndex(&self, sampler: Option<&ProtocolObject<dyn MTLSamplerState>>, index: NSUInteger) {
        let sampler = handle_or_zero(sampler);
        record(handle_of(self), |s| {
            s.nodes.push(wmt::RENDER_SET_VERTEX_SAMPLER_STATE, SetObject { object: sampler, index: index as u8 });
        });
    }
    fn setViewport(&self, viewport: MTLViewport) {
        record(handle_of(self), |s| { s.nodes.push(wmt::RENDER_SET_VIEWPORT, viewport); });
    }
    fn setCullMode(&self, cull_mode: MTLCullMode) {
        raster(handle_of(self), |r| r.cull = cull_mode.0 as u8);
    }
    fn setDepthBias_slopeScale_clamp(&self, depth_bias: f32, slope_scale: f32, clamp: f32) {
        raster(handle_of(self), |r| {
            r.bias = depth_bias;
            r.slope = slope_scale;
            r.clamp = clamp;
        });
    }
    fn setScissorRect(&self, rect: MTLScissorRect) {
        record(handle_of(self), |s| { s.nodes.push(wmt::RENDER_SET_SCISSOR_RECT, rect); });
    }
    fn setTriangleFillMode(&self, fill_mode: MTLTriangleFillMode) {
        raster(handle_of(self), |r| r.fill = fill_mode.0 as u8);
    }
    unsafe fn setFragmentBytes_length_atIndex(&self, bytes: NonNull<c_void>, length: NSUInteger, index: NSUInteger) {
        let data = bytes_of(bytes, length);
        record(handle_of(self), |s| {
            s.nodes.push_payload(wmt::RENDER_SET_FRAGMENT_BYTES, SetBytes { bytes: 0, length: length as u64, index: index as u8 }, 0, data);
        });
    }
    unsafe fn setFragmentBuffer_offset_atIndex(&self, buffer: Option<&ProtocolObject<dyn MTLBuffer>>, offset: NSUInteger, index: NSUInteger) {
        let buffer = handle_or_zero(buffer);
        record(handle_of(self), |s| {
            s.nodes.push(wmt::RENDER_SET_FRAGMENT_BUFFER, SetBuffer { buffer, offset: offset as u64, index: index as u8 });
        });
    }
    unsafe fn setFragmentTexture_atIndex(&self, texture: Option<&ProtocolObject<dyn MTLTexture>>, index: NSUInteger) {
        let texture = handle_or_zero(texture);
        record(handle_of(self), |s| { s.nodes.push(wmt::RENDER_SET_FRAGMENT_TEXTURE, SetObject { object: texture, index: index as u8 }); });
    }
    unsafe fn setFragmentSamplerState_atIndex(&self, sampler: Option<&ProtocolObject<dyn MTLSamplerState>>, index: NSUInteger) {
        let sampler = handle_or_zero(sampler);
        record(handle_of(self), |s| {
            s.nodes.push(wmt::RENDER_SET_FRAGMENT_SAMPLER_STATE, SetObject { object: sampler, index: index as u8 });
        });
    }
    fn setBlendColorRed_green_blue_alpha(&self, red: f32, green: f32, blue: f32, alpha: f32) {
        record(handle_of(self), |s| {
            s.nodes.push(wmt::RENDER_SET_BLEND_FACTOR, BlendColor { red, green, blue, alpha, stencil_ref: 0 });
        });
    }
    fn setDepthStencilState(&self, depth_stencil_state: Option<&ProtocolObject<dyn MTLDepthStencilState>>) {
        let state = handle_or_zero(depth_stencil_state);
        record(handle_of(self), |s| { s.nodes.push(wmt::RENDER_SET_DEPTH_STENCIL_STATE, SetHandle { handle: state }); });
    }
    /// winemetal's node takes 8 bits: a Depth32Float_Stencil8 plane has no more.
    fn setStencilReferenceValue(&self, reference_value: u32) {
        record(handle_of(self), |s| {
            s.nodes.push(wmt::RENDER_SET_STENCIL_REF, StencilRef { stencil_ref: reference_value as u8 });
        });
    }
    fn setVisibilityResultMode_offset(&self, mode: MTLVisibilityResultMode, offset: NSUInteger) {
        record(handle_of(self), |s| {
            s.nodes.push(wmt::RENDER_SET_VISIBILITY_MODE, SetVisibility { offset: offset as u64, mode: mode.0 as u8 });
        });
    }
    unsafe fn drawPrimitives_vertexStart_vertexCount(&self, primitive_type: MTLPrimitiveType, vertex_start: NSUInteger, vertex_count: NSUInteger) {
        record(handle_of(self), |s| {
            s.nodes.push(
                wmt::RENDER_DRAW,
                Draw { primitive: primitive_type.0 as u8, start: vertex_start as u64, count: vertex_count as u64, instances: 1, base_instance: 0 },
            );
        });
    }
    #[allow(clippy::too_many_arguments)]
    unsafe fn drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount_baseVertex_baseInstance(
        &self,
        primitive_type: MTLPrimitiveType,
        index_count: NSUInteger,
        index_type: MTLIndexType,
        index_buffer: &ProtocolObject<dyn MTLBuffer>,
        index_buffer_offset: NSUInteger,
        instance_count: NSUInteger,
        base_vertex: NSInteger,
        base_instance: NSUInteger,
    ) {
        let buffer = index_buffer.handle();
        record(handle_of(self), |s| {
            s.nodes.push(
                wmt::RENDER_DRAW_INDEXED,
                DrawIndexed {
                    primitive: primitive_type.0 as u8,
                    index_type: index_type.0 as u8,
                    count: index_count as u64,
                    buffer,
                    offset: index_buffer_offset as u64,
                    instances: instance_count as u32,
                    base_vertex: base_vertex as i32,
                    base_instance: base_instance as u32,
                },
            );
        });
    }
}

/// Cull, fill and depth bias as one rasterizer node: winemetal sets the five together, so the facade keeps them.
fn raster(encoder: u64, f: impl FnOnce(&mut Raster)) {
    record(encoder, |s| {
        let before = s.raster;
        f(&mut s.raster);
        if s.raster != before {
            let r = s.raster;
            s.nodes.push(
                wmt::RENDER_SET_RASTERIZER_STATE,
                SetRasterizer { fill: r.fill, cull: r.cull, clip: 0, winding: 0, bias: r.bias, slope: r.slope, clamp: r.clamp },
            );
        }
    });
}

/// objc2's `MTLBlitCommandEncoder`.
pub unsafe trait MTLBlitCommandEncoder: MTLCommandEncoder {
    /// A managed resource's sync: storage is shared on Apple Silicon, so there is nothing to do.
    fn synchronizeResource(&self, _resource: &ProtocolObject<dyn MTLResource>) {}
    #[allow(clippy::too_many_arguments)]
    unsafe fn copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
        &self,
        source_texture: &ProtocolObject<dyn MTLTexture>,
        source_slice: NSUInteger,
        source_level: NSUInteger,
        source_origin: MTLOrigin,
        source_size: MTLSize,
        destination_texture: &ProtocolObject<dyn MTLTexture>,
        destination_slice: NSUInteger,
        destination_level: NSUInteger,
        destination_origin: MTLOrigin,
    ) {
        let body = TextureToTexture {
            src: source_texture.handle(),
            src_slice: source_slice as u32,
            src_level: source_level as u32,
            src_origin: origin(source_origin),
            src_size: size(source_size),
            dst: destination_texture.handle(),
            dst_slice: destination_slice as u32,
            dst_level: destination_level as u32,
            dst_origin: origin(destination_origin),
        };
        record(handle_of(self), |s| { s.nodes.push(BLIT_TEXTURE_TO_TEXTURE, body); });
    }
    #[allow(clippy::too_many_arguments)]
    unsafe fn copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
        &self,
        source_buffer: &ProtocolObject<dyn MTLBuffer>,
        source_offset: NSUInteger,
        source_bytes_per_row: NSUInteger,
        source_bytes_per_image: NSUInteger,
        source_size: MTLSize,
        destination_texture: &ProtocolObject<dyn MTLTexture>,
        destination_slice: NSUInteger,
        destination_level: NSUInteger,
        destination_origin: MTLOrigin,
    ) {
        // SAFETY: the same call with no option.
        unsafe {
            self.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin_options(
                source_buffer, source_offset, source_bytes_per_row, source_bytes_per_image, source_size, destination_texture,
                destination_slice, destination_level, destination_origin, MTLBlitOption::None,
            );
        }
    }
    #[allow(clippy::too_many_arguments)]
    unsafe fn copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin_options(
        &self,
        source_buffer: &ProtocolObject<dyn MTLBuffer>,
        source_offset: NSUInteger,
        source_bytes_per_row: NSUInteger,
        source_bytes_per_image: NSUInteger,
        source_size: MTLSize,
        destination_texture: &ProtocolObject<dyn MTLTexture>,
        destination_slice: NSUInteger,
        destination_level: NSUInteger,
        destination_origin: MTLOrigin,
        options: MTLBlitOption,
    ) {
        let body = BufferToTexture {
            src: source_buffer.handle(),
            src_offset: source_offset as u64,
            bytes_per_row: source_bytes_per_row as u32,
            bytes_per_image: source_bytes_per_image as u32,
            size: size(source_size),
            dst: destination_texture.handle(),
            slice: destination_slice as u32,
            level: destination_level as u16,
            options: options.bits() as u16,
            origin: origin(destination_origin),
        };
        // The plain node's level is 32 bits where the option node's level and options are two 16-bit halves: with
        // no option the two layouts agree (a level is far below 65,536).
        let ty = if options.is_empty() { BLIT_BUFFER_TO_TEXTURE } else { BLIT_BUFFER_TO_TEXTURE_OPTION };
        record(handle_of(self), |s| { s.nodes.push(ty, body); });
    }
    #[allow(clippy::too_many_arguments)]
    unsafe fn copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
        &self,
        source_texture: &ProtocolObject<dyn MTLTexture>,
        source_slice: NSUInteger,
        source_level: NSUInteger,
        source_origin: MTLOrigin,
        source_size: MTLSize,
        destination_buffer: &ProtocolObject<dyn MTLBuffer>,
        destination_offset: NSUInteger,
        destination_bytes_per_row: NSUInteger,
        destination_bytes_per_image: NSUInteger,
    ) {
        // SAFETY: the same call with no option.
        unsafe {
            self.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage_options(
                source_texture, source_slice, source_level, source_origin, source_size, destination_buffer, destination_offset,
                destination_bytes_per_row, destination_bytes_per_image, MTLBlitOption::None,
            );
        }
    }
    #[allow(clippy::too_many_arguments)]
    unsafe fn copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage_options(
        &self,
        source_texture: &ProtocolObject<dyn MTLTexture>,
        source_slice: NSUInteger,
        source_level: NSUInteger,
        source_origin: MTLOrigin,
        source_size: MTLSize,
        destination_buffer: &ProtocolObject<dyn MTLBuffer>,
        destination_offset: NSUInteger,
        destination_bytes_per_row: NSUInteger,
        destination_bytes_per_image: NSUInteger,
        options: MTLBlitOption,
    ) {
        let body = TextureToBuffer {
            src: source_texture.handle(),
            slice: source_slice as u32,
            level: source_level as u16,
            options: options.bits() as u16,
            origin: origin(source_origin),
            size: size(source_size),
            dst: destination_buffer.handle(),
            offset: destination_offset as u64,
            bytes_per_row: destination_bytes_per_row as u32,
            bytes_per_image: destination_bytes_per_image as u32,
        };
        let ty = if options.is_empty() { BLIT_TEXTURE_TO_BUFFER } else { BLIT_TEXTURE_TO_BUFFER_OPTION };
        record(handle_of(self), |s| { s.nodes.push(ty, body); });
    }
    fn generateMipmapsForTexture(&self, texture: &ProtocolObject<dyn MTLTexture>) {
        let texture = texture.handle();
        record(handle_of(self), |s| { s.nodes.push(BLIT_GENERATE_MIPMAPS, SetHandle { handle: texture }); });
    }
    unsafe fn copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
        &self,
        source_buffer: &ProtocolObject<dyn MTLBuffer>,
        source_offset: NSUInteger,
        destination_buffer: &ProtocolObject<dyn MTLBuffer>,
        destination_offset: NSUInteger,
        size: NSUInteger,
    ) {
        let body = BufferToBuffer {
            src: source_buffer.handle(),
            src_offset: source_offset as u64,
            dst: destination_buffer.handle(),
            dst_offset: destination_offset as u64,
            length: size as u64,
        };
        record(handle_of(self), |s| { s.nodes.push(BLIT_BUFFER_TO_BUFFER, body); });
    }
}

/// objc2's `MTLComputeCommandEncoder`.
pub unsafe trait MTLComputeCommandEncoder: MTLCommandEncoder {
    fn setComputePipelineState(&self, state: &ProtocolObject<dyn MTLComputePipelineState>) {
        let pso = state.handle();
        record(handle_of(self), |s| {
            s.nodes.push(COMPUTE_SET_PSO, ComputePso { pso, threadgroup_size: wmt::Size::default() });
        });
    }
    unsafe fn setBytes_length_atIndex(&self, bytes: NonNull<c_void>, length: NSUInteger, index: NSUInteger) {
        let data = bytes_of(bytes, length);
        record(handle_of(self), |s| {
            s.nodes.push_payload(COMPUTE_SET_BYTES, SetBytes { bytes: 0, length: length as u64, index: index as u8 }, 0, data);
        });
    }
    unsafe fn setBuffer_offset_atIndex(&self, buffer: Option<&ProtocolObject<dyn MTLBuffer>>, offset: NSUInteger, index: NSUInteger) {
        let buffer = handle_or_zero(buffer);
        record(handle_of(self), |s| { s.nodes.push(COMPUTE_SET_BUFFER, SetBuffer { buffer, offset: offset as u64, index: index as u8 }); });
    }
    unsafe fn setTexture_atIndex(&self, texture: Option<&ProtocolObject<dyn MTLTexture>>, index: NSUInteger) {
        let texture = handle_or_zero(texture);
        record(handle_of(self), |s| { s.nodes.push(COMPUTE_SET_TEXTURE, SetObject { object: texture, index: index as u8 }); });
    }
    /// winemetal's dispatch takes the threadgroup size from the last pipeline node: the facade gives it there.
    fn dispatchThreadgroups_threadsPerThreadgroup(&self, threadgroups_per_grid: MTLSize, threads_per_threadgroup: MTLSize) {
        let groups = size(threadgroups_per_grid);
        let threads = size(threads_per_threadgroup);
        record(handle_of(self), |s| {
            // The size rides on the pipeline node winemetal reads it from: patch the newest one.
            if let Some(&start) = s.nodes.starts.iter().rev().find(|&&start| s.nodes.words[start] as u16 == COMPUTE_SET_PSO) {
                // SAFETY: the node at `start` is a ComputePso.
                unsafe {
                    let node = s.nodes.words.as_mut_ptr().add(start).cast::<Node<ComputePso>>();
                    (*node).body.threadgroup_size = threads;
                }
            }
            s.nodes.push(COMPUTE_DISPATCH, Dispatch { size: groups });
        });
    }
}

unsafe impl MTLCommandEncoder for ProtocolObject<dyn MTLRenderCommandEncoder> {}
unsafe impl MTLRenderCommandEncoder for ProtocolObject<dyn MTLRenderCommandEncoder> {}
unsafe impl MTLCommandEncoder for ProtocolObject<dyn MTLBlitCommandEncoder> {}
unsafe impl MTLBlitCommandEncoder for ProtocolObject<dyn MTLBlitCommandEncoder> {}
unsafe impl MTLCommandEncoder for ProtocolObject<dyn MTLComputeCommandEncoder> {}
unsafe impl MTLComputeCommandEncoder for ProtocolObject<dyn MTLComputeCommandEncoder> {}
unsafe impl MTLCommandEncoder for ProtocolObject<dyn MTLCommandEncoder> {}

/// An encoder from `slot`: slot 30 (blit) takes the buffer alone, 31 and 32 (compute, render) an argument too.
fn new_encoder<P: ?Sized>(cb: u64, slot: u32, arg: Option<u64>, list_slot: u32) -> Option<Retained<ProtocolObject<P>>> {
    let handle = if let Some(arg) = arg {
        let mut params = wmt::ObjU64ObjRet { handle: cb, arg, ret: 0 };
        // SAFETY: an encoder-creating slot on a live command buffer; the encoder is autoreleased.
        unsafe { table::call(slot, &mut params) };
        params.ret
    } else {
        let mut params = wmt::ObjObjRet { handle: cb, ret: 0 };
        // SAFETY: as above.
        unsafe { table::call(slot, &mut params) };
        params.ret
    };
    let encoder = objc2::from_autoreleased(handle)?;
    let device = meta::read(cb, |e| e.map_or(0, |e| e.device));
    meta::with(handle, |e| e.device = device);
    begin(handle, list_slot);
    Some(encoder)
}

// --- Retirement.

type CompletedBlock = dyn Fn(NonNull<ProtocolObject<dyn MTLCommandBuffer>>);
type PresentedBlock = dyn Fn(NonNull<ProtocolObject<dyn MTLDrawable>>);

enum Handler {
    Completed(Kept<CompletedBlock>),
    /// A drawable's presented handler, run when the buffer presenting `drawable` completes.
    Presented(u64, Kept<PresentedBlock>),
}

/// Handlers registered on command buffers and drawables not yet committed (few a frame).
static PENDING: Mutex<Option<HashMap<u64, Vec<Handler>>>> = Mutex::new(None);

fn pending() -> std::sync::MutexGuard<'static, Option<HashMap<u64, Vec<Handler>>>> {
    PENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn add_pending(handle: u64, handler: Handler) {
    pending().get_or_insert_with(HashMap::new).entry(handle).or_default().push(handler);
}

fn take_pending(handle: u64) -> Vec<Handler> {
    pending().as_mut().and_then(|map| map.remove(&handle)).unwrap_or_default()
}

struct Committed {
    cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    handlers: Vec<Handler>,
}

// SAFETY: the command buffer is a winemetal handle; the handlers are Kept (sendable by construction).
unsafe impl Send for Committed {}

/// The retirement thread of each queue that has had a handler: commits arrive in commit order.
static RETIRERS: Mutex<Option<HashMap<u64, mpsc::Sender<Committed>>>> = Mutex::new(None);

fn retirer(queue: u64) -> mpsc::Sender<Committed> {
    let mut map = RETIRERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    map.get_or_insert_with(HashMap::new)
        .entry(queue)
        .or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<Committed>();
            std::thread::Builder::new()
                .name("mtld3d-mtl-retire".to_owned())
                .spawn(move || {
                    for done in rx {
                        let cb = done.cb.handle();
                        let mut wait = wmt::ObjNoRet { handle: cb };
                        // SAFETY: slot 13 waits on a committed buffer the message holds.
                        unsafe { table::call(wmt::COMMAND_BUFFER_WAIT_UNTIL_COMPLETED, &mut wait) };
                        for handler in done.handlers {
                            match handler {
                                Handler::Completed(h) => (h.0)(NonNull::from(&*done.cb)),
                                Handler::Presented(drawable, h) => {
                                    meta::with(drawable, |e| e.presented_time = now());
                                    // SAFETY: the drawable is kept alive by the presenting buffer's reference below.
                                    (h.0)(NonNull::from(unsafe { ProtocolObject::<dyn MTLDrawable>::from_handle(drawable) }));
                                    release_drawable(drawable);
                                }
                            }
                        }
                    }
                })
                .expect("a retirement thread");
            tx
        })
        .clone()
}

fn now() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    // Never 0: mtld3d reads 0 as "not presented".
    1.0 + START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

fn release_drawable(drawable: u64) {
    meta::release(drawable);
    let mut params = wmt::ObjNoRet { handle: drawable };
    // SAFETY: the reference presentDrawable took for the handler.
    unsafe { table::call(wmt::RELEASE, &mut params) };
}

/// The status a test makes a buffer report (`set_status_for_test`), in place of winemetal's.
static STATUS_OVERRIDE: Mutex<Option<HashMap<u64, u64>>> = Mutex::new(None);

/// Makes `cb` report `status`, for a test of the error path (a real GPU fault isn't deterministic on Apple Silicon).
#[doc(hidden)]
pub fn set_status_for_test(cb: &ProtocolObject<dyn MTLCommandBuffer>, status: MTLCommandBufferStatus) {
    STATUS_OVERRIDE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert_with(HashMap::new).insert(cb.handle(), status.0 as u64);
}

// --- Queues and command buffers.

/// objc2's `MTLCommandQueue`.
pub unsafe trait MTLCommandQueue: NSObjectProtocol {
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn setLabel(&self, label: Option<&NSString>) {
        set_label_of(handle_of(self), label);
    }
    fn device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        device_of(handle_of(self))
    }
    fn commandBuffer(&self) -> Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>> {
        let queue = handle_of(self);
        let mut params = wmt::ObjObjRet { handle: queue, ret: 0 };
        // SAFETY: slot 11 returns an autoreleased command buffer.
        unsafe { table::call(wmt::QUEUE_COMMAND_BUFFER, &mut params) };
        let cb = objc2::from_autoreleased(params.ret)?;
        let device = meta::read(queue, |e| e.map_or(0, |e| e.device));
        meta::with(params.ret, |e| {
            e.device = device;
            e.queue = queue;
        });
        Some(cb)
    }
}

unsafe impl MTLCommandQueue for ProtocolObject<dyn MTLCommandQueue> {}

/// objc2's `MTLCommandBuffer`.
pub unsafe trait MTLCommandBuffer: NSObjectProtocol {
    fn device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        device_of(handle_of(self))
    }
    fn commandQueue(&self) -> Retained<ProtocolObject<dyn MTLCommandQueue>> {
        retained(meta::read(handle_of(self), |e| e.map_or(0, |e| e.queue))).expect("a command buffer's queue")
    }
    fn retainedReferences(&self) -> bool {
        true
    }
    fn errorOptions(&self) -> MTLCommandBufferErrorOption {
        MTLCommandBufferErrorOption::None
    }
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn setLabel(&self, label: Option<&NSString>) {
        set_label_of(handle_of(self), label);
    }
    fn GPUStartTime(&self) -> CFTimeInterval {
        gpu_time(handle_of(self), 0)
    }
    fn GPUEndTime(&self) -> CFTimeInterval {
        gpu_time(handle_of(self), 1)
    }
    /// Commit order is the queue's order already; winemetal has no enqueue.
    fn enqueue(&self) {}
    fn commit(&self) {
        let cb = handle_of(self);
        let mut params = wmt::ObjNoRet { handle: cb };
        // SAFETY: slot 12 commits a live buffer.
        unsafe { table::call(wmt::COMMAND_BUFFER_COMMIT, &mut params) };
        let handlers = take_pending(cb);
        if handlers.is_empty() {
            return;
        }
        let queue = meta::read(cb, |e| e.map_or(0, |e| e.queue));
        let committed = Committed { cb: retained(cb).expect("a committed buffer"), handlers };
        let _ = retirer(queue).send(committed);
    }
    fn presentDrawable(&self, drawable: &ProtocolObject<dyn MTLDrawable>) {
        let mut params = wmt::ObjObjNoRet { handle: handle_of(self), arg: drawable.handle() };
        // SAFETY: slot 47 presents a live drawable.
        unsafe { table::call(wmt::COMMAND_BUFFER_PRESENT_DRAWABLE, &mut params) };
        move_presented(handle_of(self), drawable.handle());
    }
    fn presentDrawable_afterMinimumDuration(&self, drawable: &ProtocolObject<dyn MTLDrawable>, duration: CFTimeInterval) {
        let mut params = wmt::ObjObjDoubleNoRet { handle: handle_of(self), arg0: drawable.handle(), arg1: duration };
        // SAFETY: slot 48 presents a live drawable.
        unsafe { table::call(wmt::COMMAND_BUFFER_PRESENT_DRAWABLE_AFTER_MINIMUM_DURATION, &mut params) };
        move_presented(handle_of(self), drawable.handle());
    }
    unsafe fn addCompletedHandler(&self, block: MTLCommandBufferHandler) {
        // SAFETY: a live block (objc2's contract).
        add_pending(handle_of(self), Handler::Completed(unsafe { Kept::from_block(block) }));
    }
    fn waitUntilCompleted(&self) {
        let mut params = wmt::ObjNoRet { handle: handle_of(self) };
        // SAFETY: slot 13 waits on a live buffer.
        unsafe { table::call(wmt::COMMAND_BUFFER_WAIT_UNTIL_COMPLETED, &mut params) };
    }
    fn status(&self) -> MTLCommandBufferStatus {
        let cb = handle_of(self);
        let forced = STATUS_OVERRIDE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(|m| m.get(&cb).copied());
        MTLCommandBufferStatus(forced.unwrap_or_else(|| super::query_u64(wmt::COMMAND_BUFFER_STATUS, cb)) as usize)
    }
    fn error(&self) -> Option<Retained<NSError>> {
        let mut params = wmt::ObjObjRet { handle: handle_of(self), ret: 0 };
        // SAFETY: slot 89 returns an autoreleased error or 0.
        unsafe { table::call(wmt::COMMAND_BUFFER_ERROR, &mut params) };
        NSError::from_autoreleased(params.ret)
    }
    fn blitCommandEncoder(&self) -> Option<Retained<ProtocolObject<dyn MTLBlitCommandEncoder>>> {
        new_encoder(handle_of(self), wmt::COMMAND_BUFFER_BLIT_COMMAND_ENCODER, None, wmt::BLIT_ENCODE_COMMANDS)
    }
    fn renderCommandEncoderWithDescriptor(
        &self,
        render_pass_descriptor: &MTLRenderPassDescriptor,
    ) -> Option<Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>> {
        let info = render_pass_descriptor.info();
        new_encoder(handle_of(self), wmt::COMMAND_BUFFER_RENDER_COMMAND_ENCODER, Some((&raw const info) as u64), wmt::RENDER_ENCODE_COMMANDS)
    }
    fn computeCommandEncoder(&self) -> Option<Retained<ProtocolObject<dyn MTLComputeCommandEncoder>>> {
        new_encoder(handle_of(self), wmt::COMMAND_BUFFER_COMPUTE_COMMAND_ENCODER, Some(0), wmt::COMPUTE_ENCODE_COMMANDS)
    }
    fn encodeWaitForEvent_value(&self, event: &ProtocolObject<dyn MTLEvent>, value: u64) {
        let mut params = wmt::ObjObjU64NoRet { handle: handle_of(self), arg0: event.handle(), arg1: value };
        // SAFETY: slot 102 on a live buffer and event.
        unsafe { table::call(wmt::COMMAND_BUFFER_ENCODE_WAIT_FOR_EVENT, &mut params) };
    }
    fn pushDebugGroup(&self, _string: &NSString) {}
    fn popDebugGroup(&self) {}
}

unsafe impl MTLCommandBuffer for ProtocolObject<dyn MTLCommandBuffer> {}

/// A command buffer's GPU start (0) or end (1) time in seconds (winemetal's property slot answers nanoseconds).
fn gpu_time(cb: u64, which: u64) -> CFTimeInterval {
    super::query_u64_arg(wmt::COMMAND_BUFFER_PROPERTY, cb, which) as f64 / 1e9
}

/// A drawable's presented handlers go with the buffer that presents it; the buffer holds the drawable for them.
fn move_presented(cb: u64, drawable: u64) {
    let handlers = take_pending(drawable);
    if handlers.is_empty() {
        return;
    }
    for handler in handlers {
        if let Handler::Presented(_, h) = handler {
            meta::retain(drawable);
            let mut params = wmt::ObjNoRet { handle: drawable };
            // SAFETY: kept until the handler has run (release_drawable).
            unsafe { table::call(wmt::RETAIN, &mut params) };
            add_pending(cb, Handler::Presented(drawable, h));
        }
    }
}

/// objc2's `MTLDrawable`.
pub unsafe trait MTLDrawable: NSObjectProtocol {
    /// A drawable is presented through a command buffer here (winemetal's only way).
    fn present(&self) {
        log::warn!(target: "mtld3d::mtl", "MTLDrawable::present without a command buffer: not presented");
    }
    unsafe fn addPresentedHandler(&self, block: MTLDrawablePresentedHandler) {
        // SAFETY: a live block (objc2's contract).
        add_pending(handle_of(self), Handler::Presented(handle_of(self), unsafe { Kept::from_block(block) }));
    }
    fn presentedTime(&self) -> CFTimeInterval {
        meta::read(handle_of(self), |e| e.map_or(0.0, |e| e.presented_time))
    }
}

unsafe impl MTLDrawable for ProtocolObject<dyn MTLDrawable> {}
