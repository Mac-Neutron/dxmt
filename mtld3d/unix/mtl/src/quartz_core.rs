//! objc2-quartz-core's `CAMetalLayer` and `CAMetalDrawable` over winemetal's layer slots.
//!
//! A layer is the one `CreateMetalViewFromHWND` made for a window; its properties are winemetal's
//! `WMTLayerProps`.

#![allow(non_snake_case)]

use crate::{
    foundation::NSUInteger,
    meta,
    metal::{MTLDevice, MTLDrawable, MTLPixelFormat, MTLTexture, handle_of, retained},
    objc2::{
        self, Message, Retained,
        runtime::{NSObjectProtocol, ProtocolObject},
    },
    table, wmt,
};

/// objc2-core-foundation's `CGSize`.
#[repr(C)]
#[derive(Debug, Default, PartialEq)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

impl CGSize {
    #[must_use]
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// A winemetal layer.
#[repr(C)]
pub struct CAMetalLayer {
    _inner: [u8; 0],
}

// SAFETY: winemetal's retain and release, as any winemetal object.
unsafe impl Message for CAMetalLayer {
    unsafe fn retain_raw(ptr: *const Self) {
        // SAFETY: the same handle as a protocol object.
        unsafe { ProtocolObject::<dyn core::any::Any>::retain_raw(ptr.cast()) }
    }
    unsafe fn release_raw(ptr: *const Self) {
        // SAFETY: as above.
        unsafe { ProtocolObject::<dyn core::any::Any>::release_raw(ptr.cast()) }
    }
}

impl NSObjectProtocol for CAMetalLayer {}

impl CAMetalLayer {
    /// The layer with handle `handle` (from `CreateMetalViewFromHWND`), retained.
    #[must_use]
    pub fn from_handle(handle: u64) -> Option<Retained<Self>> {
        if handle == 0 {
            return None;
        }
        // SAFETY: a live layer.
        unsafe { Retained::retain(handle as usize as *mut Self) }
    }

    fn props(&self) -> wmt::LayerProps {
        let mut props = wmt::LayerProps::default();
        let mut params = wmt::ObjPtrNoRet { handle: handle_of(self), arg: (&raw mut props).cast() };
        // SAFETY: slot 71 fills the props.
        unsafe { table::call(wmt::LAYER_GET_PROPS, &mut params) };
        props
    }

    fn set(&self, f: impl FnOnce(&mut wmt::LayerProps)) {
        let mut props = self.props();
        f(&mut props);
        let mut params = wmt::ObjConstPtrNoRet { handle: handle_of(self), arg: (&raw const props).cast() };
        // SAFETY: slot 70 reads the props.
        unsafe { table::call(wmt::LAYER_SET_PROPS, &mut params) };
    }

    pub fn device(&self) -> Option<Retained<ProtocolObject<dyn MTLDevice>>> {
        retained(meta::read(handle_of(self), |e| e.map_or(0, |e| e.device)))
    }
    pub fn setDevice(&self, device: Option<&ProtocolObject<dyn MTLDevice>>) {
        let device = device.map_or(0, |d| d.handle());
        meta::with(handle_of(self), |e| e.device = device);
        self.set(|p| p.device = device);
    }
    pub fn pixelFormat(&self) -> MTLPixelFormat {
        MTLPixelFormat(self.props().pixel_format as usize)
    }
    pub fn setPixelFormat(&self, pixel_format: MTLPixelFormat) {
        self.set(|p| p.pixel_format = pixel_format.0 as u32);
    }
    pub fn setFramebufferOnly(&self, framebuffer_only: bool) {
        self.set(|p| p.framebuffer_only = framebuffer_only);
    }
    pub fn setOpaque(&self, opaque: bool) {
        self.set(|p| p.opaque = opaque);
    }
    pub fn drawableSize(&self) -> CGSize {
        let p = self.props();
        CGSize::new(p.drawable_width, p.drawable_height)
    }
    pub fn setDrawableSize(&self, drawable_size: CGSize) {
        self.set(|p| {
            p.drawable_width = drawable_size.width;
            p.drawable_height = drawable_size.height;
        });
    }
    pub fn contentsScale(&self) -> f64 {
        self.props().contents_scale
    }
    pub fn setDisplaySyncEnabled(&self, display_sync_enabled: bool) {
        self.set(|p| p.display_sync_enabled = display_sync_enabled);
    }
    /// winemetal's layer keeps Core Animation's default (3).
    pub fn setMaximumDrawableCount(&self, _maximum_drawable_count: NSUInteger) {}
    /// winemetal's layer keeps Core Animation's default (no transaction).
    pub fn setPresentsWithTransaction(&self, _presents_with_transaction: bool) {}
    /// winemetal's layer keeps Core Animation's default (timeouts allowed).
    pub fn setAllowsNextDrawableTimeout(&self, _allows_next_drawable_timeout: bool) {}
    /// SDR only under the facade.
    pub fn wantsExtendedDynamicRangeContent(&self) -> bool {
        false
    }
    pub fn setWantsExtendedDynamicRangeContent(&self, _wants: bool) {}

    pub fn nextDrawable(&self) -> Option<Retained<ProtocolObject<dyn CAMetalDrawable>>> {
        let layer = handle_of(self);
        let mut params = wmt::ObjObjRet { handle: layer, ret: 0 };
        // SAFETY: slot 67 returns an autoreleased drawable or 0 (a timeout).
        unsafe { table::call(wmt::LAYER_NEXT_DRAWABLE, &mut params) };
        let drawable = objc2::from_autoreleased(params.ret)?;
        let device = meta::read(layer, |e| e.map_or(0, |e| e.device));
        meta::with(params.ret, |e| {
            e.queue = layer;
            e.device = device;
        });
        Some(drawable)
    }
}

/// objc2-quartz-core's `CAMetalDrawable`.
///
/// # Safety
/// Implemented for the facade's drawables only, as objc2 requires.
pub unsafe trait CAMetalDrawable: MTLDrawable {
    fn texture(&self) -> Retained<ProtocolObject<dyn MTLTexture>> {
        let mut params = wmt::ObjObjRet { handle: handle_of(self), ret: 0 };
        // SAFETY: slot 66 returns the drawable's texture (autoreleased with the frame's pool).
        unsafe { table::call(wmt::DRAWABLE_TEXTURE, &mut params) };
        objc2::from_autoreleased(params.ret).expect("a drawable's texture")
    }
    fn layer(&self) -> Retained<CAMetalLayer> {
        CAMetalLayer::from_handle(meta::read(handle_of(self), |e| e.map_or(0, |e| e.queue))).expect("a drawable's layer")
    }
}

unsafe impl MTLDrawable for ProtocolObject<dyn CAMetalDrawable> {}
unsafe impl CAMetalDrawable for ProtocolObject<dyn CAMetalDrawable> {}

/// A metal view over the client area of window `hwnd` for `device`.
///
/// Winemetal's `CreateMetalViewFromHWND` (slot 72) makes it. Returns the view's handle and its
/// layer, or `None` when winemetal found no window data and so has no view to give.
#[must_use]
pub fn create_metal_view(hwnd: u64, device: &ProtocolObject<dyn MTLDevice>) -> Option<(u64, Retained<CAMetalLayer>)> {
    let mut params = wmt::CreateMetalView { hwnd, device: device.handle(), ret_view: 0, ret_layer: 0 };
    // SAFETY: slot 72 makes a view for a window handle.
    unsafe { table::call(wmt::CREATE_METAL_VIEW_FROM_HWND, &mut params) };
    if params.ret_view == 0 || params.ret_layer == 0 {
        return None;
    }
    let layer = CAMetalLayer::from_handle(params.ret_layer)?;
    let d = device.handle();
    meta::with(params.ret_layer, |e| e.device = d);
    Some((params.ret_view, layer))
}

/// Releases a view `create_metal_view` made (slot 73), taking its layer off the window.
pub fn release_metal_view(view: u64) {
    let mut params = wmt::ObjNoRet { handle: view };
    // SAFETY: slot 73 releases a live view.
    unsafe { table::call(wmt::RELEASE_METAL_VIEW, &mut params) };
}

impl CAMetalLayer {
    /// sRGB, standard range (winemetal's `MetalLayer_setColorSpace`, slot 93).
    ///
    /// The facade's only layer mode.
    pub fn set_srgb(&self) {
        let mut params = wmt::ObjU64U64Ret { handle: handle_of(self), arg: 0, ret: 0 };
        // SAFETY: slot 93 on a live layer; 0 is WMTColorSpaceSRGB.
        unsafe { table::call(wmt::LAYER_SET_COLOR_SPACE, &mut params) };
    }
}
