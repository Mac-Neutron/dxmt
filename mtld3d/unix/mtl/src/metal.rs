//! objc2-metal's protocols and classes, as far as mtld3d uses them, over winemetal's slots.
//!
//! Protocols are traits implemented by `ProtocolObject<dyn Protocol>` (objc2's shape), their
//! methods default bodies over the object's handle. What winemetal has no getter for comes from
//! [`crate::meta`].

#![allow(non_snake_case, clippy::missing_safety_doc, clippy::not_unsafe_ptr_arg_deref)]

mod command;
mod descriptors;
mod types;

use core::{ffi::c_void, ptr::NonNull};
use std::{ffi::CString, sync::LazyLock};

pub use command::*;
pub use descriptors::*;
pub use types::*;

use crate::{
    block2,
    foundation::{NSError, NSRange, NSString, NSUInteger},
    meta,
    objc2::{
        self, Retained,
        runtime::{NSObjectProtocol, ProtocolObject},
    },
    table, wmt,
};

pub type CFTimeInterval = f64;

/// A no-copy buffer's deallocator block.
pub type Deallocator = block2::DynBlock<dyn Fn(NonNull<c_void>, NSUInteger)>;

/// The handle of any facade object reference.
pub fn handle_of<T: ?Sized>(obj: &T) -> u64 {
    core::ptr::from_ref(obj).cast::<u8>() as usize as u64
}

/// `handle`, retained, as `Retained<ProtocolObject<P>>`; `None` for 0.
pub fn retained<P: ?Sized>(handle: u64) -> Option<Retained<ProtocolObject<P>>> {
    if handle == 0 {
        return None;
    }
    // SAFETY: a live object the facade knows.
    unsafe { Retained::retain(handle as usize as *mut ProtocolObject<P>) }
}

/// The device an object was made on (its entry), else the default device.
fn device_of(handle: u64) -> Retained<ProtocolObject<dyn MTLDevice>> {
    let device = meta::read(handle, |e| e.map_or(0, |e| e.device));
    retained(if device == 0 { default_device() } else { device }).expect("a Metal device")
}

fn label_of(handle: u64) -> Option<Retained<NSString>> {
    meta::read(handle, |e| e.and_then(|e| e.label.as_deref().map(NSString::from_str)))
}

/// A label stays in the facade.
///
/// Winemetal would take two slot calls per object for what only a capture shows.
fn set_label_of(handle: u64, label: Option<&NSString>) {
    let label = label.map(NSString::to_rust);
    meta::with(handle, |e| e.label = label);
}

fn query_u64(slot: u32, handle: u64) -> u64 {
    let mut params = wmt::ObjU64Ret { handle, ret: 0 };
    // SAFETY: a getter slot on a live object.
    unsafe { table::call(slot, &mut params) };
    params.ret
}

fn query_u64_arg(slot: u32, handle: u64, arg: u64) -> u64 {
    let mut params = wmt::ObjU64U64Ret { handle, arg, ret: 0 };
    // SAFETY: a query slot on a live object.
    unsafe { table::call(slot, &mut params) };
    params.ret
}

/// Implements the protocols of `$dyn` (and their parents) for `ProtocolObject<$dyn>`.
macro_rules! protocols {
    ($dyn:ty: $($trait:ident),+) => {
        $(unsafe impl $trait for ProtocolObject<$dyn> {})+
    };
}

/// objc2's `MTLResource`.
pub unsafe trait MTLResource: NSObjectProtocol {
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn setLabel(&self, label: Option<&NSString>) {
        set_label_of(handle_of(self), label);
    }
    fn device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        device_of(handle_of(self))
    }
    fn storageMode(&self) -> MTLStorageMode {
        MTLStorageMode(meta::read(handle_of(self), |e| e.map_or(0, |e| e.storage_mode)) as usize)
    }
}

/// objc2's `MTLBuffer`.
pub unsafe trait MTLBuffer: MTLResource {
    fn length(&self) -> NSUInteger {
        meta::read(handle_of(self), |e| e.map_or(0, |e| e.buffer_length)) as usize
    }
    fn contents(&self) -> NonNull<c_void> {
        let contents = meta::read(handle_of(self), |e| e.map_or(0, |e| e.buffer_contents));
        NonNull::new(contents as *mut c_void).unwrap_or(NonNull::dangling())
    }
    fn didModifyRange(&self, range: NSRange) {
        let mut params = wmt::ObjU64U64Ret { handle: handle_of(self), arg: range.location as u64, ret: range.length as u64 };
        // SAFETY: slot 46 on a live buffer; `ret` is its length input.
        unsafe { table::call(wmt::BUFFER_DID_MODIFY_RANGE, &mut params) };
    }
}

fn texture_meta(handle: u64) -> meta::Texture {
    if let Some(t) = meta::read(handle, |e| e.and_then(|e| e.texture)) {
        return t;
    }
    // A texture winemetal made (a drawable's): its sizes from the slots, once; it is a 2D render target.
    let t = meta::Texture {
        texture_type: MTLTextureType::Type2D.0 as u64,
        pixel_format: query_u64(wmt::TEXTURE_PIXEL_FORMAT, handle),
        width: query_u64(wmt::TEXTURE_WIDTH, handle),
        height: query_u64(wmt::TEXTURE_HEIGHT, handle),
        depth: query_u64(wmt::TEXTURE_DEPTH, handle),
        mipmap_level_count: query_u64(wmt::TEXTURE_MIPMAP_LEVEL_COUNT, handle),
        sample_count: 1,
        array_length: query_u64(wmt::TEXTURE_ARRAY_LENGTH, handle),
        usage: MTLTextureUsage::RenderTarget.bits() as u64,
        storage_mode: MTLStorageMode::Private.0 as u64,
        swizzle: IDENTITY_SWIZZLE,
    };
    meta::with(handle, |e| e.texture = Some(t));
    t
}

const IDENTITY_SWIZZLE: [u8; 4] = [2, 3, 4, 5];

/// objc2's `MTLTexture`.
pub unsafe trait MTLTexture: MTLResource {
    fn textureType(&self) -> MTLTextureType {
        MTLTextureType(texture_meta(handle_of(self)).texture_type as usize)
    }
    fn pixelFormat(&self) -> MTLPixelFormat {
        MTLPixelFormat(texture_meta(handle_of(self)).pixel_format as usize)
    }
    fn width(&self) -> NSUInteger {
        texture_meta(handle_of(self)).width as usize
    }
    fn height(&self) -> NSUInteger {
        texture_meta(handle_of(self)).height as usize
    }
    fn depth(&self) -> NSUInteger {
        texture_meta(handle_of(self)).depth as usize
    }
    fn mipmapLevelCount(&self) -> NSUInteger {
        texture_meta(handle_of(self)).mipmap_level_count as usize
    }
    fn sampleCount(&self) -> NSUInteger {
        texture_meta(handle_of(self)).sample_count as usize
    }
    fn arrayLength(&self) -> NSUInteger {
        texture_meta(handle_of(self)).array_length as usize
    }
    fn usage(&self) -> MTLTextureUsage {
        MTLTextureUsage::from_bits_retain(texture_meta(handle_of(self)).usage as usize)
    }
    fn swizzle(&self) -> MTLTextureSwizzleChannels {
        let s = texture_meta(handle_of(self)).swizzle;
        MTLTextureSwizzleChannels {
            red: MTLTextureSwizzle(s[0]),
            green: MTLTextureSwizzle(s[1]),
            blue: MTLTextureSwizzle(s[2]),
            alpha: MTLTextureSwizzle(s[3]),
        }
    }
    unsafe fn replaceRegion_mipmapLevel_slice_withBytes_bytesPerRow_bytesPerImage(
        &self,
        region: MTLRegion,
        level: NSUInteger,
        slice: NSUInteger,
        pixel_bytes: NonNull<c_void>,
        bytes_per_row: NSUInteger,
        bytes_per_image: NSUInteger,
    ) {
        let mut params = wmt::ReplaceRegion {
            texture: handle_of(self),
            origin: wmt::Origin { x: region.origin.x as u64, y: region.origin.y as u64, z: region.origin.z as u64 },
            size: wmt::Size { width: region.size.width as u64, height: region.size.height as u64, depth: region.size.depth as u64 },
            level: level as u64,
            slice: slice as u64,
            data: pixel_bytes.as_ptr(),
            bytes_per_row: bytes_per_row as u64,
            bytes_per_image: bytes_per_image as u64,
        };
        // SAFETY: slot 45 copies from the caller's bytes.
        unsafe { table::call(wmt::TEXTURE_REPLACE_REGION, &mut params) };
    }
    fn newTextureViewWithPixelFormat(&self, pixel_format: MTLPixelFormat) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let t = texture_meta(handle_of(self));
        texture_view(
            handle_of(self),
            pixel_format,
            MTLTextureType(t.texture_type as usize),
            NSRange::new(0, t.mipmap_level_count as usize),
            NSRange::new(0, slices_of(&t)),
            None,
        )
    }
    unsafe fn newTextureViewWithPixelFormat_textureType_levels_slices(
        &self,
        pixel_format: MTLPixelFormat,
        texture_type: MTLTextureType,
        level_range: NSRange,
        slice_range: NSRange,
    ) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        texture_view(handle_of(self), pixel_format, texture_type, level_range, slice_range, None)
    }
    unsafe fn newTextureViewWithPixelFormat_textureType_levels_slices_swizzle(
        &self,
        pixel_format: MTLPixelFormat,
        texture_type: MTLTextureType,
        level_range: NSRange,
        slice_range: NSRange,
        swizzle: MTLTextureSwizzleChannels,
    ) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        texture_view(handle_of(self), pixel_format, texture_type, level_range, slice_range, Some(swizzle))
    }
}

/// A texture's slice count as Metal counts it for a view: array length, times 6 for a cube.
fn slices_of(t: &meta::Texture) -> usize {
    let cube = t.texture_type == MTLTextureType::TypeCube.0 as u64 || t.texture_type == MTLTextureType::TypeCubeArray.0 as u64;
    (t.array_length.max(1) * if cube { 6 } else { 1 }) as usize
}

fn texture_view(
    source: u64,
    format: MTLPixelFormat,
    texture_type: MTLTextureType,
    levels: NSRange,
    slices: NSRange,
    swizzle: Option<MTLTextureSwizzleChannels>,
) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
    let s = swizzle.map_or(IDENTITY_SWIZZLE, |s| [s.red.0, s.green.0, s.blue.0, s.alpha.0]);
    let mut params = wmt::NewTextureView {
        texture: source,
        format: format.0 as u32,
        texture_type: texture_type.0 as u32,
        level_start: levels.location as u16,
        level_count: levels.length as u16,
        slice_start: slices.location as u16,
        slice_count: slices.length as u16,
        swizzle: wmt::TextureSwizzleChannels { r: s[0], g: s[1], b: s[2], a: s[3] },
        ret: 0,
        gpu_resource_id: 0,
    };
    // SAFETY: slot 23 on a live texture.
    unsafe { table::call(wmt::TEXTURE_NEW_TEXTURE_VIEW, &mut params) };
    let view = objc2::from_plus_one(params.ret)?;
    let base = texture_meta(source);
    let device = meta::read(source, |e| e.map_or(0, |e| e.device));
    meta::with(params.ret, |e| {
        e.device = device;
        e.storage_mode = base.storage_mode;
        e.texture = Some(meta::Texture {
            texture_type: texture_type.0 as u64,
            pixel_format: format.0 as u64,
            width: (base.width >> levels.location).max(1),
            height: (base.height >> levels.location).max(1),
            depth: (base.depth >> levels.location).max(1),
            mipmap_level_count: levels.length as u64,
            sample_count: base.sample_count,
            array_length: if texture_type == MTLTextureType::TypeCube { 1 } else { slices.length as u64 },
            usage: base.usage,
            storage_mode: base.storage_mode,
            swizzle: s,
        });
    });
    Some(view)
}

/// objc2's `MTLSamplerState`.
pub unsafe trait MTLSamplerState: NSObjectProtocol {
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        device_of(handle_of(self))
    }
}

/// objc2's `MTLDepthStencilState`.
pub unsafe trait MTLDepthStencilState: NSObjectProtocol {
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        device_of(handle_of(self))
    }
}

/// objc2's `MTLRenderPipelineState`.
pub unsafe trait MTLRenderPipelineState: NSObjectProtocol {
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        device_of(handle_of(self))
    }
}

/// objc2's `MTLComputePipelineState`.
pub unsafe trait MTLComputePipelineState: NSObjectProtocol {
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        device_of(handle_of(self))
    }
}

/// objc2's `MTLFunction`.
pub unsafe trait MTLFunction: NSObjectProtocol {
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn setLabel(&self, label: Option<&NSString>) {
        set_label_of(handle_of(self), label);
    }
    fn device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        device_of(handle_of(self))
    }
    fn name(&self) -> Retained<NSString> {
        NSString::from_str(&meta::read(handle_of(self), |e| e.and_then(|e| e.name.clone())).unwrap_or_default())
    }
}

/// objc2's `MTLLibrary`.
pub unsafe trait MTLLibrary: NSObjectProtocol {
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn setLabel(&self, label: Option<&NSString>) {
        set_label_of(handle_of(self), label);
    }
    fn device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        device_of(handle_of(self))
    }
    fn newFunctionWithName(&self, function_name: &NSString) -> Option<Retained<ProtocolObject<dyn MTLFunction>>> {
        let name = function_name.to_rust();
        let c_name = CString::new(name.clone()).ok()?;
        let mut params = wmt::ObjU64ObjRet { handle: handle_of(self), arg: c_name.as_ptr() as u64, ret: 0 };
        // SAFETY: slot 26 reads the NUL-terminated name.
        unsafe { table::call(wmt::LIBRARY_NEW_FUNCTION, &mut params) };
        let function = objc2::from_plus_one(params.ret)?;
        let device = meta::read(handle_of(self), |e| e.map_or(0, |e| e.device));
        meta::with(params.ret, |e| {
            e.device = device;
            e.name = Some(name);
        });
        Some(function)
    }
}

/// objc2's `MTLEvent`.
pub unsafe trait MTLEvent: NSObjectProtocol {
    fn device(&self) -> Option<Retained<ProtocolObject<dyn MTLDevice>>> {
        Some(device_of(handle_of(self)))
    }
    fn label(&self) -> Option<Retained<NSString>> {
        label_of(handle_of(self))
    }
    fn setLabel(&self, label: Option<&NSString>) {
        set_label_of(handle_of(self), label);
    }
}

/// objc2's `MTLSharedEvent`.
pub unsafe trait MTLSharedEvent: MTLEvent {
    fn setSignaledValue(&self, signaled_value: u64) {
        let mut params = wmt::ObjU64NoRet { handle: handle_of(self), arg: signaled_value };
        // SAFETY: slot 103 on a live shared event.
        unsafe { table::call(wmt::SHARED_EVENT_SIGNAL_VALUE, &mut params) };
    }
}

protocols!(dyn MTLBuffer: MTLResource, MTLBuffer);
protocols!(dyn MTLTexture: MTLResource, MTLTexture);
protocols!(dyn MTLResource: MTLResource);
protocols!(dyn MTLSamplerState: MTLSamplerState);
protocols!(dyn MTLDepthStencilState: MTLDepthStencilState);
protocols!(dyn MTLRenderPipelineState: MTLRenderPipelineState);
protocols!(dyn MTLComputePipelineState: MTLComputePipelineState);
protocols!(dyn MTLFunction: MTLFunction);
protocols!(dyn MTLLibrary: MTLLibrary);
protocols!(dyn MTLEvent: MTLEvent);
protocols!(dyn MTLSharedEvent: MTLEvent, MTLSharedEvent);

/// The process's device.
///
/// Winemetal's first, retained for good (mtld3d asks for it often; one device on Apple Silicon).
fn default_device() -> u64 {
    static DEVICE: LazyLock<u64> = LazyLock::new(first_device);
    *DEVICE
}

fn first_device() -> u64 {
    if table::Table::get().is_err() {
        return 0;
    }
    let mut all = wmt::ObjRet::default();
    // SAFETY: slot 4 returns a +1 array of devices.
    unsafe { table::call(wmt::COPY_ALL_DEVICES, &mut all) };
    if all.ret == 0 {
        return 0;
    }
    let device = if query_u64(wmt::NSARRAY_COUNT, all.ret) == 0 {
        0
    } else {
        let mut object = wmt::ObjU64ObjRet { handle: all.ret, arg: 0, ret: 0 };
        // SAFETY: slot 2 returns the array's element 0 (+0).
        unsafe { table::call(wmt::NSARRAY_OBJECT, &mut object) };
        let mut retain = wmt::ObjNoRet { handle: object.ret };
        // SAFETY: kept for the process's life.
        unsafe { table::call(wmt::RETAIN, &mut retain) };
        meta::adopt(object.ret);
        object.ret
    };
    let mut release = wmt::ObjNoRet { handle: all.ret };
    // SAFETY: the array's +1 given up.
    unsafe { table::call(wmt::RELEASE, &mut release) };
    device
}

/// objc2's `MTLCreateSystemDefaultDevice`.
///
/// Winemetal's first device; `None` when winemetal can't be loaded (the log says why).
#[must_use]
pub fn MTLCreateSystemDefaultDevice() -> Option<Retained<ProtocolObject<dyn MTLDevice>>> {
    if let Err(e) = table::Table::get() {
        log::error!(target: "mtld3d::mtl", "{e}");
        return None;
    }
    retained(default_device())
}

fn new_buffer(device: u64, length: usize, options: MTLResourceOptions, memory: *mut c_void) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
    let mut info = wmt::BufferInfo { length: length as u64, options: options.bits() as u64, memory, gpu_address: 0 };
    let mut params = wmt::NewWithInfo { device, info: (&raw mut info).cast(), ret: 0 };
    // SAFETY: slot 18 fills the info's memory and GPU address.
    unsafe { table::call(wmt::DEVICE_NEW_BUFFER, &mut params) };
    let buffer = objc2::from_plus_one(params.ret)?;
    meta::with(params.ret, |e| {
        e.device = device;
        e.buffer_length = length as u64;
        e.buffer_contents = info.memory as usize;
        e.storage_mode = ((options.bits() >> MTLResourceStorageModeShift) & 0xf) as u64;
    });
    Some(buffer)
}

/// objc2's `MTLDevice`.
pub unsafe trait MTLDevice: NSObjectProtocol {
    fn name(&self) -> Retained<NSString> {
        let mut params = wmt::ObjObjRet { handle: handle_of(self), ret: 0 };
        // SAFETY: slot 7 returns an autoreleased string.
        unsafe { table::call(wmt::DEVICE_NAME, &mut params) };
        NSString::from_autoreleased(params.ret).unwrap_or_else(|| NSString::from_str(""))
    }
    fn registryID(&self) -> u64 {
        query_u64(wmt::DEVICE_REGISTRY_ID, handle_of(self))
    }
    fn hasUnifiedMemory(&self) -> bool {
        query_u64(wmt::DEVICE_HAS_UNIFIED_MEMORY, handle_of(self)) != 0
    }
    fn recommendedMaxWorkingSetSize(&self) -> u64 {
        query_u64(wmt::DEVICE_RECOMMENDED_MAX_WORKING_SET_SIZE, handle_of(self))
    }
    /// Metal answers it on Apple7 and Mac2 GPUs, every Apple Silicon Mac.
    ///
    /// Winemetal has no query of its own.
    fn supports32BitFloatFiltering(&self) -> bool {
        self.supportsFamily(MTLGPUFamily::Apple7) || self.supportsFamily(MTLGPUFamily::Mac2)
    }
    fn currentAllocatedSize(&self) -> NSUInteger {
        query_u64(wmt::DEVICE_CURRENT_ALLOCATED_SIZE, handle_of(self)) as usize
    }
    fn newCommandQueue(&self) -> Option<Retained<ProtocolObject<dyn MTLCommandQueue>>> {
        // Metal's default maximum of uncompleted command buffers is 64.
        let mut params = wmt::ObjU64ObjRet { handle: handle_of(self), arg: 64, ret: 0 };
        // SAFETY: slot 9 returns a +1 queue.
        unsafe { table::call(wmt::DEVICE_NEW_COMMAND_QUEUE, &mut params) };
        let queue = objc2::from_plus_one(params.ret)?;
        let device = handle_of(self);
        meta::with(params.ret, |e| e.device = device);
        Some(queue)
    }
    fn newBufferWithLength_options(&self, length: NSUInteger, options: MTLResourceOptions) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        new_buffer(handle_of(self), length, options, core::ptr::null_mut())
    }
    unsafe fn newBufferWithBytesNoCopy_length_options_deallocator(
        &self,
        pointer: NonNull<c_void>,
        length: NSUInteger,
        options: MTLResourceOptions,
        deallocator: Option<&Deallocator>,
    ) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        debug_assert!(deallocator.is_none(), "a no-copy buffer's deallocator: winemetal runs none");
        new_buffer(handle_of(self), length, options, pointer.as_ptr())
    }
    fn newDepthStencilStateWithDescriptor(&self, descriptor: &MTLDepthStencilDescriptor) -> Option<Retained<ProtocolObject<dyn MTLDepthStencilState>>> {
        let info = descriptor.info();
        let mut params = wmt::NewWithConstInfo { device: handle_of(self), info: (&raw const info).cast(), ret: 0 };
        // SAFETY: slot 20 reads the info.
        unsafe { table::call(wmt::DEVICE_NEW_DEPTH_STENCIL_STATE, &mut params) };
        let state = objc2::from_plus_one(params.ret)?;
        let device = handle_of(self);
        meta::with(params.ret, |e| e.device = device);
        Some(state)
    }
    fn newTextureWithDescriptor(&self, descriptor: &MTLTextureDescriptor) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let (mut info, texture) = descriptor.info();
        let mut params = wmt::NewWithInfo { device: handle_of(self), info: (&raw mut info).cast(), ret: 0 };
        // SAFETY: slot 21 reads the info and fills its GPU resource id.
        unsafe { table::call(wmt::DEVICE_NEW_TEXTURE, &mut params) };
        let created = objc2::from_plus_one(params.ret)?;
        let device = handle_of(self);
        meta::with(params.ret, |e| {
            e.device = device;
            e.storage_mode = texture.storage_mode;
            e.texture = Some(texture);
        });
        Some(created)
    }
    fn newSamplerStateWithDescriptor(&self, descriptor: &MTLSamplerDescriptor) -> Option<Retained<ProtocolObject<dyn MTLSamplerState>>> {
        let mut info = descriptor.info();
        let mut params = wmt::NewWithInfo { device: handle_of(self), info: (&raw mut info).cast(), ret: 0 };
        // SAFETY: slot 19 reads the info and fills its GPU resource id.
        unsafe { table::call(wmt::DEVICE_NEW_SAMPLER_STATE, &mut params) };
        let sampler = objc2::from_plus_one(params.ret)?;
        let device = handle_of(self);
        meta::with(params.ret, |e| e.device = device);
        Some(sampler)
    }
    fn newLibraryWithSource_options_error(
        &self,
        source: &NSString,
        options: Option<&MTLCompileOptions>,
    ) -> Result<Retained<ProtocolObject<dyn MTLLibrary>>, Retained<NSError>> {
        let text = source.to_rust();
        let device = handle_of(self);
        let (library, error) = if let Some(options) = options {
            let opts = options.info();
            let mut params = wmt::NewLibrarySourceOptions {
                device,
                source: text.as_ptr(),
                length: text.len() as u64,
                options: &raw const opts,
                ret_error: 0,
                ret_library: 0,
            };
            // SAFETY: slot 157 reads the source and options.
            unsafe { table::call(wmt::DEVICE_NEW_LIBRARY_WITH_SOURCE_OPTIONS, &mut params) };
            (params.ret_library, params.ret_error)
        } else {
            let mut params = wmt::NewLibrarySource { device, source: text.as_ptr(), length: text.len() as u64, ret_error: 0, ret_library: 0 };
            // SAFETY: slot 144 reads the source.
            unsafe { table::call(wmt::DEVICE_NEW_LIBRARY_WITH_SOURCE, &mut params) };
            (params.ret_library, params.ret_error)
        };
        match objc2::from_plus_one(library) {
            Some(library) => {
                meta::with(handle_of(&*library), |e| e.device = device);
                Ok(library)
            }
            None => Err(NSError::from_autoreleased_or(error, "winemetal returned no object and no error")),
        }
    }
    fn newRenderPipelineStateWithDescriptor_error(
        &self,
        descriptor: &MTLRenderPipelineDescriptor,
    ) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>, Retained<NSError>> {
        let (info, vertex) = descriptor.info();
        let device = handle_of(self);
        let (pso, error) = if let Some(vertex) = vertex {
            let mut params = wmt::NewPipelineVertex {
                device,
                info: (&raw const info).cast(),
                vertex: (&raw const vertex).cast(),
                ret_error: 0,
                ret_pso: 0,
            };
            // SAFETY: slot 156 reads the infos.
            unsafe { table::call(wmt::DEVICE_NEW_RENDER_PIPELINE_STATE_WITH_VERTEX_DESCRIPTOR, &mut params) };
            (params.ret_pso, params.ret_error)
        } else {
            let mut params = wmt::NewPipeline { device, info: (&raw const info).cast(), ret_error: 0, ret_pso: 0 };
            // SAFETY: slot 34 reads the info.
            unsafe { table::call(wmt::DEVICE_NEW_RENDER_PIPELINE_STATE, &mut params) };
            (params.ret_pso, params.ret_error)
        };
        match objc2::from_plus_one(pso) {
            Some(pso) => {
                meta::with(handle_of(&*pso), |e| {
                    e.device = device;
                    e.label = descriptor.label().map(|l| l.to_rust());
                });
                Ok(pso)
            }
            None => Err(NSError::from_autoreleased_or(error, "winemetal returned no object and no error")),
        }
    }
    fn newComputePipelineStateWithFunction_error(
        &self,
        compute_function: &ProtocolObject<dyn MTLFunction>,
    ) -> Result<Retained<ProtocolObject<dyn MTLComputePipelineState>>, Retained<NSError>> {
        let info = wmt::ComputePipelineInfo {
            compute_function: compute_function.handle(),
            binary_archives_for_lookup: core::ptr::null(),
            binary_archive_for_serialization: 0,
            num_binary_archives_for_lookup: 0,
            fail_on_binary_archive_miss: false,
            support_indirect_command_buffers: false,
            tgsize_is_multiple_of_sgwidth: false,
            immutable_buffers: 0,
        };
        let device = handle_of(self);
        let mut params = wmt::NewPipeline { device, info: (&raw const info).cast(), ret_error: 0, ret_pso: 0 };
        // SAFETY: slot 29 reads the info.
        unsafe { table::call(wmt::DEVICE_NEW_COMPUTE_PIPELINE_STATE, &mut params) };
        match objc2::from_plus_one(params.ret_pso) {
            Some(pso) => {
                meta::with(params.ret_pso, |e| e.device = device);
                Ok(pso)
            }
            None => Err(NSError::from_autoreleased_or(params.ret_error, "winemetal returned no object and no error")),
        }
    }
    fn supportsFamily(&self, gpu_family: MTLGPUFamily) -> bool {
        query_u64_arg(wmt::DEVICE_SUPPORTS_FAMILY, handle_of(self), gpu_family.0 as u64) != 0
    }
    fn supportsTextureSampleCount(&self, sample_count: NSUInteger) -> bool {
        query_u64_arg(wmt::DEVICE_SUPPORTS_TEXTURE_SAMPLE_COUNT, handle_of(self), sample_count as u64) != 0
    }
    fn minimumLinearTextureAlignmentForPixelFormat(&self, format: MTLPixelFormat) -> NSUInteger {
        query_u64_arg(wmt::DEVICE_MIN_LINEAR_TEXTURE_ALIGNMENT, handle_of(self), format.0 as u64) as usize
    }
    fn newSharedEvent(&self) -> Option<Retained<ProtocolObject<dyn MTLSharedEvent>>> {
        let mut params = wmt::ObjObjRet { handle: handle_of(self), ret: 0 };
        // SAFETY: slot 15 returns a +1 shared event.
        unsafe { table::call(wmt::DEVICE_NEW_SHARED_EVENT, &mut params) };
        let event = objc2::from_plus_one(params.ret)?;
        let device = handle_of(self);
        meta::with(params.ret, |e| e.device = device);
        Some(event)
    }
}

protocols!(dyn MTLDevice: MTLDevice);
