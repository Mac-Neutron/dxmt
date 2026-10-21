//! objc2-metal-fx's spatial scaler over winemetal (slots 57, 59, 68).
//!
//! The scaler keeps its textures until it is encoded, as winemetal's encode slot takes them
//! together.

#![allow(non_snake_case)]

use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use rustc_hash::FxHashMap;

pub use crate::metal::MTLFXSpatialScalerColorProcessingMode;
use crate::{
    foundation::NSUInteger,
    meta,
    metal::{MTLCommandBuffer, MTLDevice, MTLPixelFormat, MTLTexture, MTLTextureUsage, handle_of},
    objc2::{
        self, Message, Retained,
        runtime::{NSObjectProtocol, ProtocolObject},
    },
    table, wmt,
};

/// The descriptor (the facade's own object).
pub struct MTLFXSpatialScalerDescriptor {
    refs: AtomicUsize,
    d: Mutex<wmt::SpatialScalerInfo>,
}

// SAFETY: a Rust count; the box goes with the last reference.
unsafe impl Message for MTLFXSpatialScalerDescriptor {
    unsafe fn retain_raw(ptr: *const Self) {
        // SAFETY: a live object.
        unsafe { &*ptr }.refs.fetch_add(1, Ordering::Relaxed);
    }
    unsafe fn release_raw(ptr: *const Self) {
        // SAFETY: the last reference frees the box.
        unsafe {
            if (*ptr).refs.fetch_sub(1, Ordering::AcqRel) == 1 {
                drop(Box::from_raw(ptr.cast_mut()));
            }
        }
    }
}

fn lock(d: &Mutex<wmt::SpatialScalerInfo>) -> std::sync::MutexGuard<'_, wmt::SpatialScalerInfo> {
    d.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl MTLFXSpatialScalerDescriptor {
    /// objc2's `new`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// None (objc2-metal-fx generates it `unsafe`).
    #[must_use]
    pub unsafe fn new() -> Retained<Self> {
        let ptr = Box::into_raw(Box::new(Self { refs: AtomicUsize::new(1), d: Mutex::new(wmt::SpatialScalerInfo::default()) }));
        // SAFETY: a new object with its one reference.
        unsafe { Retained::from_raw(ptr).unwrap_unchecked() }
    }
    /// objc2's `setColorTextureFormat`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setColorTextureFormat(&self, v: MTLPixelFormat) { lock(&self.d).color_format = v.0 as u32; }
    /// objc2's `setOutputTextureFormat`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setOutputTextureFormat(&self, v: MTLPixelFormat) { lock(&self.d).output_format = v.0 as u32; }
    /// objc2's `setInputWidth`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setInputWidth(&self, v: NSUInteger) { lock(&self.d).input_width = v as u32; }
    /// objc2's `setInputHeight`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setInputHeight(&self, v: NSUInteger) { lock(&self.d).input_height = v as u32; }
    /// objc2's `setOutputWidth`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setOutputWidth(&self, v: NSUInteger) { lock(&self.d).output_width = v as u32; }
    /// objc2's `setOutputHeight`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setOutputHeight(&self, v: NSUInteger) { lock(&self.d).output_height = v as u32; }
    /// Perceptual only: winemetal's scaler has no processing mode (HDR is left out here).
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn setColorProcessingMode(&self, _v: MTLFXSpatialScalerColorProcessingMode) {}
    /// objc2's `newSpatialScalerWithDevice`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn newSpatialScalerWithDevice(&self, device: &ProtocolObject<dyn MTLDevice>) -> Option<Retained<ProtocolObject<dyn MTLFXSpatialScaler>>> {
        let info = *lock(&self.d);
        let mut params = wmt::NewWithConstInfo { device: device.handle(), info: (&raw const info).cast(), ret: 0 };
        // SAFETY: slot 57 reads the info.
        unsafe { table::call(wmt::DEVICE_NEW_SPATIAL_SCALER, &mut params) };
        let scaler = objc2::from_plus_one(params.ret)?;
        let d = device.handle();
        meta::with(params.ret, |e| e.device = d);
        Some(scaler)
    }
    /// objc2's `supportsDevice`, unsafe as objc2 declares it.
    ///
    /// # Safety
    /// objc2's contract.
    pub unsafe fn supportsDevice(device: &ProtocolObject<dyn MTLDevice>) -> bool {
        let mut params = wmt::ObjU64Ret { handle: device.handle(), ret: 0 };
        // SAFETY: slot 68 queries a live device.
        unsafe { table::call(wmt::DEVICE_SUPPORTS_FX_SPATIAL_SCALER, &mut params) };
        params.ret != 0
    }
}

/// Each scaler's colour and output textures, set until it is encoded.
static TEXTURES: Mutex<Option<FxHashMap<u64, (u64, u64)>>> = Mutex::new(None);

fn textures<R>(f: impl FnOnce(&mut FxHashMap<u64, (u64, u64)>) -> R) -> R {
    f(TEXTURES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert_with(FxHashMap::default))
}

/// A scaler's last facade reference: its textures go with it.
pub fn forget(handle: u64) {
    textures(|map| map.remove(&handle));
}

/// objc2-metal-fx's `MTLFXSpatialScalerBase`.
///
/// # Safety
/// Implemented for the facade's scaler objects only, as objc2 requires.
pub unsafe trait MTLFXSpatialScalerBase: NSObjectProtocol {
    /// objc2-metal-fx's `colorTextureUsage`.
    ///
    /// # Safety
    /// objc2's contract.
    unsafe fn colorTextureUsage(&self) -> MTLTextureUsage {
        MTLTextureUsage::ShaderRead
    }
    /// objc2-metal-fx's `outputTextureUsage`.
    ///
    /// # Safety
    /// objc2's contract.
    unsafe fn outputTextureUsage(&self) -> MTLTextureUsage {
        MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite
    }
    /// objc2-metal-fx's `setColorTexture`.
    ///
    /// # Safety
    /// objc2's contract.
    unsafe fn setColorTexture(&self, color_texture: Option<&ProtocolObject<dyn MTLTexture>>) {
        let t = color_texture.map_or(0, |t| t.handle());
        textures(|m| m.entry(handle_of(self)).or_default().0 = t);
    }
    /// objc2-metal-fx's `setOutputTexture`.
    ///
    /// # Safety
    /// objc2's contract.
    unsafe fn setOutputTexture(&self, output_texture: Option<&ProtocolObject<dyn MTLTexture>>) {
        let t = output_texture.map_or(0, |t| t.handle());
        textures(|m| m.entry(handle_of(self)).or_default().1 = t);
    }
}

/// objc2-metal-fx's `MTLFXSpatialScaler`.
///
/// # Safety
/// Implemented for the facade's scaler objects only, as objc2 requires.
pub unsafe trait MTLFXSpatialScaler: MTLFXSpatialScalerBase {
    /// objc2-metal-fx's `encodeToCommandBuffer`.
    ///
    /// # Safety
    /// objc2's contract.
    unsafe fn encodeToCommandBuffer(&self, command_buffer: &ProtocolObject<dyn MTLCommandBuffer>) {
        let (color, output) = textures(|m| m.get(&handle_of(self)).copied().unwrap_or_default());
        let mut params = wmt::SpatialScale { cmdbuf: command_buffer.handle(), scaler: handle_of(self), color, output, fence: 0 };
        // SAFETY: slot 59 encodes the scaler into a live buffer.
        unsafe { table::call(wmt::COMMAND_BUFFER_ENCODE_SPATIAL_SCALE, &mut params) };
    }
}

unsafe impl MTLFXSpatialScalerBase for ProtocolObject<dyn MTLFXSpatialScaler> {}
unsafe impl MTLFXSpatialScaler for ProtocolObject<dyn MTLFXSpatialScaler> {}
