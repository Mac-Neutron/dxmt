//! What the facade knows of each winemetal object that winemetal has no getter for (a texture's type and usage, a
//! buffer's length and contents, an object's device and label, a command buffer's queue), kept from its creation
//! until the facade's last reference to it goes.
//!
//! Process-wide by necessity: a handle reaches the facade from anywhere (the PE side keeps them as `u64`s), so the
//! table is keyed by handle, with a count of the facade's references beside each entry (`Retained` creations,
//! clones and drops; a handle the PE side holds counts as the reference it is).

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

/// A texture's description, as created.
#[derive(Clone, Copy, Default, Debug)]
pub struct Texture {
    pub texture_type: u64,
    pub pixel_format: u64,
    pub width: u64,
    pub height: u64,
    pub depth: u64,
    pub mipmap_level_count: u64,
    pub sample_count: u64,
    pub array_length: u64,
    pub usage: u64,
    pub storage_mode: u64,
    pub swizzle: [u8; 4],
}

/// One object's entry.
#[derive(Default, Debug)]
pub struct Entry {
    refs: u32,
    pub device: u64,
    pub queue: u64,
    pub label: Option<String>,
    pub texture: Option<Texture>,
    pub buffer_length: u64,
    pub buffer_contents: usize,
    pub storage_mode: u64,
    pub name: Option<String>,
    /// A drawable's presented time (seconds; 0 until the buffer that presents it completes).
    pub presented_time: f64,
}

/// The table (see the module doc for why it is process-wide).
static TABLE: Mutex<Option<HashMap<u64, Entry>>> = Mutex::new(None);

fn lock() -> MutexGuard<'static, Option<HashMap<u64, Entry>>> {
    TABLE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A +1 from a slot: the first reference of a new object, or one more of a known one.
pub fn adopt(handle: u64) {
    let mut table = lock();
    table.get_or_insert_with(HashMap::new).entry(handle).or_default().refs += 1;
}

/// One more facade reference.
pub fn retain(handle: u64) {
    adopt(handle);
}

/// One reference fewer; the entry goes with the last.
pub fn release(handle: u64) {
    let mut table = lock();
    let Some(map) = table.as_mut() else { return };
    if let Some(entry) = map.get_mut(&handle) {
        entry.refs = entry.refs.saturating_sub(1);
        if entry.refs == 0 {
            map.remove(&handle);
        }
    }
}

/// The facade's count of `handle`'s references (0 for an object it holds none of).
#[must_use]
pub fn refs(handle: u64) -> usize {
    read(handle, |e| e.map_or(0, |e| e.refs as usize))
}

/// Runs `f` on `handle`'s entry, made if the object is unknown (one winemetal made, such as a drawable's texture).
pub fn with<R>(handle: u64, f: impl FnOnce(&mut Entry) -> R) -> R {
    let mut table = lock();
    f(table.get_or_insert_with(HashMap::new).entry(handle).or_default())
}

/// `handle`'s entry's fields, read.
pub fn read<R>(handle: u64, f: impl FnOnce(Option<&Entry>) -> R) -> R {
    let table = lock();
    f(table.as_ref().and_then(|map| map.get(&handle)))
}
