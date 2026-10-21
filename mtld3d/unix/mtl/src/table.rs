//! winemetal's Unix-call table.
//!
//! `winemetal.so` loaded once, its 64-bit `__wine_unix_call_funcs` slots called with winemetal's
//! own parameter structs (`winemetal_thunks.h`). A 32-bit process ([`mark_wow64`]) calls its
//! `__wine_unix_call_wow64_funcs` instead, whose entries for the slots the facade calls take the
//! same structs: in the VM's guest they mark the process 32-bit, so the memory winemetal hands out
//! lies below 2 GiB, where the i386 PE side writes into it (D3D9 M2b).
//!
//! The library is the one beside the image holding this crate (`mtld3d.so` in Wine's
//! `lib/wine/<arch>-unix`), or `MTLD3D_WINEMETAL` when that names one (tests). Under Wine the
//! libraries it links (`ntdll.so`, `winemac.so` on macOS) are already loaded; outside Wine a test
//! folder holds stand-ins beside it.

use core::ffi::{CStr, c_char, c_int, c_void};
use std::sync::{
    LazyLock,
    atomic::{AtomicBool, Ordering},
};

type Slot = unsafe extern "C" fn(*mut c_void) -> i32;

/// The loaded table.
///
/// Process-wide, as the library is (one `dlopen` handle per process, never closed).
pub struct Table {
    funcs: *const Slot,
}

// SAFETY: the table is an immutable array of function pointers in a library that is never unloaded.
unsafe impl Send for Table {}
// SAFETY: as above; every slot is callable from any thread (winemetal's own contract).
unsafe impl Sync for Table {}

static TABLE: LazyLock<Result<Table, String>> = LazyLock::new(Table::load);

/// Whether the process is 32-bit.
///
/// The resource is process-wide (one 32-bit address space) and a machine fact, latched once:
/// mtld3d.so's own WoW64 table sets it at its first thunk, before any Metal call, and the table
/// reads it once when it loads.
static WOW64: AtomicBool = AtomicBool::new(false);

/// Marks the process 32-bit: the table loads winemetal's WoW64 entries.
///
/// Called before the table's first use.
pub fn mark_wow64() {
    WOW64.store(true, Ordering::Relaxed);
}

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
    fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
}

#[repr(C)]
struct DlInfo {
    dli_fname: *const c_char,
    dli_fbase: *mut c_void,
    dli_sname: *const c_char,
    dli_saddr: *mut c_void,
}

#[cfg(target_vendor = "apple")]
const RTLD_NOW: c_int = 0x2;
#[cfg(not(target_vendor = "apple"))]
const RTLD_NOW: c_int = 0x2;

fn last_error() -> String {
    // SAFETY: dlerror returns null or a NUL-terminated string valid until the next dl call on this thread.
    let e = unsafe { dlerror() };
    if e.is_null() { "unknown dlopen error".to_owned() } else { unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned() }
}

/// The path of `winemetal.so` to load: `MTLD3D_WINEMETAL`, else the one beside this image.
fn library_path() -> Result<String, String> {
    if let Some(path) = std::env::var_os("MTLD3D_WINEMETAL") {
        return Ok(path.to_string_lossy().into_owned());
    }
    let mut info = DlInfo { dli_fname: core::ptr::null(), dli_fbase: core::ptr::null_mut(), dli_sname: core::ptr::null(), dli_saddr: core::ptr::null_mut() };
    // SAFETY: dladdr only reads the address, a function of this image.
    if unsafe { dladdr(library_path as *const c_void, &raw mut info) } == 0 || info.dli_fname.is_null() {
        return Err("dladdr found no image for mtld3d's facade".to_owned());
    }
    // SAFETY: dli_fname is a NUL-terminated path owned by the dynamic loader.
    let image = unsafe { CStr::from_ptr(info.dli_fname) }.to_string_lossy().into_owned();
    let dir = image.rsplit_once('/').map_or(".", |(dir, _)| dir).to_owned();
    Ok(format!("{dir}/winemetal.so"))
}

impl Table {
    /// The table, loaded at the first call.
    ///
    /// # Errors
    /// The library or its table can't be loaded; the message says why.
    pub fn get() -> Result<&'static Self, &'static str> {
        TABLE.as_ref().map_err(String::as_str)
    }

    fn load() -> Result<Self, String> {
        let path = library_path()?;
        let c_path = std::ffi::CString::new(path.clone()).map_err(|_| format!("{path}: a NUL in the path"))?;
        // SAFETY: a plain dlopen of a path; the handle is kept for the process's life.
        let handle = unsafe { dlopen(c_path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            return Err(format!("dlopen {path}: {}", last_error()));
        }
        let symbol = if WOW64.load(Ordering::Relaxed) { c"__wine_unix_call_wow64_funcs" } else { c"__wine_unix_call_funcs" };
        // SAFETY: a symbol lookup in the handle just opened.
        let funcs = unsafe { dlsym(handle, symbol.as_ptr()) };
        if funcs.is_null() {
            return Err(format!("{path} has no {}: {}", symbol.to_string_lossy(), last_error()));
        }
        log::info!(target: "mtld3d::mtl", "winemetal: {path} ({})", symbol.to_string_lossy());
        Ok(Self { funcs: funcs.cast() })
    }

    /// The address of slot `slot`'s function, for a test that checks the table against its symbols.
    #[must_use]
    pub fn slot_address(&self, slot: u32) -> usize {
        // SAFETY: callers ask only for slots the facade uses, all inside the table.
        unsafe { *self.funcs.add(slot as usize) as usize }
    }

    /// Calls slot `slot` with `params`, winemetal's parameter struct for it; its NTSTATUS.
    ///
    /// # Safety `params` is that slot's struct, valid for reads and writes, and every pointer and
    /// handle in it is valid for the call.
    pub unsafe fn call<P>(&self, slot: u32, params: &mut P) -> i32 {
        // SAFETY: the caller's contract; the slot index is one the facade uses (inside the table).
        unsafe { (*self.funcs.add(slot as usize))((params as *mut P).cast()) }
    }
}

/// Calls slot `slot` of the loaded table.
///
/// Panicking if winemetal isn't loadable (every facade object comes from it, so a call without it
/// is a bug in the order of things).
///
/// # Safety
/// As [`Table::call`].
pub unsafe fn call<P>(slot: u32, params: &mut P) -> i32 {
    let table = Table::get().unwrap_or_else(|e| panic!("winemetal is not loaded: {e}"));
    // SAFETY: the caller's contract.
    unsafe { table.call(slot, params) }
}
