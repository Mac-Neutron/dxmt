//! winemetal's parameter and info structs (`winemetal.h`, `winemetal_thunks.h`, `#pragma pack(8)`).
//!
//! The ones the facade passes, laid out as the 64-bit Unix side reads them. Enums are their C base
//! types: `WMTLoadAction`, `WMTStoreAction` and `WMTGPUFamily` have none and are `int`.
//!
//! The same layouts on i686 Linux (old WoW64's Unix side in the VM's guest), whose winemetal.so
//! takes DXMT's i386 ones: a pointer is a [`Ptr`], and a struct the C side aligns to 8 says so
//! (`align(8)`), as this target's `u64` aligns to 4 (MacNeutron's `mtld3d-layout32-check.sh`
//! compares the two builds).


/// A winemetal object.
///
/// On macOS the Objective-C object's address, in the VM's guest a handle the guest picked.
pub type Handle = u64;

/// A pointer in winemetal's structs (`WMTMemoryPointer`).
///
/// 8 bytes on either width, its upper half 0 in a 32-bit process.
#[repr(C)]
pub struct Ptr<T> {
    ptr: *mut T,
    #[cfg(target_pointer_width = "32")]
    high: u32,
}

impl<T> Ptr<T> {
    /// `ptr`, mutable or not.
    #[must_use]
    pub const fn new(ptr: *const T) -> Self {
        Self {
            ptr: ptr.cast_mut(),
            #[cfg(target_pointer_width = "32")]
            high: 0,
        }
    }

    #[must_use]
    pub const fn null() -> Self { Self::new(core::ptr::null()) }

    #[must_use]
    pub const fn get(&self) -> *mut T { self.ptr }
}

// Slots: the entries of winemetal.so's `__wine_unix_call_funcs`, numbered as the VM guest's catalog does.
pub const RETAIN: u32 = 0;
pub const RELEASE: u32 = 1;
pub const NSARRAY_OBJECT: u32 = 2;
pub const NSARRAY_COUNT: u32 = 3;
pub const COPY_ALL_DEVICES: u32 = 4;
pub const DEVICE_RECOMMENDED_MAX_WORKING_SET_SIZE: u32 = 5;
pub const DEVICE_CURRENT_ALLOCATED_SIZE: u32 = 6;
pub const DEVICE_NAME: u32 = 7;
pub const NSSTRING_GET_CSTRING: u32 = 8;
pub const NSSTRING_LENGTH_OF_BYTES: u32 = 27;
pub const DEVICE_NEW_COMMAND_QUEUE: u32 = 9;
pub const AUTORELEASE_POOL_ALLOC_INIT: u32 = 10;
pub const QUEUE_COMMAND_BUFFER: u32 = 11;
pub const COMMAND_BUFFER_COMMIT: u32 = 12;
pub const COMMAND_BUFFER_WAIT_UNTIL_COMPLETED: u32 = 13;
pub const COMMAND_BUFFER_STATUS: u32 = 14;
pub const DEVICE_NEW_SHARED_EVENT: u32 = 15;
pub const DEVICE_NEW_BUFFER: u32 = 18;
pub const DEVICE_NEW_SAMPLER_STATE: u32 = 19;
pub const DEVICE_NEW_DEPTH_STENCIL_STATE: u32 = 20;
pub const DEVICE_NEW_TEXTURE: u32 = 21;
pub const TEXTURE_NEW_TEXTURE_VIEW: u32 = 23;
pub const DEVICE_MIN_LINEAR_TEXTURE_ALIGNMENT: u32 = 24;
pub const LIBRARY_NEW_FUNCTION: u32 = 26;
pub const NSOBJECT_DESCRIPTION: u32 = 28;
pub const DEVICE_NEW_COMPUTE_PIPELINE_STATE: u32 = 29;
pub const COMMAND_BUFFER_BLIT_COMMAND_ENCODER: u32 = 30;
pub const COMMAND_BUFFER_COMPUTE_COMMAND_ENCODER: u32 = 31;
pub const COMMAND_BUFFER_RENDER_COMMAND_ENCODER: u32 = 32;
pub const COMMAND_ENCODER_END_ENCODING: u32 = 33;
pub const DEVICE_NEW_RENDER_PIPELINE_STATE: u32 = 34;
pub const BLIT_ENCODE_COMMANDS: u32 = 36;
pub const COMPUTE_ENCODE_COMMANDS: u32 = 37;
pub const RENDER_ENCODE_COMMANDS: u32 = 38;
pub const TEXTURE_PIXEL_FORMAT: u32 = 39;
pub const TEXTURE_WIDTH: u32 = 40;
pub const TEXTURE_HEIGHT: u32 = 41;
pub const TEXTURE_DEPTH: u32 = 42;
pub const TEXTURE_ARRAY_LENGTH: u32 = 43;
pub const TEXTURE_MIPMAP_LEVEL_COUNT: u32 = 44;
pub const TEXTURE_REPLACE_REGION: u32 = 45;
pub const BUFFER_DID_MODIFY_RANGE: u32 = 46;
pub const COMMAND_BUFFER_PRESENT_DRAWABLE: u32 = 47;
pub const COMMAND_BUFFER_PRESENT_DRAWABLE_AFTER_MINIMUM_DURATION: u32 = 48;
pub const DEVICE_SUPPORTS_FAMILY: u32 = 49;
pub const DEVICE_SUPPORTS_TEXTURE_SAMPLE_COUNT: u32 = 51;
pub const DEVICE_HAS_UNIFIED_MEMORY: u32 = 52;
pub const CAPTURE_MANAGER_SHARED: u32 = 53;
pub const CAPTURE_MANAGER_START_CAPTURE: u32 = 54;
pub const CAPTURE_MANAGER_STOP_CAPTURE: u32 = 55;
pub const DEVICE_NEW_SPATIAL_SCALER: u32 = 57;
pub const COMMAND_BUFFER_ENCODE_SPATIAL_SCALE: u32 = 59;
pub const NSSTRING_ALLOC_INIT: u32 = 61;
pub const DRAWABLE_TEXTURE: u32 = 66;
pub const LAYER_NEXT_DRAWABLE: u32 = 67;
pub const DEVICE_SUPPORTS_FX_SPATIAL_SCALER: u32 = 68;
pub const LAYER_SET_PROPS: u32 = 70;
pub const LAYER_GET_PROPS: u32 = 71;
pub const CREATE_METAL_VIEW_FROM_HWND: u32 = 72;
pub const RELEASE_METAL_VIEW: u32 = 73;
pub const COMMAND_ENCODER_SET_LABEL: u32 = 86;
pub const COMMAND_BUFFER_ERROR: u32 = 89;
pub const LAYER_SET_COLOR_SPACE: u32 = 93;
pub const COMMAND_BUFFER_ENCODE_WAIT_FOR_EVENT: u32 = 102;
pub const SHARED_EVENT_SIGNAL_VALUE: u32 = 103;
pub const DEVICE_REGISTRY_ID: u32 = 125;
pub const COMMAND_BUFFER_PROPERTY: u32 = 130;
pub const DEVICE_NEW_LIBRARY_WITH_SOURCE: u32 = 144;
pub const DEVICE_NEW_RENDER_PIPELINE_STATE_WITH_VERTEX_DESCRIPTOR: u32 = 156;
pub const DEVICE_NEW_LIBRARY_WITH_SOURCE_OPTIONS: u32 = 157;
/// The VM guest's own (DXMT 0071's `WMTMemoryAlloc`/`WMTMemoryFree`).
///
/// Memory both sides map, which a no-copy buffer there must be made over. macOS's table refuses
/// them; they are not in [`SLOTS`].
pub const MEMORY_ALLOC: u32 = 153;
pub const MEMORY_FREE: u32 = 154;

/// Every slot the facade calls.
///
/// With winemetal's function name for it (the table check in tests/objects.rs).
pub const SLOTS: &[(u32, &str)] = &[
    (RETAIN, "_NSObject_retain"),
    (RELEASE, "_NSObject_release"),
    (NSARRAY_OBJECT, "_NSArray_object"),
    (NSARRAY_COUNT, "_NSArray_count"),
    (COPY_ALL_DEVICES, "_MTLCopyAllDevices"),
    (DEVICE_RECOMMENDED_MAX_WORKING_SET_SIZE, "_MTLDevice_recommendedMaxWorkingSetSize"),
    (DEVICE_CURRENT_ALLOCATED_SIZE, "_MTLDevice_currentAllocatedSize"),
    (DEVICE_NAME, "_MTLDevice_name"),
    (NSSTRING_GET_CSTRING, "_NSString_getCString"),
    (NSSTRING_LENGTH_OF_BYTES, "_NSString_lengthOfBytesUsingEncoding"),
    (DEVICE_NEW_COMMAND_QUEUE, "_MTLDevice_newCommandQueue"),
    (AUTORELEASE_POOL_ALLOC_INIT, "_NSAutoreleasePool_alloc_init"),
    (QUEUE_COMMAND_BUFFER, "_MTLCommandQueue_commandBuffer"),
    (COMMAND_BUFFER_COMMIT, "_MTLCommandBuffer_commit"),
    (COMMAND_BUFFER_WAIT_UNTIL_COMPLETED, "_MTLCommandBuffer_waitUntilCompleted"),
    (COMMAND_BUFFER_STATUS, "_MTLCommandBuffer_status"),
    (DEVICE_NEW_SHARED_EVENT, "_MTLDevice_newSharedEvent"),
    (DEVICE_NEW_BUFFER, "_MTLDevice_newBuffer"),
    (DEVICE_NEW_SAMPLER_STATE, "_MTLDevice_newSamplerState"),
    (DEVICE_NEW_DEPTH_STENCIL_STATE, "_MTLDevice_newDepthStencilState"),
    (DEVICE_NEW_TEXTURE, "_MTLDevice_newTexture"),
    (TEXTURE_NEW_TEXTURE_VIEW, "_MTLTexture_newTextureView"),
    (DEVICE_MIN_LINEAR_TEXTURE_ALIGNMENT, "_MTLDevice_minimumLinearTextureAlignmentForPixelFormat"),
    (LIBRARY_NEW_FUNCTION, "_MTLLibrary_newFunction"),
    (NSOBJECT_DESCRIPTION, "_NSObject_description"),
    (DEVICE_NEW_COMPUTE_PIPELINE_STATE, "_MTLDevice_newComputePipelineState"),
    (COMMAND_BUFFER_BLIT_COMMAND_ENCODER, "_MTLCommandBuffer_blitCommandEncoder"),
    (COMMAND_BUFFER_COMPUTE_COMMAND_ENCODER, "_MTLCommandBuffer_computeCommandEncoder"),
    (COMMAND_BUFFER_RENDER_COMMAND_ENCODER, "_MTLCommandBuffer_renderCommandEncoder"),
    (COMMAND_ENCODER_END_ENCODING, "_MTLCommandEncoder_endEncoding"),
    (DEVICE_NEW_RENDER_PIPELINE_STATE, "_MTLDevice_newRenderPipelineState"),
    (BLIT_ENCODE_COMMANDS, "_MTLBlitCommandEncoder_encodeCommands"),
    (COMPUTE_ENCODE_COMMANDS, "_MTLComputeCommandEncoder_encodeCommands"),
    (RENDER_ENCODE_COMMANDS, "_MTLRenderCommandEncoder_encodeCommands"),
    (TEXTURE_PIXEL_FORMAT, "_MTLTexture_pixelFormat"),
    (TEXTURE_WIDTH, "_MTLTexture_width"),
    (TEXTURE_HEIGHT, "_MTLTexture_height"),
    (TEXTURE_DEPTH, "_MTLTexture_depth"),
    (TEXTURE_ARRAY_LENGTH, "_MTLTexture_arrayLength"),
    (TEXTURE_MIPMAP_LEVEL_COUNT, "_MTLTexture_mipmapLevelCount"),
    (TEXTURE_REPLACE_REGION, "_MTLTexture_replaceRegion"),
    (BUFFER_DID_MODIFY_RANGE, "_MTLBuffer_didModifyRange"),
    (COMMAND_BUFFER_PRESENT_DRAWABLE, "_MTLCommandBuffer_presentDrawable"),
    (COMMAND_BUFFER_PRESENT_DRAWABLE_AFTER_MINIMUM_DURATION, "_MTLCommandBuffer_presentDrawableAfterMinimumDuration"),
    (DEVICE_SUPPORTS_FAMILY, "_MTLDevice_supportsFamily"),
    (DEVICE_SUPPORTS_TEXTURE_SAMPLE_COUNT, "_MTLDevice_supportsTextureSampleCount"),
    (DEVICE_HAS_UNIFIED_MEMORY, "_MTLDevice_hasUnifiedMemory"),
    (CAPTURE_MANAGER_SHARED, "_MTLCaptureManager_sharedCaptureManager"),
    (CAPTURE_MANAGER_START_CAPTURE, "_MTLCaptureManager_startCapture"),
    (CAPTURE_MANAGER_STOP_CAPTURE, "_MTLCaptureManager_stopCapture"),
    (DEVICE_NEW_SPATIAL_SCALER, "_MTLDevice_newSpatialScaler"),
    (COMMAND_BUFFER_ENCODE_SPATIAL_SCALE, "_MTLCommandBuffer_encodeSpatialScale"),
    (NSSTRING_ALLOC_INIT, "_NSString_alloc_init"),
    (DRAWABLE_TEXTURE, "_MetalDrawable_texture"),
    (LAYER_NEXT_DRAWABLE, "_MetalLayer_nextDrawable"),
    (DEVICE_SUPPORTS_FX_SPATIAL_SCALER, "_MTLDevice_supportsFXSpatialScaler"),
    (LAYER_SET_PROPS, "_MetalLayer_setProps"),
    (LAYER_GET_PROPS, "_MetalLayer_getProps"),
    (CREATE_METAL_VIEW_FROM_HWND, "_CreateMetalViewFromHWND"),
    (RELEASE_METAL_VIEW, "_ReleaseMetalView"),
    (COMMAND_ENCODER_SET_LABEL, "_MTLCommandEncoder_setLabel"),
    (COMMAND_BUFFER_ERROR, "_MTLCommandBuffer_error"),
    (LAYER_SET_COLOR_SPACE, "_MetalLayer_setColorSpace"),
    (COMMAND_BUFFER_ENCODE_WAIT_FOR_EVENT, "_MTLCommandBuffer_encodeWaitForEvent"),
    (SHARED_EVENT_SIGNAL_VALUE, "_MTLSharedEvent_signalValue"),
    (DEVICE_REGISTRY_ID, "_MTLDevice_registryID"),
    (COMMAND_BUFFER_PROPERTY, "_MTLCommandBuffer_property"),
    (DEVICE_NEW_LIBRARY_WITH_SOURCE, "_MTLDevice_newLibraryWithSource"),
    (DEVICE_NEW_RENDER_PIPELINE_STATE_WITH_VERTEX_DESCRIPTOR, "_MTLDevice_newRenderPipelineStateWithVertexDescriptor"),
    (DEVICE_NEW_LIBRARY_WITH_SOURCE_OPTIONS, "_MTLDevice_newLibraryWithSourceOptions"),
];

// Generic parameter structs.
#[repr(C)]
#[derive(Default)]
pub struct ObjRet { pub ret: Handle }
#[repr(C)]
#[derive(Default)]
pub struct ObjNoRet { pub handle: Handle }
#[repr(C)]
#[derive(Default)]
pub struct ObjObjRet { pub handle: Handle, pub ret: Handle }
#[repr(C)]
#[derive(Default)]
pub struct ObjU64ObjRet { pub handle: Handle, pub arg: u64, pub ret: Handle }
#[repr(C)]
#[derive(Default)]
pub struct ObjU64Ret { pub handle: Handle, pub ret: u64 }
#[repr(C)]
#[derive(Default)]
pub struct ObjU64NoRet { pub handle: Handle, pub arg: u64 }
#[repr(C)]
#[derive(Default)]
pub struct ObjU64U64Ret { pub handle: Handle, pub arg: u64, pub ret: u64 }
#[repr(C)]
#[derive(Default)]
pub struct ObjObjNoRet { pub handle: Handle, pub arg: Handle }
#[repr(C)]
#[derive(Default)]
pub struct ObjObjDoubleNoRet { pub handle: Handle, pub arg0: Handle, pub arg1: f64 }
#[repr(C)]
#[derive(Default)]
pub struct ObjObjU64NoRet { pub handle: Handle, pub arg0: Handle, pub arg1: u64 }
#[repr(C)]
pub struct ObjPtrNoRet { pub handle: Handle, pub arg: Ptr<core::ffi::c_void> }
#[repr(C)]
pub struct Memory { pub length: u64, pub ptr: Ptr<core::ffi::c_void> }
#[repr(C)]
pub struct ObjConstPtrNoRet { pub handle: Handle, pub arg: Ptr<core::ffi::c_void> }
#[repr(C)]
pub struct ObjCmdNoRet { pub encoder: Handle, pub cmd_head: Ptr<core::ffi::c_void> }

#[repr(C, align(8))]
pub struct NSStringGetCString { pub str: Handle, pub buffer_ptr: u64, pub max_length: u64, pub encoding: u64, pub ret: u32 }
pub const UTF8_STRING_ENCODING: u64 = 4;

#[repr(C)]
pub struct NewWithInfo { pub device: Handle, pub info: Ptr<core::ffi::c_void>, pub ret: Handle }
#[repr(C)]
pub struct NewWithConstInfo { pub device: Handle, pub info: Ptr<core::ffi::c_void>, pub ret: Handle }
#[repr(C)]
pub struct NewPipeline { pub device: Handle, pub info: Ptr<core::ffi::c_void>, pub ret_error: Handle, pub ret_pso: Handle }
#[repr(C)]
pub struct NewPipelineVertex {
    pub device: Handle,
    pub info: Ptr<core::ffi::c_void>,
    pub vertex: Ptr<core::ffi::c_void>,
    pub ret_error: Handle,
    pub ret_pso: Handle,
}
#[repr(C)]
pub struct NewLibrarySource { pub device: Handle, pub source: Ptr<u8>, pub length: u64, pub ret_error: Handle, pub ret_library: Handle }
#[repr(C)]
pub struct NewLibrarySourceOptions {
    pub device: Handle,
    pub source: Ptr<u8>,
    pub length: u64,
    pub options: Ptr<CompileOptions>,
    pub ret_error: Handle,
    pub ret_library: Handle,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct CompileOptions { pub language_version: u64, pub preserve_invariance: u8, pub math_mode: u8, pub padding: [u8; 6] }

#[repr(C)]
pub struct BufferInfo { pub length: u64, pub options: u64, pub memory: Ptr<core::ffi::c_void>, pub gpu_address: u64 }

#[repr(C)]
#[derive(Default)]
pub struct SamplerInfo {
    pub min_filter: u8,
    pub mag_filter: u8,
    pub mip_filter: u8,
    pub r_address_mode: u8,
    pub s_address_mode: u8,
    pub t_address_mode: u8,
    pub border_color: u8,
    pub compare_function: u8,
    pub lod_min_clamp: f32,
    pub lod_max_clamp: f32,
    pub max_anisotropy: u32,
    pub normalized_coords: bool,
    pub lod_average: bool,
    pub support_argument_buffers: bool,
    pub gpu_resource_id: u64,
}

#[repr(C)]
#[derive(Default)]
pub struct StencilInfo {
    pub enabled: bool,
    pub depth_stencil_pass_op: u8,
    pub stencil_fail_op: u8,
    pub depth_fail_op: u8,
    pub stencil_compare_function: u8,
    pub write_mask: u8,
    pub read_mask: u8,
}

#[repr(C)]
#[derive(Default)]
pub struct DepthStencilInfo {
    pub depth_compare_function: u8,
    pub depth_write_enabled: bool,
    pub front_stencil: StencilInfo,
    pub back_stencil: StencilInfo,
}

/// `WMTTextureInfo`.
///
/// `packed` holds the four 8-bit bitfields, low byte first (type, mip count, sample count, usage).
#[repr(C)]
#[derive(Default)]
pub struct TextureInfo {
    pub pixel_format: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub array_length: u32,
    pub packed: u32,
    pub options: u64,
    pub flags: u32,
    pub mach_port: u32,
    pub gpu_resource_id: u64,
}

impl TextureInfo {
    #[must_use]
    pub const fn pack(texture_type: u32, mip_count: u32, sample_count: u32, usage: u32) -> u32 {
        (texture_type & 0xff) | ((mip_count & 0xff) << 8) | ((sample_count & 0xff) << 16) | ((usage & 0xff) << 24)
    }
}

#[repr(C)]
#[derive(Default)]
pub struct TextureSwizzleChannels { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }

#[repr(C)]
pub struct NewTextureView {
    pub texture: Handle,
    pub format: u32,
    pub texture_type: u32,
    pub level_start: u16,
    pub level_count: u16,
    pub slice_start: u16,
    pub slice_count: u16,
    pub swizzle: TextureSwizzleChannels,
    pub padding: [u8; 4],
    pub ret: Handle,
    pub gpu_resource_id: u64,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct ColorAttachmentBlendInfo {
    pub pixel_format: u32,
    pub rgb_blend_operation: u8,
    pub alpha_blend_operation: u8,
    pub src_rgb_blend_factor: u8,
    pub dst_rgb_blend_factor: u8,
    pub src_alpha_blend_factor: u8,
    pub dst_alpha_blend_factor: u8,
    pub write_mask: u8,
    pub blending_enabled: bool,
}

/// `WMTRenderPipelineInfo`.
#[repr(C)]
pub struct RenderPipelineInfo {
    pub colors: [ColorAttachmentBlendInfo; 8],
    pub alpha_to_coverage_enabled: bool,
    pub logic_operation_enabled: bool,
    pub logic_operation: u8,
    pub rasterization_enabled: bool,
    pub raster_sample_count: u8,
    pub depth_pixel_format: u32,
    pub stencil_pixel_format: u32,
    pub vertex_function: Handle,
    pub fragment_function: Handle,
    pub immutable_vertex_buffers: u32,
    pub immutable_fragment_buffers: u32,
    pub input_primitive_topology: u32,
    pub tessellation_partition_mode: u8,
    pub max_tessellation_factor: u8,
    pub tessellation_output_winding_order: u8,
    pub tessellation_factor_step: u8,
    pub binary_archive_for_serialization: Handle,
    pub binary_archives_for_lookup: Ptr<core::ffi::c_void>,
    pub num_binary_archives_for_lookup: u8,
    pub fail_on_binary_archive_miss: bool,
    pub support_indirect_command_buffers: bool,
    pub padding: [u8; 5],
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct VertexAttributeInfo { pub format: u32, pub offset: u32, pub buffer_index: u32 }
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct VertexBufferLayoutInfo { pub stride: u32, pub step_function: u32, pub step_rate: u32 }
#[repr(C)]
pub struct VertexDescriptorInfo { pub attributes: [VertexAttributeInfo; 31], pub layouts: [VertexBufferLayoutInfo; 31] }

#[repr(C)]
pub struct ComputePipelineInfo {
    pub compute_function: Handle,
    pub binary_archives_for_lookup: Ptr<core::ffi::c_void>,
    pub binary_archive_for_serialization: Handle,
    pub num_binary_archives_for_lookup: u8,
    pub fail_on_binary_archive_miss: bool,
    pub support_indirect_command_buffers: bool,
    pub tgsize_is_multiple_of_sgwidth: bool,
    pub immutable_buffers: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct ClearColor { pub r: f64, pub g: f64, pub b: f64, pub a: f64 }

#[repr(C)]
#[derive(Default)]
pub struct ColorAttachmentInfo {
    pub texture: Handle,
    pub load_action: i32,
    pub store_action: i32,
    pub level: u16,
    pub slice: u16,
    pub depth_plane: u32,
    pub clear_color: ClearColor,
    pub resolve_texture: Handle,
    pub resolve_level: u16,
    pub resolve_slice: u16,
    pub resolve_depth_plane: u32,
}

#[repr(C, align(8))]
#[derive(Default)]
pub struct DepthAttachmentInfo {
    pub texture: Handle,
    pub load_action: i32,
    pub store_action: i32,
    pub level: u16,
    pub slice: u16,
    pub depth_plane: u32,
    pub clear_depth: f32,
}

#[repr(C, align(8))]
#[derive(Default)]
pub struct StencilAttachmentInfo {
    pub texture: Handle,
    pub load_action: i32,
    pub store_action: i32,
    pub level: u16,
    pub slice: u16,
    pub depth_plane: u32,
    pub clear_stencil: u8,
}

#[repr(C)]
pub struct RenderPassSampleBufferInfo { pub sample_buffer: Handle, pub end_of_fragment_sample_index: u64, pub start_of_vertex_sample_index: u64 }

impl Default for RenderPassSampleBufferInfo {
    fn default() -> Self { Self { sample_buffer: 0, end_of_fragment_sample_index: !0, start_of_vertex_sample_index: !0 } }
}

#[repr(C)]
#[derive(Default)]
pub struct RenderPassInfo {
    pub colors: [ColorAttachmentInfo; 8],
    pub depth: DepthAttachmentInfo,
    pub stencil: StencilAttachmentInfo,
    pub default_raster_sample_count: u8,
    pub render_target_array_length: u8,
    pub tile_width: u8,
    pub tile_height: u8,
    pub render_target_height: u32,
    pub render_target_width: u32,
    pub visibility_accumulate: u8,
    pub visibility_buffer: Handle,
    pub sample_buffers: [RenderPassSampleBufferInfo; 4],
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Origin { pub x: u64, pub y: u64, pub z: u64 }
#[repr(C)]
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct Size { pub width: u64, pub height: u64, pub depth: u64 }

#[repr(C)]
pub struct ReplaceRegion {
    pub texture: Handle,
    pub origin: Origin,
    pub size: Size,
    pub level: u64,
    pub slice: u64,
    pub data: Ptr<core::ffi::c_void>,
    pub bytes_per_row: u64,
    pub bytes_per_image: u64,
}


#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct SpatialScalerInfo {
    pub color_format: u32,
    pub output_format: u32,
    pub input_width: u32,
    pub input_height: u32,
    pub output_width: u32,
    pub output_height: u32,
}
#[repr(C)]
pub struct SpatialScale { pub cmdbuf: Handle, pub scaler: Handle, pub color: Handle, pub output: Handle, pub fence: Handle }

#[repr(C)]
#[derive(Default)]
pub struct CreateMetalView { pub hwnd: u64, pub device: Handle, pub ret_view: Handle, pub ret_layer: Handle }

#[repr(C)]
#[derive(Default)]
pub struct LayerProps {
    pub device: Handle,
    pub contents_scale: f64,
    pub drawable_width: f64,
    pub drawable_height: f64,
    pub opaque: bool,
    pub display_sync_enabled: bool,
    pub framebuffer_only: bool,
    pub pixel_format: u32,
}

// Render command nodes (`wmtcmd_render_*`): each starts with its type, three reserved u16s and the next node's address.
pub const RENDER_SET_VERTEX_BUFFER: u16 = 2;
pub const RENDER_SET_FRAGMENT_BUFFER: u16 = 4;
pub const RENDER_SET_FRAGMENT_TEXTURE: u16 = 10;
pub const RENDER_SET_FRAGMENT_BYTES: u16 = 11;
pub const RENDER_SET_RASTERIZER_STATE: u16 = 12;
pub const RENDER_SET_PSO: u16 = 15;
pub const RENDER_SET_VISIBILITY_MODE: u16 = 18;
pub const RENDER_DRAW: u16 = 19;
pub const RENDER_DRAW_INDEXED: u16 = 20;
pub const RENDER_SET_VIEWPORT: u16 = 35;
pub const RENDER_SET_SCISSOR_RECT: u16 = 36;
pub const RENDER_SET_BLEND_FACTOR: u16 = 43;
pub const RENDER_SET_STENCIL_REF: u16 = 44;
pub const RENDER_SET_DEPTH_STENCIL_STATE: u16 = 45;
pub const RENDER_SET_VERTEX_BYTES: u16 = 46;
pub const RENDER_SET_VERTEX_TEXTURE: u16 = 47;
pub const RENDER_SET_VERTEX_SAMPLER_STATE: u16 = 48;
pub const RENDER_SET_FRAGMENT_SAMPLER_STATE: u16 = 49;
