//! objc2-foundation's `NSString`, `NSError` and `NSRange`, as far as mtld3d's Metal layer uses them.

use core::{
    fmt,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{
    meta,
    objc2::{Message, Retained},
    table, wmt,
};

pub type NSUInteger = usize;
pub type NSInteger = isize;
pub use crate::objc2::runtime::NSObjectProtocol;

/// A string: one the facade made (`from_str`), kept in Rust, or one winemetal returned (an error's description).
pub struct NSString {
    refs: AtomicUsize,
    text: Text,
}

enum Text {
    Owned(String),
    Handle(u64),
}

// SAFETY: a Rust count; a winemetal string is released with the last reference.
unsafe impl Message for NSString {
    unsafe fn retain_raw(ptr: *const Self) {
        // SAFETY: a live string.
        unsafe { &*ptr }.refs.fetch_add(1, Ordering::Relaxed);
    }
    unsafe fn release_raw(ptr: *const Self) {
        // SAFETY: a reference given up; the last one frees the box `boxed` made.
        unsafe {
            if (*ptr).refs.fetch_sub(1, Ordering::AcqRel) == 1 {
                let boxed = Box::from_raw(ptr.cast_mut());
                if let Text::Handle(handle) = boxed.text {
                    release_handle(handle);
                }
            }
        }
    }
}

fn release_handle(handle: u64) {
    meta::release(handle);
    let mut params = wmt::ObjNoRet { handle };
    // SAFETY: the reference this string held.
    unsafe { table::call(wmt::RELEASE, &mut params) };
}

impl NSString {
    fn boxed(text: Text) -> Retained<Self> {
        let ptr = Box::into_raw(Box::new(Self { refs: AtomicUsize::new(1), text }));
        // SAFETY: a new object with its one reference.
        unsafe { Retained::from_raw(ptr).unwrap_unchecked() }
    }

    /// A string the facade keeps itself (no winemetal call).
    #[must_use]
    pub fn from_str(text: &str) -> Retained<Self> {
        Self::boxed(Text::Owned(text.to_owned()))
    }

    /// An autoreleased winemetal string, retained; `None` for 0.
    pub(crate) fn from_autoreleased(handle: u64) -> Option<Retained<Self>> {
        if handle == 0 {
            return None;
        }
        meta::adopt(handle);
        let mut params = wmt::ObjNoRet { handle };
        // SAFETY: a live string.
        unsafe { table::call(wmt::RETAIN, &mut params) };
        Some(Self::boxed(Text::Handle(handle)))
    }

    /// The string's characters.
    #[must_use]
    pub fn to_rust(&self) -> String {
        match &self.text {
            Text::Owned(text) => text.clone(),
            Text::Handle(handle) => winemetal_string(*handle),
        }
    }

    /// The length in UTF-16 units, as Foundation counts (here: in characters).
    #[must_use]
    pub fn length(&self) -> NSUInteger {
        self.to_rust().chars().map(char::len_utf16).sum()
    }
}

/// The UTF-8 text of winemetal string `handle`.
pub(crate) fn winemetal_string(handle: u64) -> String {
    let mut length = wmt::ObjU64U64Ret { handle, arg: wmt::UTF8_STRING_ENCODING, ret: 0 };
    // SAFETY: slot 27 reads a live string.
    unsafe { table::call(27, &mut length) };
    let mut buffer = vec![0u8; usize::try_from(length.ret).unwrap_or(0) + 1];
    let mut params = wmt::NSStringGetCString {
        str: handle,
        buffer_ptr: buffer.as_mut_ptr() as u64,
        max_length: buffer.len() as u64,
        encoding: wmt::UTF8_STRING_ENCODING,
        ret: 0,
    };
    // SAFETY: slot 8 writes at most `max_length` bytes into the buffer.
    unsafe { table::call(wmt::NSSTRING_GET_CSTRING, &mut params) };
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

impl fmt::Display for NSString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rust())
    }
}

impl fmt::Debug for NSString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_rust())
    }
}

/// An error: one winemetal returned (autoreleased, retained here), or the facade's own for a failure winemetal gave
/// no error object for. Its text is its description.
pub struct NSError {
    refs: AtomicUsize,
    text: Text,
}

// SAFETY: as NSString: a Rust count, a winemetal error released with the last reference.
unsafe impl Message for NSError {
    unsafe fn retain_raw(ptr: *const Self) {
        // SAFETY: a live error.
        unsafe { &*ptr }.refs.fetch_add(1, Ordering::Relaxed);
    }
    unsafe fn release_raw(ptr: *const Self) {
        // SAFETY: a reference given up; the last one frees the box.
        unsafe {
            if (*ptr).refs.fetch_sub(1, Ordering::AcqRel) == 1 {
                let boxed = Box::from_raw(ptr.cast_mut());
                if let Text::Handle(handle) = boxed.text {
                    release_handle(handle);
                }
            }
        }
    }
}

impl NSError {
    fn boxed(text: Text) -> Retained<Self> {
        let ptr = Box::into_raw(Box::new(Self { refs: AtomicUsize::new(1), text }));
        // SAFETY: a new object with its one reference.
        unsafe { Retained::from_raw(ptr).unwrap_unchecked() }
    }

    /// An autoreleased winemetal error, retained; else one saying `fallback`.
    pub(crate) fn from_autoreleased_or(handle: u64, fallback: &str) -> Retained<Self> {
        if handle == 0 {
            return Self::boxed(Text::Owned(fallback.to_owned()));
        }
        meta::adopt(handle);
        let mut params = wmt::ObjNoRet { handle };
        // SAFETY: a live error inside the call that returned it.
        unsafe { table::call(wmt::RETAIN, &mut params) };
        Self::boxed(Text::Handle(handle))
    }

    /// An autoreleased winemetal error, retained; `None` for 0.
    pub(crate) fn from_autoreleased(handle: u64) -> Option<Retained<Self>> {
        (handle != 0).then(|| Self::from_autoreleased_or(handle, ""))
    }

    /// The error's description (winemetal has no localized one: `description` is what it has).
    #[must_use]
    pub fn localizedDescription(&self) -> Retained<NSString> {
        match &self.text {
            Text::Owned(text) => NSString::from_str(text),
            Text::Handle(handle) => {
                let mut params = wmt::ObjObjRet { handle: *handle, ret: 0 };
                // SAFETY: slot 28 describes a live object; its string is autoreleased.
                unsafe { table::call(wmt::NSOBJECT_DESCRIPTION, &mut params) };
                NSString::from_autoreleased(params.ret).unwrap_or_else(|| NSString::from_str("(no description)"))
            }
        }
    }

    /// Foundation's error code: winemetal doesn't carry one.
    #[must_use]
    pub const fn code(&self) -> NSInteger {
        0
    }
}

impl fmt::Display for NSError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.localizedDescription().to_rust())
    }
}

impl fmt::Debug for NSError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NSError({self})")
    }
}

/// Foundation's `NSRange`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NSRange {
    pub location: NSUInteger,
    pub length: NSUInteger,
}

impl NSRange {
    #[must_use]
    pub const fn new(location: NSUInteger, length: NSUInteger) -> Self {
        Self { location, length }
    }
}
