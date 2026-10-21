//! objc2-metal's descriptor classes as Rust objects the facade owns.
//!
//! With Metal's defaults, filled into winemetal's info structs when the object is created. A
//! sub-descriptor (an attachment, a vertex attribute or layout) is a view of its parent's state, as
//! Metal's are: setting one sets the parent.

use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::{
    MTLBlendFactor, MTLBlendOperation, MTLBuffer, MTLClearColor, MTLColorWriteMask, MTLCompareFunction, MTLFunction,
    MTLLanguageVersion, MTLLoadAction, MTLMathMode, MTLPixelFormat, MTLSamplerAddressMode, MTLSamplerBorderColor,
    MTLSamplerMinMagFilter, MTLSamplerMipFilter, MTLStencilOperation, MTLStorageMode, MTLStoreAction, MTLTexture,
    MTLTextureSwizzleChannels, MTLTextureType, MTLTextureUsage, MTLVertexFormat, MTLVertexStepFunction,
    MTLResourceStorageModeShift, retained,
};

/// objc2-metal's `MTLMultisampleDepthResolveFilter`.
#[repr(usize)]
#[derive(Debug, PartialEq, Eq, Hash, Default)]
pub enum MTLMultisampleDepthResolveFilter {
    #[default]
    Sample0 = 0,
    Min = 1,
    Max = 2,
}

/// objc2-metal's `MTLMultisampleStencilResolveFilter`.
#[repr(usize)]
#[derive(Debug, PartialEq, Eq, Hash, Default)]
pub enum MTLMultisampleStencilResolveFilter {
    #[default]
    Sample0 = 0,
    DepthResolvedSample = 1,
}

use crate::{
    foundation::{NSString, NSUInteger},
    meta,
    objc2::{Message, Retained, runtime::ProtocolObject},
    wmt,
};

/// A facade-owned object: a Rust count beside its state.
macro_rules! object {
    ($(#[$m:meta])* $name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        $(#[$m])*
        pub struct $name {
            refs: AtomicUsize,
            $(pub $field: $ty,)*
        }

        // SAFETY: a Rust count; the box goes with the last reference.
        unsafe impl Message for $name {
            unsafe fn retain_raw(ptr: *const Self) {
                // SAFETY: a live object.
                unsafe { &*ptr }.refs.fetch_add(1, Ordering::Relaxed);
            }
            unsafe fn release_raw(ptr: *const Self) {
                // SAFETY: a reference given up; the last frees the box `boxed` made.
                unsafe {
                    if (*ptr).refs.fetch_sub(1, Ordering::AcqRel) == 1 {
                        drop(Box::from_raw(ptr.cast_mut()));
                    }
                }
            }
        }

        impl $name {
            fn boxed($($field: $ty),*) -> Retained<Self> {
                let ptr = Box::into_raw(Box::new(Self { refs: AtomicUsize::new(1), $($field,)* }));
                // SAFETY: a new object with its one reference.
                unsafe { Retained::from_raw(ptr).unwrap_unchecked() }
            }
        }
    };
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

type Texture = Retained<ProtocolObject<dyn MTLTexture>>;

fn keep_texture(texture: Option<&ProtocolObject<dyn MTLTexture>>) -> Option<Texture> {
    texture.and_then(|t| retained(t.handle()))
}

// --- Textures.

#[derive(Clone, Copy)]
pub struct TextureDesc {
    texture_type: MTLTextureType,
    pixel_format: MTLPixelFormat,
    width: usize,
    height: usize,
    depth: usize,
    mipmap_level_count: usize,
    sample_count: usize,
    array_length: usize,
    storage_mode: MTLStorageMode,
    usage: MTLTextureUsage,
    swizzle: [u8; 4],
}

impl Default for TextureDesc {
    fn default() -> Self {
        Self {
            texture_type: MTLTextureType::Type2D,
            pixel_format: MTLPixelFormat::RGBA8Unorm,
            width: 1,
            height: 1,
            depth: 1,
            mipmap_level_count: 1,
            sample_count: 1,
            array_length: 1,
            storage_mode: MTLStorageMode::Shared,
            usage: MTLTextureUsage::ShaderRead,
            swizzle: [2, 3, 4, 5],
        }
    }
}

object!(
    /// objc2-metal's `MTLTextureDescriptor`.
    MTLTextureDescriptor { d: Mutex<TextureDesc> }
);

impl MTLTextureDescriptor {
    #[must_use]
    pub fn new() -> Retained<Self> {
        Self::boxed(Mutex::new(TextureDesc::default()))
    }

    /// objc2's `texture2DDescriptorWithPixelFormat_width_height_mipmapped`, unsafe as declared.
    ///
    /// # Safety
    /// objc2's contract (sizes Metal accepts).
    #[must_use]
    pub unsafe fn texture2DDescriptorWithPixelFormat_width_height_mipmapped(
        pixel_format: MTLPixelFormat,
        width: NSUInteger,
        height: NSUInteger,
        mipmapped: bool,
    ) -> Retained<Self> {
        let mips = if mipmapped { usize::BITS as usize - width.max(height).max(1).leading_zeros() as usize } else { 1 };
        Self::boxed(Mutex::new(TextureDesc { pixel_format, width, height, mipmap_level_count: mips, ..TextureDesc::default() }))
    }

    pub fn textureType(&self) -> MTLTextureType { lock(&self.d).texture_type }
    pub fn setTextureType(&self, v: MTLTextureType) { lock(&self.d).texture_type = v; }
    pub fn pixelFormat(&self) -> MTLPixelFormat { lock(&self.d).pixel_format }
    pub fn setPixelFormat(&self, v: MTLPixelFormat) { lock(&self.d).pixel_format = v; }
    pub fn width(&self) -> NSUInteger { lock(&self.d).width }
    /// objc2's `setWidth`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setWidth(&self, v: NSUInteger) { lock(&self.d).width = v; }
    pub fn height(&self) -> NSUInteger { lock(&self.d).height }
    /// objc2's `setHeight`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setHeight(&self, v: NSUInteger) { lock(&self.d).height = v; }
    pub fn depth(&self) -> NSUInteger { lock(&self.d).depth }
    /// objc2's `setDepth`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setDepth(&self, v: NSUInteger) { lock(&self.d).depth = v; }
    pub fn mipmapLevelCount(&self) -> NSUInteger { lock(&self.d).mipmap_level_count }
    /// objc2's `setMipmapLevelCount`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setMipmapLevelCount(&self, v: NSUInteger) { lock(&self.d).mipmap_level_count = v; }
    pub fn sampleCount(&self) -> NSUInteger { lock(&self.d).sample_count }
    /// objc2's `setSampleCount`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setSampleCount(&self, v: NSUInteger) { lock(&self.d).sample_count = v; }
    pub fn arrayLength(&self) -> NSUInteger { lock(&self.d).array_length }
    /// objc2's `setArrayLength`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setArrayLength(&self, v: NSUInteger) { lock(&self.d).array_length = v; }
    pub fn storageMode(&self) -> MTLStorageMode { lock(&self.d).storage_mode }
    pub fn setStorageMode(&self, v: MTLStorageMode) { lock(&self.d).storage_mode = v; }
    pub fn usage(&self) -> MTLTextureUsage { lock(&self.d).usage }
    pub fn setUsage(&self, v: MTLTextureUsage) { lock(&self.d).usage = v; }
    pub fn swizzle(&self) -> MTLTextureSwizzleChannels {
        let s = lock(&self.d).swizzle;
        MTLTextureSwizzleChannels {
            red: super::MTLTextureSwizzle(s[0]),
            green: super::MTLTextureSwizzle(s[1]),
            blue: super::MTLTextureSwizzle(s[2]),
            alpha: super::MTLTextureSwizzle(s[3]),
        }
    }
    pub fn setSwizzle(&self, s: MTLTextureSwizzleChannels) { lock(&self.d).swizzle = [s.red.0, s.green.0, s.blue.0, s.alpha.0]; }

    /// winemetal's info for slot 21, and the facade's record of the texture.
    pub fn info(&self) -> (wmt::TextureInfo, meta::Texture) {
        let d = *lock(&self.d);
        let storage = d.storage_mode.0 as u64;
        let info = wmt::TextureInfo {
            pixel_format: d.pixel_format.0 as u32,
            width: d.width.max(1) as u32,
            height: d.height.max(1) as u32,
            depth: d.depth.max(1) as u32,
            array_length: d.array_length.max(1) as u32,
            packed: wmt::TextureInfo::pack(
                d.texture_type.0 as u32,
                d.mipmap_level_count.max(1) as u32,
                d.sample_count.max(1) as u32,
                d.usage.bits() as u32,
            ),
            options: storage << MTLResourceStorageModeShift,
            flags: 0,
            mach_port: 0,
            gpu_resource_id: 0,
        };
        let texture = meta::Texture {
            texture_type: d.texture_type.0 as u64,
            pixel_format: d.pixel_format.0 as u64,
            width: info.width.into(),
            height: info.height.into(),
            depth: info.depth.into(),
            mipmap_level_count: d.mipmap_level_count.max(1) as u64,
            sample_count: d.sample_count.max(1) as u64,
            array_length: info.array_length.into(),
            usage: d.usage.bits() as u64,
            storage_mode: storage,
            swizzle: d.swizzle,
        };
        (info, texture)
    }
}

// --- Samplers.

pub struct SamplerDesc {
    min: MTLSamplerMinMagFilter,
    mag: MTLSamplerMinMagFilter,
    mip: MTLSamplerMipFilter,
    max_anisotropy: usize,
    s: MTLSamplerAddressMode,
    t: MTLSamplerAddressMode,
    r: MTLSamplerAddressMode,
    border: MTLSamplerBorderColor,
    lod_min: f32,
    lod_max: f32,
    compare: MTLCompareFunction,
    label: Option<String>,
}

object!(
    /// objc2-metal's `MTLSamplerDescriptor`.
    MTLSamplerDescriptor { d: Mutex<SamplerDesc> }
);

impl MTLSamplerDescriptor {
    #[must_use]
    pub fn new() -> Retained<Self> {
        Self::boxed(Mutex::new(SamplerDesc {
            min: MTLSamplerMinMagFilter::Nearest,
            mag: MTLSamplerMinMagFilter::Nearest,
            mip: MTLSamplerMipFilter::NotMipmapped,
            max_anisotropy: 1,
            s: MTLSamplerAddressMode::ClampToEdge,
            t: MTLSamplerAddressMode::ClampToEdge,
            r: MTLSamplerAddressMode::ClampToEdge,
            border: MTLSamplerBorderColor::TransparentBlack,
            lod_min: 0.0,
            lod_max: f32::MAX,
            compare: MTLCompareFunction::Never,
            label: None,
        }))
    }
    pub fn setMinFilter(&self, v: MTLSamplerMinMagFilter) { lock(&self.d).min = v; }
    pub fn setMagFilter(&self, v: MTLSamplerMinMagFilter) { lock(&self.d).mag = v; }
    pub fn setMipFilter(&self, v: MTLSamplerMipFilter) { lock(&self.d).mip = v; }
    pub fn setMaxAnisotropy(&self, v: NSUInteger) { lock(&self.d).max_anisotropy = v; }
    pub fn setSAddressMode(&self, v: MTLSamplerAddressMode) { lock(&self.d).s = v; }
    pub fn setTAddressMode(&self, v: MTLSamplerAddressMode) { lock(&self.d).t = v; }
    pub fn setRAddressMode(&self, v: MTLSamplerAddressMode) { lock(&self.d).r = v; }
    pub fn setBorderColor(&self, v: MTLSamplerBorderColor) { lock(&self.d).border = v; }
    pub fn setLodMinClamp(&self, v: f32) { lock(&self.d).lod_min = v; }
    pub fn setLodMaxClamp(&self, v: f32) { lock(&self.d).lod_max = v; }
    pub fn setCompareFunction(&self, v: MTLCompareFunction) { lock(&self.d).compare = v; }
    pub fn label(&self) -> Option<Retained<NSString>> { lock(&self.d).label.as_deref().map(NSString::from_str) }
    pub fn setLabel(&self, v: Option<&NSString>) { lock(&self.d).label = v.map(NSString::to_rust); }

    pub fn info(&self) -> wmt::SamplerInfo {
        let d = lock(&self.d);
        wmt::SamplerInfo {
            min_filter: d.min.0 as u8,
            mag_filter: d.mag.0 as u8,
            mip_filter: d.mip.0 as u8,
            r_address_mode: d.r.0 as u8,
            s_address_mode: d.s.0 as u8,
            t_address_mode: d.t.0 as u8,
            border_color: d.border.0 as u8,
            compare_function: d.compare.0 as u8,
            lod_min_clamp: d.lod_min,
            lod_max_clamp: d.lod_max,
            max_anisotropy: d.max_anisotropy.max(1) as u32,
            normalized_coords: true,
            lod_average: false,
            support_argument_buffers: false,
            gpu_resource_id: 0,
        }
    }
}

// --- Depth and stencil.

#[derive(Clone, Copy)]
pub struct StencilDesc {
    compare: MTLCompareFunction,
    fail: MTLStencilOperation,
    depth_fail: MTLStencilOperation,
    pass: MTLStencilOperation,
    read_mask: u32,
    write_mask: u32,
}

impl Default for StencilDesc {
    fn default() -> Self {
        Self {
            compare: MTLCompareFunction::Always,
            fail: MTLStencilOperation::Keep,
            depth_fail: MTLStencilOperation::Keep,
            pass: MTLStencilOperation::Keep,
            read_mask: u32::MAX,
            write_mask: u32::MAX,
        }
    }
}

object!(
    /// objc2-metal's `MTLStencilDescriptor`.
    MTLStencilDescriptor { d: Mutex<StencilDesc> }
);

impl MTLStencilDescriptor {
    #[must_use]
    pub fn new() -> Retained<Self> {
        Self::boxed(Mutex::new(StencilDesc::default()))
    }
    pub fn setStencilCompareFunction(&self, v: MTLCompareFunction) { lock(&self.d).compare = v; }
    pub fn setStencilFailureOperation(&self, v: MTLStencilOperation) { lock(&self.d).fail = v; }
    pub fn setDepthFailureOperation(&self, v: MTLStencilOperation) { lock(&self.d).depth_fail = v; }
    pub fn setDepthStencilPassOperation(&self, v: MTLStencilOperation) { lock(&self.d).pass = v; }
    pub fn setReadMask(&self, v: u32) { lock(&self.d).read_mask = v; }
    pub fn setWriteMask(&self, v: u32) { lock(&self.d).write_mask = v; }
}

fn stencil_info(s: Option<StencilDesc>) -> wmt::StencilInfo {
    s.map_or_else(wmt::StencilInfo::default, |s| wmt::StencilInfo {
        enabled: true,
        depth_stencil_pass_op: s.pass.0 as u8,
        stencil_fail_op: s.fail.0 as u8,
        depth_fail_op: s.depth_fail.0 as u8,
        stencil_compare_function: s.compare.0 as u8,
        // Depth32Float_Stencil8 has 8 stencil bits: the rest of D3D9's 32-bit masks reaches nothing.
        write_mask: s.write_mask as u8,
        read_mask: s.read_mask as u8,
    })
}

pub struct DepthStencilDesc {
    compare: MTLCompareFunction,
    write: bool,
    front: Option<StencilDesc>,
    back: Option<StencilDesc>,
    label: Option<String>,
}

object!(
    /// objc2-metal's `MTLDepthStencilDescriptor`.
    MTLDepthStencilDescriptor { d: Mutex<DepthStencilDesc> }
);

impl MTLDepthStencilDescriptor {
    #[must_use]
    pub fn new() -> Retained<Self> {
        Self::boxed(Mutex::new(DepthStencilDesc { compare: MTLCompareFunction::Always, write: false, front: None, back: None, label: None }))
    }
    pub fn setDepthCompareFunction(&self, v: MTLCompareFunction) { lock(&self.d).compare = v; }
    pub fn setDepthWriteEnabled(&self, v: bool) { lock(&self.d).write = v; }
    pub fn setFrontFaceStencil(&self, v: Option<&MTLStencilDescriptor>) { lock(&self.d).front = v.map(|s| *lock(&s.d)); }
    pub fn setBackFaceStencil(&self, v: Option<&MTLStencilDescriptor>) { lock(&self.d).back = v.map(|s| *lock(&s.d)); }
    pub fn label(&self) -> Option<Retained<NSString>> { lock(&self.d).label.as_deref().map(NSString::from_str) }
    pub fn setLabel(&self, v: Option<&NSString>) { lock(&self.d).label = v.map(NSString::to_rust); }

    pub fn info(&self) -> wmt::DepthStencilInfo {
        let d = lock(&self.d);
        wmt::DepthStencilInfo {
            depth_compare_function: d.compare.0 as u8,
            depth_write_enabled: d.write,
            front_stencil: stencil_info(d.front),
            back_stencil: stencil_info(d.back),
        }
    }
}

// --- Compile options.

object!(
    /// objc2-metal's `MTLCompileOptions`.
    MTLCompileOptions { d: Mutex<wmt::CompileOptions> }
);

impl MTLCompileOptions {
    #[must_use]
    pub fn new() -> Retained<Self> {
        // Metal's defaults: the newest language version, no invariance, fast math.
        Self::boxed(Mutex::new(wmt::CompileOptions { language_version: 0, preserve_invariance: 0, math_mode: MTLMathMode::Fast.0 as u8, padding: [0; 6] }))
    }
    pub fn setMathMode(&self, v: MTLMathMode) { lock(&self.d).math_mode = v.0 as u8; }
    pub fn setLanguageVersion(&self, v: MTLLanguageVersion) { lock(&self.d).language_version = v.0 as u64; }
    pub fn setPreserveInvariance(&self, v: bool) { lock(&self.d).preserve_invariance = u8::from(v); }

    pub fn info(&self) -> wmt::CompileOptions {
        *lock(&self.d)
    }
}

// --- Vertex descriptors.

#[derive(Clone, Copy)]
pub struct VertexDesc {
    attributes: [wmt::VertexAttributeInfo; 31],
    layouts: [wmt::VertexBufferLayoutInfo; 31],
}

impl Default for VertexDesc {
    fn default() -> Self {
        Self {
            attributes: [wmt::VertexAttributeInfo::default(); 31],
            layouts: [wmt::VertexBufferLayoutInfo { stride: 0, step_function: MTLVertexStepFunction::PerVertex.0 as u32, step_rate: 1 }; 31],
        }
    }
}

object!(
    /// objc2-metal's `MTLVertexDescriptor`.
    MTLVertexDescriptor { d: Arc<Mutex<VertexDesc>> }
);
object!(
    /// objc2-metal's `MTLVertexAttributeDescriptorArray`.
    MTLVertexAttributeDescriptorArray { d: Arc<Mutex<VertexDesc>> }
);
object!(
    /// objc2-metal's `MTLVertexBufferLayoutDescriptorArray`.
    MTLVertexBufferLayoutDescriptorArray { d: Arc<Mutex<VertexDesc>> }
);
object!(
    /// objc2-metal's `MTLVertexAttributeDescriptor`: attribute `index` of its descriptor.
    MTLVertexAttributeDescriptor { d: Arc<Mutex<VertexDesc>>, index: usize }
);
object!(
    /// objc2-metal's `MTLVertexBufferLayoutDescriptor`: layout `index` of its descriptor.
    MTLVertexBufferLayoutDescriptor { d: Arc<Mutex<VertexDesc>>, index: usize }
);

impl MTLVertexDescriptor {
    #[must_use]
    pub fn new() -> Retained<Self> {
        Self::boxed(Arc::new(Mutex::new(VertexDesc::default())))
    }
    pub fn attributes(&self) -> Retained<MTLVertexAttributeDescriptorArray> {
        MTLVertexAttributeDescriptorArray::boxed(Arc::clone(&self.d))
    }
    pub fn layouts(&self) -> Retained<MTLVertexBufferLayoutDescriptorArray> {
        MTLVertexBufferLayoutDescriptorArray::boxed(Arc::clone(&self.d))
    }
    pub fn reset(&self) {
        *lock(&self.d) = VertexDesc::default();
    }
}

impl MTLVertexAttributeDescriptorArray {
    /// objc2's `objectAtIndexedSubscript`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// `index` < 31.
    pub unsafe fn objectAtIndexedSubscript(&self, index: NSUInteger) -> Retained<MTLVertexAttributeDescriptor> {
        MTLVertexAttributeDescriptor::boxed(Arc::clone(&self.d), index.min(30))
    }
}

impl MTLVertexBufferLayoutDescriptorArray {
    /// objc2's `objectAtIndexedSubscript`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// `index` < 31.
    pub unsafe fn objectAtIndexedSubscript(&self, index: NSUInteger) -> Retained<MTLVertexBufferLayoutDescriptor> {
        MTLVertexBufferLayoutDescriptor::boxed(Arc::clone(&self.d), index.min(30))
    }
}

impl MTLVertexAttributeDescriptor {
    pub fn format(&self) -> MTLVertexFormat { MTLVertexFormat(lock(&self.d).attributes[self.index].format as usize) }
    pub fn setFormat(&self, v: MTLVertexFormat) { lock(&self.d).attributes[self.index].format = v.0 as u32; }
    /// objc2's `setOffset`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setOffset(&self, v: NSUInteger) { lock(&self.d).attributes[self.index].offset = v as u32; }
    /// objc2's `setBufferIndex`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setBufferIndex(&self, v: NSUInteger) { lock(&self.d).attributes[self.index].buffer_index = v as u32; }
}

impl MTLVertexBufferLayoutDescriptor {
    /// objc2's `setStride`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setStride(&self, v: NSUInteger) { lock(&self.d).layouts[self.index].stride = v as u32; }
    pub fn setStepFunction(&self, v: MTLVertexStepFunction) { lock(&self.d).layouts[self.index].step_function = v.0 as u32; }
    /// objc2's `setStepRate`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setStepRate(&self, v: NSUInteger) { lock(&self.d).layouts[self.index].step_rate = v as u32; }
}

// --- Render pipelines.

#[derive(Clone, Copy)]
pub struct ColorBlend {
    pixel_format: MTLPixelFormat,
    blending: bool,
    src_rgb: MTLBlendFactor,
    dst_rgb: MTLBlendFactor,
    rgb_op: MTLBlendOperation,
    src_alpha: MTLBlendFactor,
    dst_alpha: MTLBlendFactor,
    alpha_op: MTLBlendOperation,
    write_mask: MTLColorWriteMask,
}

impl Default for ColorBlend {
    fn default() -> Self {
        Self {
            pixel_format: MTLPixelFormat::Invalid,
            blending: false,
            src_rgb: MTLBlendFactor::One,
            dst_rgb: MTLBlendFactor::Zero,
            rgb_op: MTLBlendOperation::Add,
            src_alpha: MTLBlendFactor::One,
            dst_alpha: MTLBlendFactor::Zero,
            alpha_op: MTLBlendOperation::Add,
            write_mask: MTLColorWriteMask::All,
        }
    }
}

pub struct PipelineDesc {
    label: Option<String>,
    vertex: Option<Retained<ProtocolObject<dyn MTLFunction>>>,
    fragment: Option<Retained<ProtocolObject<dyn MTLFunction>>>,
    vertex_descriptor: Option<VertexDesc>,
    raster_sample_count: usize,
    alpha_to_coverage: bool,
    rasterization: bool,
    colors: [ColorBlend; 8],
    depth_format: MTLPixelFormat,
    stencil_format: MTLPixelFormat,
}

impl Default for PipelineDesc {
    fn default() -> Self {
        Self {
            label: None,
            vertex: None,
            fragment: None,
            vertex_descriptor: None,
            raster_sample_count: 1,
            alpha_to_coverage: false,
            rasterization: true,
            colors: [ColorBlend::default(); 8],
            depth_format: MTLPixelFormat::Invalid,
            stencil_format: MTLPixelFormat::Invalid,
        }
    }
}

object!(
    /// objc2-metal's `MTLRenderPipelineDescriptor`.
    MTLRenderPipelineDescriptor { d: Arc<Mutex<PipelineDesc>> }
);
object!(
    /// objc2-metal's `MTLRenderPipelineColorAttachmentDescriptorArray`.
    MTLRenderPipelineColorAttachmentDescriptorArray { d: Arc<Mutex<PipelineDesc>> }
);
object!(
    /// objc2-metal's `MTLRenderPipelineColorAttachmentDescriptor`.
    ///
    /// Colour attachment `index` of its descriptor.
    MTLRenderPipelineColorAttachmentDescriptor { d: Arc<Mutex<PipelineDesc>>, index: usize }
);

impl MTLRenderPipelineDescriptor {
    #[must_use]
    pub fn new() -> Retained<Self> {
        Self::boxed(Arc::new(Mutex::new(PipelineDesc::default())))
    }
    pub fn label(&self) -> Option<Retained<NSString>> { lock(&self.d).label.as_deref().map(NSString::from_str) }
    pub fn setLabel(&self, v: Option<&NSString>) { lock(&self.d).label = v.map(NSString::to_rust); }
    pub fn setVertexFunction(&self, v: Option<&ProtocolObject<dyn MTLFunction>>) { lock(&self.d).vertex = v.and_then(|f| retained(f.handle())); }
    pub fn setFragmentFunction(&self, v: Option<&ProtocolObject<dyn MTLFunction>>) { lock(&self.d).fragment = v.and_then(|f| retained(f.handle())); }
    pub fn setVertexDescriptor(&self, v: Option<&MTLVertexDescriptor>) { lock(&self.d).vertex_descriptor = v.map(|v| *lock(&v.d)); }
    pub fn sampleCount(&self) -> NSUInteger { lock(&self.d).raster_sample_count }
    pub fn setSampleCount(&self, v: NSUInteger) { lock(&self.d).raster_sample_count = v; }
    pub fn setRasterSampleCount(&self, v: NSUInteger) { lock(&self.d).raster_sample_count = v; }
    pub fn setAlphaToCoverageEnabled(&self, v: bool) { lock(&self.d).alpha_to_coverage = v; }
    pub fn setRasterizationEnabled(&self, v: bool) { lock(&self.d).rasterization = v; }
    pub fn colorAttachments(&self) -> Retained<MTLRenderPipelineColorAttachmentDescriptorArray> {
        MTLRenderPipelineColorAttachmentDescriptorArray::boxed(Arc::clone(&self.d))
    }
    pub fn setDepthAttachmentPixelFormat(&self, v: MTLPixelFormat) { lock(&self.d).depth_format = v; }
    pub fn setStencilAttachmentPixelFormat(&self, v: MTLPixelFormat) { lock(&self.d).stencil_format = v; }
    pub fn reset(&self) {
        *lock(&self.d) = PipelineDesc::default();
    }

    /// winemetal's info for slot 34 (or 156 with the vertex descriptor).
    pub fn info(&self) -> (wmt::RenderPipelineInfo, Option<wmt::VertexDescriptorInfo>) {
        let d = lock(&self.d);
        let mut colors = [wmt::ColorAttachmentBlendInfo::default(); 8];
        for (c, b) in colors.iter_mut().zip(d.colors.iter()) {
            *c = wmt::ColorAttachmentBlendInfo {
                pixel_format: b.pixel_format.0 as u32,
                rgb_blend_operation: b.rgb_op.0 as u8,
                alpha_blend_operation: b.alpha_op.0 as u8,
                src_rgb_blend_factor: b.src_rgb.0 as u8,
                dst_rgb_blend_factor: b.dst_rgb.0 as u8,
                src_alpha_blend_factor: b.src_alpha.0 as u8,
                dst_alpha_blend_factor: b.dst_alpha.0 as u8,
                write_mask: b.write_mask.bits() as u8,
                blending_enabled: b.blending,
            };
        }
        let info = wmt::RenderPipelineInfo {
            colors,
            alpha_to_coverage_enabled: d.alpha_to_coverage,
            logic_operation_enabled: false,
            logic_operation: 0,
            rasterization_enabled: d.rasterization,
            raster_sample_count: d.raster_sample_count.max(1) as u8,
            depth_pixel_format: d.depth_format.0 as u32,
            stencil_pixel_format: d.stencil_format.0 as u32,
            vertex_function: d.vertex.as_ref().map_or(0, |f| f.handle()),
            fragment_function: d.fragment.as_ref().map_or(0, |f| f.handle()),
            immutable_vertex_buffers: 0,
            immutable_fragment_buffers: 0,
            input_primitive_topology: 0,
            tessellation_partition_mode: 0,
            max_tessellation_factor: 16,
            tessellation_output_winding_order: 0,
            tessellation_factor_step: 0,
            binary_archive_for_serialization: 0,
            binary_archives_for_lookup: wmt::Ptr::null(),
            num_binary_archives_for_lookup: 0,
            fail_on_binary_archive_miss: false,
            support_indirect_command_buffers: false,
            padding: [0; 5],
        };
        let vertex = d.vertex_descriptor.map(|v| wmt::VertexDescriptorInfo { attributes: v.attributes, layouts: v.layouts });
        (info, vertex)
    }
}

impl MTLRenderPipelineColorAttachmentDescriptorArray {
    /// objc2's `objectAtIndexedSubscript`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// `index` < 8.
    pub unsafe fn objectAtIndexedSubscript(&self, index: NSUInteger) -> Retained<MTLRenderPipelineColorAttachmentDescriptor> {
        MTLRenderPipelineColorAttachmentDescriptor::boxed(Arc::clone(&self.d), index.min(7))
    }
}

impl MTLRenderPipelineColorAttachmentDescriptor {
    fn with<R>(&self, f: impl FnOnce(&mut ColorBlend) -> R) -> R {
        f(&mut lock(&self.d).colors[self.index])
    }
    pub fn pixelFormat(&self) -> MTLPixelFormat { self.with(|c| c.pixel_format) }
    pub fn setPixelFormat(&self, v: MTLPixelFormat) { self.with(|c| c.pixel_format = v); }
    pub fn setBlendingEnabled(&self, v: bool) { self.with(|c| c.blending = v); }
    pub fn setSourceRGBBlendFactor(&self, v: MTLBlendFactor) { self.with(|c| c.src_rgb = v); }
    pub fn setDestinationRGBBlendFactor(&self, v: MTLBlendFactor) { self.with(|c| c.dst_rgb = v); }
    pub fn setRgbBlendOperation(&self, v: MTLBlendOperation) { self.with(|c| c.rgb_op = v); }
    pub fn setSourceAlphaBlendFactor(&self, v: MTLBlendFactor) { self.with(|c| c.src_alpha = v); }
    pub fn setDestinationAlphaBlendFactor(&self, v: MTLBlendFactor) { self.with(|c| c.dst_alpha = v); }
    pub fn setAlphaBlendOperation(&self, v: MTLBlendOperation) { self.with(|c| c.alpha_op = v); }
    pub fn setWriteMask(&self, v: MTLColorWriteMask) { self.with(|c| c.write_mask = v); }
}

// --- Render passes.

#[derive(Clone, Default)]
pub struct Attachment {
    texture: Option<Texture>,
    level: usize,
    slice: usize,
    depth_plane: usize,
    resolve_texture: Option<Texture>,
    resolve_level: usize,
    resolve_slice: usize,
    resolve_depth_plane: usize,
    load: MTLLoadAction,
    store: MTLStoreAction,
}

pub struct PassDesc {
    colors: [Attachment; 8],
    clear_colors: [MTLClearColor; 8],
    depth: Attachment,
    clear_depth: f64,
    stencil: Attachment,
    clear_stencil: u32,
    visibility: Option<Retained<ProtocolObject<dyn MTLBuffer>>>,
}

impl Default for PassDesc {
    fn default() -> Self {
        // Metal's defaults: colour attachments store, depth and stencil don't; every load is DontCare.
        let color = Attachment { store: MTLStoreAction::Store, ..Attachment::default() };
        Self {
            colors: core::array::from_fn(|_| color.clone()),
            clear_colors: [MTLClearColor { red: 0.0, green: 0.0, blue: 0.0, alpha: 1.0 }; 8],
            depth: Attachment::default(),
            clear_depth: 1.0,
            stencil: Attachment::default(),
            clear_stencil: 0,
            visibility: None,
        }
    }
}

pub enum Which {
    Color(usize),
    Depth,
    Stencil,
}

object!(
    /// objc2-metal's `MTLRenderPassDescriptor`.
    MTLRenderPassDescriptor { d: Arc<Mutex<PassDesc>> }
);
object!(
    /// objc2-metal's `MTLRenderPassColorAttachmentDescriptorArray`.
    MTLRenderPassColorAttachmentDescriptorArray { d: Arc<Mutex<PassDesc>> }
);
object!(
    /// objc2-metal's `MTLRenderPassAttachmentDescriptor` and its colour.
    ///
    /// Depth and stencil subclasses: one attachment of its pass.
    MTLRenderPassAttachmentDescriptor { d: Arc<Mutex<PassDesc>>, which: Which }
);

pub type MTLRenderPassColorAttachmentDescriptor = MTLRenderPassAttachmentDescriptor;
pub type MTLRenderPassDepthAttachmentDescriptor = MTLRenderPassAttachmentDescriptor;
pub type MTLRenderPassStencilAttachmentDescriptor = MTLRenderPassAttachmentDescriptor;

impl MTLRenderPassDescriptor {
    #[must_use]
    pub fn new() -> Retained<Self> {
        Self::boxed(Arc::new(Mutex::new(PassDesc::default())))
    }
    pub fn colorAttachments(&self) -> Retained<MTLRenderPassColorAttachmentDescriptorArray> {
        MTLRenderPassColorAttachmentDescriptorArray::boxed(Arc::clone(&self.d))
    }
    pub fn depthAttachment(&self) -> Retained<MTLRenderPassDepthAttachmentDescriptor> {
        MTLRenderPassAttachmentDescriptor::boxed(Arc::clone(&self.d), Which::Depth)
    }
    pub fn stencilAttachment(&self) -> Retained<MTLRenderPassStencilAttachmentDescriptor> {
        MTLRenderPassAttachmentDescriptor::boxed(Arc::clone(&self.d), Which::Stencil)
    }
    pub fn setVisibilityResultBuffer(&self, v: Option<&ProtocolObject<dyn MTLBuffer>>) {
        lock(&self.d).visibility = v.and_then(|b| retained(b.handle()));
    }

    /// winemetal's info for slot 32.
    pub fn info(&self) -> wmt::RenderPassInfo {
        let d = lock(&self.d);
        let handle = |t: &Option<Texture>| t.as_ref().map_or(0, |t| t.handle());
        let mut info = wmt::RenderPassInfo::default();
        for (i, (c, clear)) in d.colors.iter().zip(d.clear_colors.iter()).enumerate() {
            info.colors[i] = wmt::ColorAttachmentInfo {
                texture: handle(&c.texture),
                load_action: c.load.0 as i32,
                store_action: c.store.0 as i32,
                level: c.level as u16,
                slice: c.slice as u16,
                depth_plane: c.depth_plane as u32,
                clear_color: wmt::ClearColor { r: clear.red, g: clear.green, b: clear.blue, a: clear.alpha },
                resolve_texture: handle(&c.resolve_texture),
                resolve_level: c.resolve_level as u16,
                resolve_slice: c.resolve_slice as u16,
                resolve_depth_plane: c.resolve_depth_plane as u32,
            };
        }
        info.depth = wmt::DepthAttachmentInfo {
            texture: handle(&d.depth.texture),
            load_action: d.depth.load.0 as i32,
            store_action: d.depth.store.0 as i32,
            level: d.depth.level as u16,
            slice: d.depth.slice as u16,
            depth_plane: d.depth.depth_plane as u32,
            clear_depth: d.clear_depth as f32,
        };
        info.stencil = wmt::StencilAttachmentInfo {
            texture: handle(&d.stencil.texture),
            load_action: d.stencil.load.0 as i32,
            store_action: d.stencil.store.0 as i32,
            level: d.stencil.level as u16,
            slice: d.stencil.slice as u16,
            depth_plane: d.stencil.depth_plane as u32,
            clear_stencil: d.clear_stencil as u8,
        };
        info.visibility_buffer = d.visibility.as_ref().map_or(0, |b| b.handle());
        info
    }
}

impl MTLRenderPassColorAttachmentDescriptorArray {
    /// objc2's `objectAtIndexedSubscript`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// `index` < 8.
    pub unsafe fn objectAtIndexedSubscript(&self, index: NSUInteger) -> Retained<MTLRenderPassColorAttachmentDescriptor> {
        MTLRenderPassAttachmentDescriptor::boxed(Arc::clone(&self.d), Which::Color(index.min(7)))
    }
}

impl MTLRenderPassAttachmentDescriptor {
    fn with<R>(&self, f: impl FnOnce(&mut Attachment) -> R) -> R {
        let mut d = lock(&self.d);
        match self.which {
            Which::Color(i) => f(&mut d.colors[i]),
            Which::Depth => f(&mut d.depth),
            Which::Stencil => f(&mut d.stencil),
        }
    }
    pub fn texture(&self) -> Option<Texture> { self.with(|a| a.texture.clone()) }
    pub fn setTexture(&self, v: Option<&ProtocolObject<dyn MTLTexture>>) { let t = keep_texture(v); self.with(|a| a.texture = t); }
    pub fn level(&self) -> NSUInteger { self.with(|a| a.level) }
    pub fn setLevel(&self, v: NSUInteger) { self.with(|a| a.level = v); }
    pub fn slice(&self) -> NSUInteger { self.with(|a| a.slice) }
    pub fn setSlice(&self, v: NSUInteger) { self.with(|a| a.slice = v); }
    pub fn depthPlane(&self) -> NSUInteger { self.with(|a| a.depth_plane) }
    pub fn setDepthPlane(&self, v: NSUInteger) { self.with(|a| a.depth_plane = v); }
    pub fn resolveTexture(&self) -> Option<Texture> { self.with(|a| a.resolve_texture.clone()) }
    pub fn setResolveTexture(&self, v: Option<&ProtocolObject<dyn MTLTexture>>) { let t = keep_texture(v); self.with(|a| a.resolve_texture = t); }
    pub fn resolveLevel(&self) -> NSUInteger { self.with(|a| a.resolve_level) }
    pub fn setResolveLevel(&self, v: NSUInteger) { self.with(|a| a.resolve_level = v); }
    pub fn resolveSlice(&self) -> NSUInteger { self.with(|a| a.resolve_slice) }
    pub fn setResolveSlice(&self, v: NSUInteger) { self.with(|a| a.resolve_slice = v); }
    pub fn resolveDepthPlane(&self) -> NSUInteger { self.with(|a| a.resolve_depth_plane) }
    pub fn setResolveDepthPlane(&self, v: NSUInteger) { self.with(|a| a.resolve_depth_plane = v); }
    pub fn loadAction(&self) -> MTLLoadAction { self.with(|a| a.load) }
    pub fn setLoadAction(&self, v: MTLLoadAction) { self.with(|a| a.load = v); }
    pub fn storeAction(&self) -> MTLStoreAction { self.with(|a| a.store) }
    pub fn setStoreAction(&self, v: MTLStoreAction) { self.with(|a| a.store = v); }
    pub fn clearColor(&self) -> MTLClearColor {
        match self.which { Which::Color(i) => lock(&self.d).clear_colors[i], _ => MTLClearColor::default() }
    }
    pub fn setClearColor(&self, v: MTLClearColor) {
        if let Which::Color(i) = self.which { lock(&self.d).clear_colors[i] = v; }
    }
    pub fn clearDepth(&self) -> f64 { lock(&self.d).clear_depth }
    pub fn setClearDepth(&self, v: f64) { lock(&self.d).clear_depth = v; }
    /// Metal's default (sample 0): mtld3d only reads it.
    pub fn depthResolveFilter(&self) -> MTLMultisampleDepthResolveFilter { MTLMultisampleDepthResolveFilter::Sample0 }
    /// Metal's default (sample 0): mtld3d only reads it.
    pub fn stencilResolveFilter(&self) -> MTLMultisampleStencilResolveFilter { MTLMultisampleStencilResolveFilter::Sample0 }
    pub fn clearStencil(&self) -> u32 { lock(&self.d).clear_stencil }
    pub fn setClearStencil(&self, v: u32) { lock(&self.d).clear_stencil = v; }
}
