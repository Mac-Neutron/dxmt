//! Metal's enums, option sets and plain structs, copied from objc2-metal 0.3.2 and objc2-metal-fx 0.3.2's generated
//! bindings (their values are Metal's, and mtld3d matches on them).
//!
//! objc2-metal and objc2-metal-fx are the objc2 project's (github.com/madsmtm/objc2), offered under Zlib, Apache-2.0
//! or MIT; these copies are taken under Zlib, mtld3d's own licence, and altered only by the removal of their
//! documentation and platform attributes.

#![allow(non_upper_case_globals, clippy::all)]

use core::ffi::c_double;

use crate::foundation::{NSInteger, NSUInteger};

pub const MTLResourceCPUCacheModeShift: NSUInteger = 0;
pub const MTLResourceStorageModeShift: NSUInteger = 4;
pub const MTLResourceHazardTrackingModeShift: NSUInteger = 8;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLBlendFactor(pub NSUInteger);

impl MTLBlendFactor {
    pub const Zero: Self = Self(0);
    pub const One: Self = Self(1);
    pub const SourceColor: Self = Self(2);
    pub const OneMinusSourceColor: Self = Self(3);
    pub const SourceAlpha: Self = Self(4);
    pub const OneMinusSourceAlpha: Self = Self(5);
    pub const DestinationColor: Self = Self(6);
    pub const OneMinusDestinationColor: Self = Self(7);
    pub const DestinationAlpha: Self = Self(8);
    pub const OneMinusDestinationAlpha: Self = Self(9);
    pub const SourceAlphaSaturated: Self = Self(10);
    pub const BlendColor: Self = Self(11);
    pub const OneMinusBlendColor: Self = Self(12);
    pub const BlendAlpha: Self = Self(13);
    pub const OneMinusBlendAlpha: Self = Self(14);
    pub const Source1Color: Self = Self(15);
    pub const OneMinusSource1Color: Self = Self(16);
    pub const Source1Alpha: Self = Self(17);
    pub const OneMinusSource1Alpha: Self = Self(18);
    pub const Unspecialized: Self = Self(19);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLBlendOperation(pub NSUInteger);

impl MTLBlendOperation {
    pub const Add: Self = Self(0);
    pub const Subtract: Self = Self(1);
    pub const ReverseSubtract: Self = Self(2);
    pub const Min: Self = Self(3);
    pub const Max: Self = Self(4);
    pub const Unspecialized: Self = Self(5);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLBlitOption(pub NSUInteger);

bitflags::bitflags! {
    impl MTLBlitOption: NSUInteger {
        const None = 0;
        const DepthFromDepthStencil = 1<<0;
        const StencilFromDepthStencil = 1<<1;
        const RowLinearPVRTC = 1<<2;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MTLClearColor {
    pub red: c_double,
    pub green: c_double,
    pub blue: c_double,
    pub alpha: c_double,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLColorWriteMask(pub NSUInteger);

bitflags::bitflags! {
    impl MTLColorWriteMask: NSUInteger {
        const None = 0;
        const Red = 0x1<<3;
        const Green = 0x1<<2;
        const Blue = 0x1<<1;
        const Alpha = 0x1<<0;
        const All = 0xf;
        const Unspecialized = 0x10;
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLCommandBufferStatus(pub NSUInteger);

impl MTLCommandBufferStatus {
    pub const NotEnqueued: Self = Self(0);
    pub const Enqueued: Self = Self(1);
    pub const Committed: Self = Self(2);
    pub const Scheduled: Self = Self(3);
    pub const Completed: Self = Self(4);
    pub const Error: Self = Self(5);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLCompareFunction(pub NSUInteger);

impl MTLCompareFunction {
    pub const Never: Self = Self(0);
    pub const Less: Self = Self(1);
    pub const Equal: Self = Self(2);
    pub const LessEqual: Self = Self(3);
    pub const Greater: Self = Self(4);
    pub const NotEqual: Self = Self(5);
    pub const GreaterEqual: Self = Self(6);
    pub const Always: Self = Self(7);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLCullMode(pub NSUInteger);

impl MTLCullMode {
    pub const None: Self = Self(0);
    pub const Front: Self = Self(1);
    pub const Back: Self = Self(2);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLGPUFamily(pub NSInteger);

impl MTLGPUFamily {
    pub const Apple1: Self = Self(1001);
    pub const Apple2: Self = Self(1002);
    pub const Apple3: Self = Self(1003);
    pub const Apple4: Self = Self(1004);
    pub const Apple5: Self = Self(1005);
    pub const Apple6: Self = Self(1006);
    pub const Apple7: Self = Self(1007);
    pub const Apple8: Self = Self(1008);
    pub const Apple9: Self = Self(1009);
    pub const Apple10: Self = Self(1010);
    pub const Mac1: Self = Self(2001);
    pub const Mac2: Self = Self(2002);
    pub const Common1: Self = Self(3001);
    pub const Common2: Self = Self(3002);
    pub const Common3: Self = Self(3003);
    pub const MacCatalyst1: Self = Self(4001);
    pub const MacCatalyst2: Self = Self(4002);
    pub const Metal3: Self = Self(5001);
    pub const Metal4: Self = Self(5002);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLIndexType(pub NSUInteger);

impl MTLIndexType {
    pub const UInt16: Self = Self(0);
    pub const UInt32: Self = Self(1);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLLanguageVersion(pub NSUInteger);

impl MTLLanguageVersion {
    pub const Version1_0: Self = Self(1 << 16);
    pub const Version1_1: Self = Self((1 << 16) + 1);
    pub const Version1_2: Self = Self((1 << 16) + 2);
    pub const Version2_0: Self = Self(2 << 16);
    pub const Version2_1: Self = Self((2 << 16) + 1);
    pub const Version2_2: Self = Self((2 << 16) + 2);
    pub const Version2_3: Self = Self((2 << 16) + 3);
    pub const Version2_4: Self = Self((2 << 16) + 4);
    pub const Version3_0: Self = Self((3 << 16) + 0);
    pub const Version3_1: Self = Self((3 << 16) + 1);
    pub const Version3_2: Self = Self((3 << 16) + 2);
    pub const Version4_0: Self = Self((4 << 16) + 0);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLLoadAction(pub NSUInteger);

impl MTLLoadAction {
    pub const DontCare: Self = Self(0);
    pub const Load: Self = Self(1);
    pub const Clear: Self = Self(2);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLMathMode(pub NSInteger);

impl MTLMathMode {
    pub const Safe: Self = Self(0);
    pub const Relaxed: Self = Self(1);
    pub const Fast: Self = Self(2);
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MTLOrigin {
    pub x: NSUInteger,
    pub y: NSUInteger,
    pub z: NSUInteger,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLPixelFormat(pub NSUInteger);

impl MTLPixelFormat {
    pub const Invalid: Self = Self(0);
    pub const A8Unorm: Self = Self(1);
    pub const R8Unorm: Self = Self(10);
    pub const R8Unorm_sRGB: Self = Self(11);
    pub const R8Snorm: Self = Self(12);
    pub const R8Uint: Self = Self(13);
    pub const R8Sint: Self = Self(14);
    pub const R16Unorm: Self = Self(20);
    pub const R16Snorm: Self = Self(22);
    pub const R16Uint: Self = Self(23);
    pub const R16Sint: Self = Self(24);
    pub const R16Float: Self = Self(25);
    pub const RG8Unorm: Self = Self(30);
    pub const RG8Unorm_sRGB: Self = Self(31);
    pub const RG8Snorm: Self = Self(32);
    pub const RG8Uint: Self = Self(33);
    pub const RG8Sint: Self = Self(34);
    pub const B5G6R5Unorm: Self = Self(40);
    pub const A1BGR5Unorm: Self = Self(41);
    pub const ABGR4Unorm: Self = Self(42);
    pub const BGR5A1Unorm: Self = Self(43);
    pub const R32Uint: Self = Self(53);
    pub const R32Sint: Self = Self(54);
    pub const R32Float: Self = Self(55);
    pub const RG16Unorm: Self = Self(60);
    pub const RG16Snorm: Self = Self(62);
    pub const RG16Uint: Self = Self(63);
    pub const RG16Sint: Self = Self(64);
    pub const RG16Float: Self = Self(65);
    pub const RGBA8Unorm: Self = Self(70);
    pub const RGBA8Unorm_sRGB: Self = Self(71);
    pub const RGBA8Snorm: Self = Self(72);
    pub const RGBA8Uint: Self = Self(73);
    pub const RGBA8Sint: Self = Self(74);
    pub const BGRA8Unorm: Self = Self(80);
    pub const BGRA8Unorm_sRGB: Self = Self(81);
    pub const RGB10A2Unorm: Self = Self(90);
    pub const RGB10A2Uint: Self = Self(91);
    pub const RG11B10Float: Self = Self(92);
    pub const RGB9E5Float: Self = Self(93);
    pub const BGR10A2Unorm: Self = Self(94);
    pub const BGR10_XR: Self = Self(554);
    pub const BGR10_XR_sRGB: Self = Self(555);
    pub const RG32Uint: Self = Self(103);
    pub const RG32Sint: Self = Self(104);
    pub const RG32Float: Self = Self(105);
    pub const RGBA16Unorm: Self = Self(110);
    pub const RGBA16Snorm: Self = Self(112);
    pub const RGBA16Uint: Self = Self(113);
    pub const RGBA16Sint: Self = Self(114);
    pub const RGBA16Float: Self = Self(115);
    pub const BGRA10_XR: Self = Self(552);
    pub const BGRA10_XR_sRGB: Self = Self(553);
    pub const RGBA32Uint: Self = Self(123);
    pub const RGBA32Sint: Self = Self(124);
    pub const RGBA32Float: Self = Self(125);
    pub const BC1_RGBA: Self = Self(130);
    pub const BC1_RGBA_sRGB: Self = Self(131);
    pub const BC2_RGBA: Self = Self(132);
    pub const BC2_RGBA_sRGB: Self = Self(133);
    pub const BC3_RGBA: Self = Self(134);
    pub const BC3_RGBA_sRGB: Self = Self(135);
    pub const BC4_RUnorm: Self = Self(140);
    pub const BC4_RSnorm: Self = Self(141);
    pub const BC5_RGUnorm: Self = Self(142);
    pub const BC5_RGSnorm: Self = Self(143);
    pub const BC6H_RGBFloat: Self = Self(150);
    pub const BC6H_RGBUfloat: Self = Self(151);
    pub const BC7_RGBAUnorm: Self = Self(152);
    pub const BC7_RGBAUnorm_sRGB: Self = Self(153);
    pub const PVRTC_RGB_2BPP: Self = Self(160);
    pub const PVRTC_RGB_2BPP_sRGB: Self = Self(161);
    pub const PVRTC_RGB_4BPP: Self = Self(162);
    pub const PVRTC_RGB_4BPP_sRGB: Self = Self(163);
    pub const PVRTC_RGBA_2BPP: Self = Self(164);
    pub const PVRTC_RGBA_2BPP_sRGB: Self = Self(165);
    pub const PVRTC_RGBA_4BPP: Self = Self(166);
    pub const PVRTC_RGBA_4BPP_sRGB: Self = Self(167);
    pub const EAC_R11Unorm: Self = Self(170);
    pub const EAC_R11Snorm: Self = Self(172);
    pub const EAC_RG11Unorm: Self = Self(174);
    pub const EAC_RG11Snorm: Self = Self(176);
    pub const EAC_RGBA8: Self = Self(178);
    pub const EAC_RGBA8_sRGB: Self = Self(179);
    pub const ETC2_RGB8: Self = Self(180);
    pub const ETC2_RGB8_sRGB: Self = Self(181);
    pub const ETC2_RGB8A1: Self = Self(182);
    pub const ETC2_RGB8A1_sRGB: Self = Self(183);
    pub const ASTC_4x4_sRGB: Self = Self(186);
    pub const ASTC_5x4_sRGB: Self = Self(187);
    pub const ASTC_5x5_sRGB: Self = Self(188);
    pub const ASTC_6x5_sRGB: Self = Self(189);
    pub const ASTC_6x6_sRGB: Self = Self(190);
    pub const ASTC_8x5_sRGB: Self = Self(192);
    pub const ASTC_8x6_sRGB: Self = Self(193);
    pub const ASTC_8x8_sRGB: Self = Self(194);
    pub const ASTC_10x5_sRGB: Self = Self(195);
    pub const ASTC_10x6_sRGB: Self = Self(196);
    pub const ASTC_10x8_sRGB: Self = Self(197);
    pub const ASTC_10x10_sRGB: Self = Self(198);
    pub const ASTC_12x10_sRGB: Self = Self(199);
    pub const ASTC_12x12_sRGB: Self = Self(200);
    pub const ASTC_4x4_LDR: Self = Self(204);
    pub const ASTC_5x4_LDR: Self = Self(205);
    pub const ASTC_5x5_LDR: Self = Self(206);
    pub const ASTC_6x5_LDR: Self = Self(207);
    pub const ASTC_6x6_LDR: Self = Self(208);
    pub const ASTC_8x5_LDR: Self = Self(210);
    pub const ASTC_8x6_LDR: Self = Self(211);
    pub const ASTC_8x8_LDR: Self = Self(212);
    pub const ASTC_10x5_LDR: Self = Self(213);
    pub const ASTC_10x6_LDR: Self = Self(214);
    pub const ASTC_10x8_LDR: Self = Self(215);
    pub const ASTC_10x10_LDR: Self = Self(216);
    pub const ASTC_12x10_LDR: Self = Self(217);
    pub const ASTC_12x12_LDR: Self = Self(218);
    pub const ASTC_4x4_HDR: Self = Self(222);
    pub const ASTC_5x4_HDR: Self = Self(223);
    pub const ASTC_5x5_HDR: Self = Self(224);
    pub const ASTC_6x5_HDR: Self = Self(225);
    pub const ASTC_6x6_HDR: Self = Self(226);
    pub const ASTC_8x5_HDR: Self = Self(228);
    pub const ASTC_8x6_HDR: Self = Self(229);
    pub const ASTC_8x8_HDR: Self = Self(230);
    pub const ASTC_10x5_HDR: Self = Self(231);
    pub const ASTC_10x6_HDR: Self = Self(232);
    pub const ASTC_10x8_HDR: Self = Self(233);
    pub const ASTC_10x10_HDR: Self = Self(234);
    pub const ASTC_12x10_HDR: Self = Self(235);
    pub const ASTC_12x12_HDR: Self = Self(236);
    pub const GBGR422: Self = Self(240);
    pub const BGRG422: Self = Self(241);
    pub const Depth16Unorm: Self = Self(250);
    pub const Depth32Float: Self = Self(252);
    pub const Stencil8: Self = Self(253);
    pub const Depth24Unorm_Stencil8: Self = Self(255);
    pub const Depth32Float_Stencil8: Self = Self(260);
    pub const X32_Stencil8: Self = Self(261);
    pub const X24_Stencil8: Self = Self(262);
    pub const Unspecialized: Self = Self(263);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLPrimitiveType(pub NSUInteger);

impl MTLPrimitiveType {
    pub const Point: Self = Self(0);
    pub const Line: Self = Self(1);
    pub const LineStrip: Self = Self(2);
    pub const Triangle: Self = Self(3);
    pub const TriangleStrip: Self = Self(4);
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MTLRegion {
    pub origin: MTLOrigin,
    pub size: MTLSize,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLResourceOptions(pub NSUInteger);

bitflags::bitflags! {
    impl MTLResourceOptions: NSUInteger {
        const CPUCacheModeDefaultCache = MTLCPUCacheMode::DefaultCache.0<<MTLResourceCPUCacheModeShift;
        const CPUCacheModeWriteCombined = MTLCPUCacheMode::WriteCombined.0<<MTLResourceCPUCacheModeShift;
        const StorageModeShared = MTLStorageMode::Shared.0<<MTLResourceStorageModeShift;
        const StorageModeManaged = MTLStorageMode::Managed.0<<MTLResourceStorageModeShift;
        const StorageModePrivate = MTLStorageMode::Private.0<<MTLResourceStorageModeShift;
        const StorageModeMemoryless = MTLStorageMode::Memoryless.0<<MTLResourceStorageModeShift;
        const HazardTrackingModeDefault = MTLHazardTrackingMode::Default.0<<MTLResourceHazardTrackingModeShift;
        const HazardTrackingModeUntracked = MTLHazardTrackingMode::Untracked.0<<MTLResourceHazardTrackingModeShift;
        const HazardTrackingModeTracked = MTLHazardTrackingMode::Tracked.0<<MTLResourceHazardTrackingModeShift;
        const OptionCPUCacheModeDefault = MTLResourceOptions::CPUCacheModeDefaultCache.0;
        const OptionCPUCacheModeWriteCombined = MTLResourceOptions::CPUCacheModeWriteCombined.0;
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLSamplerAddressMode(pub NSUInteger);

impl MTLSamplerAddressMode {
    pub const ClampToEdge: Self = Self(0);
    pub const MirrorClampToEdge: Self = Self(1);
    pub const Repeat: Self = Self(2);
    pub const MirrorRepeat: Self = Self(3);
    pub const ClampToZero: Self = Self(4);
    pub const ClampToBorderColor: Self = Self(5);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLSamplerBorderColor(pub NSUInteger);

impl MTLSamplerBorderColor {
    pub const TransparentBlack: Self = Self(0);
    pub const OpaqueBlack: Self = Self(1);
    pub const OpaqueWhite: Self = Self(2);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLSamplerMinMagFilter(pub NSUInteger);

impl MTLSamplerMinMagFilter {
    pub const Nearest: Self = Self(0);
    pub const Linear: Self = Self(1);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLSamplerMipFilter(pub NSUInteger);

impl MTLSamplerMipFilter {
    pub const NotMipmapped: Self = Self(0);
    pub const Nearest: Self = Self(1);
    pub const Linear: Self = Self(2);
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MTLScissorRect {
    pub x: NSUInteger,
    pub y: NSUInteger,
    pub width: NSUInteger,
    pub height: NSUInteger,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MTLSize {
    pub width: NSUInteger,
    pub height: NSUInteger,
    pub depth: NSUInteger,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLStencilOperation(pub NSUInteger);

impl MTLStencilOperation {
    pub const Keep: Self = Self(0);
    pub const Zero: Self = Self(1);
    pub const Replace: Self = Self(2);
    pub const IncrementClamp: Self = Self(3);
    pub const DecrementClamp: Self = Self(4);
    pub const Invert: Self = Self(5);
    pub const IncrementWrap: Self = Self(6);
    pub const DecrementWrap: Self = Self(7);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLStorageMode(pub NSUInteger);

impl MTLStorageMode {
    pub const Shared: Self = Self(0);
    pub const Managed: Self = Self(1);
    pub const Private: Self = Self(2);
    pub const Memoryless: Self = Self(3);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLStoreAction(pub NSUInteger);

impl MTLStoreAction {
    pub const DontCare: Self = Self(0);
    pub const Store: Self = Self(1);
    pub const MultisampleResolve: Self = Self(2);
    pub const StoreAndMultisampleResolve: Self = Self(3);
    pub const Unknown: Self = Self(4);
    pub const CustomSampleDepthStore: Self = Self(5);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLTextureSwizzle(pub u8);

impl MTLTextureSwizzle {
    pub const Zero: Self = Self(0);
    pub const One: Self = Self(1);
    pub const Red: Self = Self(2);
    pub const Green: Self = Self(3);
    pub const Blue: Self = Self(4);
    pub const Alpha: Self = Self(5);
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MTLTextureSwizzleChannels {
    pub red: MTLTextureSwizzle,
    pub green: MTLTextureSwizzle,
    pub blue: MTLTextureSwizzle,
    pub alpha: MTLTextureSwizzle,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLTextureType(pub NSUInteger);

impl MTLTextureType {
    pub const Type1D: Self = Self(0);
    pub const Type1DArray: Self = Self(1);
    pub const Type2D: Self = Self(2);
    pub const Type2DArray: Self = Self(3);
    pub const Type2DMultisample: Self = Self(4);
    pub const TypeCube: Self = Self(5);
    pub const TypeCubeArray: Self = Self(6);
    pub const Type3D: Self = Self(7);
    pub const Type2DMultisampleArray: Self = Self(8);
    pub const TypeTextureBuffer: Self = Self(9);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLTextureUsage(pub NSUInteger);

bitflags::bitflags! {
    impl MTLTextureUsage: NSUInteger {
        const Unknown = 0x0000;
        const ShaderRead = 0x0001;
        const ShaderWrite = 0x0002;
        const RenderTarget = 0x0004;
        const PixelFormatView = 0x0010;
        const ShaderAtomic = 0x0020;
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLTriangleFillMode(pub NSUInteger);

impl MTLTriangleFillMode {
    pub const Fill: Self = Self(0);
    pub const Lines: Self = Self(1);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLVertexFormat(pub NSUInteger);

impl MTLVertexFormat {
    pub const Invalid: Self = Self(0);
    pub const UChar2: Self = Self(1);
    pub const UChar3: Self = Self(2);
    pub const UChar4: Self = Self(3);
    pub const Char2: Self = Self(4);
    pub const Char3: Self = Self(5);
    pub const Char4: Self = Self(6);
    pub const UChar2Normalized: Self = Self(7);
    pub const UChar3Normalized: Self = Self(8);
    pub const UChar4Normalized: Self = Self(9);
    pub const Char2Normalized: Self = Self(10);
    pub const Char3Normalized: Self = Self(11);
    pub const Char4Normalized: Self = Self(12);
    pub const UShort2: Self = Self(13);
    pub const UShort3: Self = Self(14);
    pub const UShort4: Self = Self(15);
    pub const Short2: Self = Self(16);
    pub const Short3: Self = Self(17);
    pub const Short4: Self = Self(18);
    pub const UShort2Normalized: Self = Self(19);
    pub const UShort3Normalized: Self = Self(20);
    pub const UShort4Normalized: Self = Self(21);
    pub const Short2Normalized: Self = Self(22);
    pub const Short3Normalized: Self = Self(23);
    pub const Short4Normalized: Self = Self(24);
    pub const Half2: Self = Self(25);
    pub const Half3: Self = Self(26);
    pub const Half4: Self = Self(27);
    pub const Float: Self = Self(28);
    pub const Float2: Self = Self(29);
    pub const Float3: Self = Self(30);
    pub const Float4: Self = Self(31);
    pub const Int: Self = Self(32);
    pub const Int2: Self = Self(33);
    pub const Int3: Self = Self(34);
    pub const Int4: Self = Self(35);
    pub const UInt: Self = Self(36);
    pub const UInt2: Self = Self(37);
    pub const UInt3: Self = Self(38);
    pub const UInt4: Self = Self(39);
    pub const Int1010102Normalized: Self = Self(40);
    pub const UInt1010102Normalized: Self = Self(41);
    pub const UChar4Normalized_BGRA: Self = Self(42);
    pub const UChar: Self = Self(45);
    pub const Char: Self = Self(46);
    pub const UCharNormalized: Self = Self(47);
    pub const CharNormalized: Self = Self(48);
    pub const UShort: Self = Self(49);
    pub const Short: Self = Self(50);
    pub const UShortNormalized: Self = Self(51);
    pub const ShortNormalized: Self = Self(52);
    pub const Half: Self = Self(53);
    pub const FloatRG11B10: Self = Self(54);
    pub const FloatRGB9E5: Self = Self(55);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLVertexStepFunction(pub NSUInteger);

impl MTLVertexStepFunction {
    pub const Constant: Self = Self(0);
    pub const PerVertex: Self = Self(1);
    pub const PerInstance: Self = Self(2);
    pub const PerPatch: Self = Self(3);
    pub const PerPatchControlPoint: Self = Self(4);
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MTLViewport {
    pub originX: c_double,
    pub originY: c_double,
    pub width: c_double,
    pub height: c_double,
    pub znear: c_double,
    pub zfar: c_double,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLVisibilityResultMode(pub NSUInteger);

impl MTLVisibilityResultMode {
    pub const Disabled: Self = Self(0);
    pub const Boolean: Self = Self(1);
    pub const Counting: Self = Self(2);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLCaptureDestination(pub NSInteger);

impl MTLCaptureDestination {
    pub const DeveloperTools: Self = Self(1);
    pub const GPUTraceDocument: Self = Self(2);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLCommandBufferErrorOption(pub NSUInteger);

bitflags::bitflags! {
    impl MTLCommandBufferErrorOption: NSUInteger {
        const None = 0;
        const EncoderExecutionStatus = 1<<0;
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLCommandEncoderErrorState(pub NSInteger);

impl MTLCommandEncoderErrorState {
    pub const Unknown: Self = Self(0);
    pub const Completed: Self = Self(1);
    pub const Affected: Self = Self(2);
    pub const Pending: Self = Self(3);
    pub const Faulted: Self = Self(4);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLDepthClipMode(pub NSUInteger);

impl MTLDepthClipMode {
    pub const Clip: Self = Self(0);
    pub const Clamp: Self = Self(1);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLFXSpatialScalerColorProcessingMode(pub NSInteger);

impl MTLFXSpatialScalerColorProcessingMode {
    pub const Perceptual: Self = Self(0);
    pub const Linear: Self = Self(1);
    pub const HDR: Self = Self(2);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLCPUCacheMode(pub NSUInteger);

impl MTLCPUCacheMode {
    pub const DefaultCache: Self = Self(0);
    pub const WriteCombined: Self = Self(1);
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MTLHazardTrackingMode(pub NSUInteger);

impl MTLHazardTrackingMode {
    pub const Default: Self = Self(0);
    pub const Untracked: Self = Self(1);
    pub const Tracked: Self = Self(2);
}
