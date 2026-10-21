//! objc2's object model, as far as mtld3d uses it, over winemetal handles.
//!
//! A Metal object is a `ProtocolObject<dyn MTLX>` reference whose address is its winemetal handle (on macOS the
//! Objective-C object's address, as in objc2; in the VM's guest a number the guest picked): the type is zero-sized and
//! never read, so code that turns objects into `u64`s and back works unchanged. `Retained<T>` holds +1 of it; its
//! release goes to winemetal when the facade's own count of the object's references reaches zero. Descriptors and
//! strings the facade makes are Rust objects behind the same `Retained` (`Message` says how each is kept).

use core::{fmt, marker::PhantomData, ops::Deref, ptr::NonNull};

use crate::meta;

/// How an object is retained and released: winemetal for Metal objects, a Rust count for the facade's own.
///
/// # Safety
/// `retain_raw`/`release_raw` keep the object alive exactly as long as its references.
pub unsafe trait Message {
    /// +1 on the object at `ptr`.
    ///
    /// # Safety
    /// `ptr` is a live object of this type.
    unsafe fn retain_raw(ptr: *const Self);
    /// -1 on the object at `ptr`, freeing it at zero.
    ///
    /// # Safety
    /// `ptr` holds a reference the caller gives up.
    unsafe fn release_raw(ptr: *const Self);

    /// A new +1 reference to `self`.
    fn retain(&self) -> Retained<Self>
    where
        Self: Sized,
    {
        // SAFETY: `self` is live; the new reference is the Retained's.
        unsafe {
            Self::retain_raw(self);
            Retained::from_raw(core::ptr::from_ref(self).cast_mut()).unwrap_unchecked()
        }
    }
}

/// objc2's `ProtocolType` (a marker here).
pub trait ProtocolType {}

pub mod rc {
    use super::{Deref, Message, NonNull, PhantomData, fmt};

    /// A +1 reference.
    pub struct Retained<T: ?Sized + Message> {
        ptr: NonNull<T>,
        _p: PhantomData<T>,
    }

    // SAFETY: winemetal objects are thread-safe as Metal's are; the facade's own objects are Sync themselves.
    unsafe impl<T: ?Sized + Message + Sync> Send for Retained<T> {}
    // SAFETY: as above.
    unsafe impl<T: ?Sized + Message + Sync> Sync for Retained<T> {}

    impl<T: Message> Retained<T> {
        /// Takes over +1 at `ptr` (objc2's `from_raw`).
        ///
        /// # Safety
        /// `ptr` is null or holds +1 the caller gives up.
        pub unsafe fn from_raw(ptr: *mut T) -> Option<Self> {
            NonNull::new(ptr).map(|ptr| Self { ptr, _p: PhantomData })
        }

        /// +1 on `ptr` (objc2's `retain`).
        ///
        /// # Safety
        /// `ptr` is null or a live object.
        pub unsafe fn retain(ptr: *mut T) -> Option<Self> {
            let ptr = NonNull::new(ptr)?;
            // SAFETY: the caller's contract.
            unsafe { T::retain_raw(ptr.as_ptr()) };
            Some(Self { ptr, _p: PhantomData })
        }

        /// Gives up the reference without releasing it.
        #[must_use]
        pub fn into_raw(this: Self) -> *mut T {
            let ptr = this.ptr.as_ptr();
            core::mem::forget(this);
            ptr
        }

        /// The object's address.
        #[must_use]
        pub fn as_ptr(this: &Self) -> *const T {
            this.ptr.as_ptr()
        }

        /// The same reference as another type (objc2's `cast_unchecked`).
        ///
        /// # Safety
        /// The object is also a `U`.
        #[must_use]
        pub unsafe fn cast_unchecked<U: Message>(this: Self) -> Retained<U> {
            let ptr = Self::into_raw(this).cast::<U>();
            // SAFETY: non-null, +1 handed over.
            unsafe { Retained::from_raw(ptr).unwrap_unchecked() }
        }

        /// objc2's `into_retained` on an owned object: itself.
        #[must_use]
        pub const fn into_retained(this: Self) -> Self {
            this
        }
    }

    impl<T: ?Sized + Message> Deref for Retained<T> {
        type Target = T;
        fn deref(&self) -> &T {
            // SAFETY: a live object; for a winemetal object the type is zero-sized and never read.
            unsafe { self.ptr.as_ref() }
        }
    }

    impl<T: Message> Clone for Retained<T> {
        fn clone(&self) -> Self {
            (**self).retain()
        }
    }

    impl<T: ?Sized + Message> Drop for Retained<T> {
        fn drop(&mut self) {
            // SAFETY: this reference is given up here.
            unsafe { T::release_raw(self.ptr.as_ptr()) };
        }
    }

    impl<T: ?Sized + Message + fmt::Display> fmt::Display for Retained<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            (**self).fmt(f)
        }
    }

    impl<T: ?Sized + Message> AsRef<T> for Retained<T> {
        fn as_ref(&self) -> &T {
            self
        }
    }

    impl<T: ?Sized + Message> core::borrow::Borrow<T> for Retained<T> {
        fn borrow(&self) -> &T {
            self
        }
    }

    impl<T: ?Sized + Message> fmt::Debug for Retained<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "Retained({:p})", self.ptr)
        }
    }

    impl<T: ?Sized + Message> PartialEq for Retained<T> {
        fn eq(&self, other: &Self) -> bool {
            core::ptr::eq(self.ptr.as_ptr(), other.ptr.as_ptr())
        }
    }

    /// objc2's `AutoreleasePool` marker.
    pub struct AutoreleasePool<'p>(PhantomData<&'p ()>);

    /// Runs `f` inside an autorelease pool: winemetal's on macOS (slot 10, drained by its release), none on Linux,
    /// where the VM's host owns the pools of the calls it runs.
    pub fn autoreleasepool<R>(f: impl for<'p> FnOnce(AutoreleasePool<'p>) -> R) -> R {
        #[cfg(target_vendor = "apple")]
        {
            use crate::{table, wmt};
            let mut params = wmt::ObjRet::default();
            // SAFETY: slot 10 takes and fills this struct.
            unsafe { table::call(wmt::AUTORELEASE_POOL_ALLOC_INIT, &mut params) };
            let result = f(AutoreleasePool(PhantomData));
            let mut release = wmt::ObjNoRet { handle: params.ret };
            // SAFETY: the pool made above, released (drained) on the thread that made it.
            unsafe { table::call(wmt::RELEASE, &mut release) };
            result
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            f(AutoreleasePool(PhantomData))
        }
    }
}

pub use rc::Retained;

pub mod runtime {
    use super::{Message, PhantomData, meta};
    use crate::{table, wmt};

    /// A winemetal object seen through a protocol: zero-sized, its address is the handle.
    #[repr(C)]
    pub struct ProtocolObject<P: ?Sized> {
        _inner: [u8; 0],
        _p: PhantomData<fn() -> *const P>,
    }

    // SAFETY: the handle is a number; winemetal's calls are thread-safe.
    unsafe impl<P: ?Sized> Send for ProtocolObject<P> {}
    // SAFETY: as above.
    unsafe impl<P: ?Sized> Sync for ProtocolObject<P> {}

    impl<P: ?Sized> ProtocolObject<P> {
        /// The same object through another protocol (objc2's `from_ref`).
        pub fn from_ref<T: ?Sized>(obj: &T) -> &Self {
            // SAFETY: a zero-sized type at the same address.
            unsafe { &*(core::ptr::from_ref(obj).cast::<Self>()) }
        }

        /// The same reference through this protocol (objc2's `from_retained`).
        #[must_use]
        pub fn from_retained<T: Message>(obj: super::Retained<T>) -> super::Retained<Self> {
            // SAFETY: a zero-sized view of the same object, its +1 handed over.
            unsafe { super::Retained::cast_unchecked(obj) }
        }

        /// The object's winemetal handle.
        #[must_use]
        pub fn handle(&self) -> u64 {
            core::ptr::from_ref(self) as usize as u64
        }

        /// The object with handle `handle`, borrowed (no reference taken).
        ///
        /// # Safety
        /// `handle` is a live, non-zero winemetal handle of this protocol's kind, alive for `'a`.
        #[must_use]
        pub unsafe fn from_handle<'a>(handle: u64) -> &'a Self {
            // SAFETY: a zero-sized type at a non-null address.
            unsafe { &*(handle as usize as *const Self) }
        }
    }

    // SAFETY: winemetal's retain and release (slots 0, 1), mirrored in the facade's count so its metadata goes with
    // the last reference.
    unsafe impl<P: ?Sized> Message for ProtocolObject<P> {
        unsafe fn retain_raw(ptr: *const Self) {
            let handle = ptr as usize as u64;
            meta::retain(handle);
            let mut params = wmt::ObjNoRet { handle };
            // SAFETY: a live object.
            unsafe { table::call(wmt::RETAIN, &mut params) };
        }
        unsafe fn release_raw(ptr: *const Self) {
            let handle = ptr as usize as u64;
            meta::release(handle);
            let mut params = wmt::ObjNoRet { handle };
            // SAFETY: a reference given up.
            unsafe { table::call(wmt::RELEASE, &mut params) };
        }
    }

    impl<P: ?Sized> PartialEq for ProtocolObject<P> {
        fn eq(&self, other: &Self) -> bool {
            self.handle() == other.handle()
        }
    }

    impl<P: ?Sized> core::fmt::Debug for ProtocolObject<P> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "<object {:#x}>", self.handle())
        }
    }

    /// objc2's `AnyObject`: any winemetal object.
    pub type AnyObject = ProtocolObject<dyn core::any::Any>;

    /// objc2's `NSObjectProtocol`.
    pub trait NSObjectProtocol {
        /// The facade's count of the object's references (the references this process holds, which is what a
        /// retain-discipline test compares).
        fn retainCount(&self) -> usize {
            meta::refs(core::ptr::from_ref(self).cast::<u8>() as usize as u64)
        }
    }
    impl<P: ?Sized> NSObjectProtocol for ProtocolObject<P> {}
}

/// A +1 winemetal handle from a slot, as a `Retained`; `None` for 0.
pub(crate) fn from_plus_one<P: ?Sized>(handle: u64) -> Option<Retained<runtime::ProtocolObject<P>>> {
    if handle == 0 {
        return None;
    }
    meta::adopt(handle);
    // SAFETY: a +1 handle the slot handed over.
    unsafe { Retained::from_raw(handle as usize as *mut runtime::ProtocolObject<P>) }
}

/// An autoreleased (+0) winemetal handle from a slot, retained as a `Retained`; `None` for 0.
pub(crate) fn from_autoreleased<P: ?Sized>(handle: u64) -> Option<Retained<runtime::ProtocolObject<P>>> {
    if handle == 0 {
        return None;
    }
    // SAFETY: a live object (its pool hasn't drained: we're inside the call that returned it).
    unsafe { Retained::retain(handle as usize as *mut runtime::ProtocolObject<P>) }
}
