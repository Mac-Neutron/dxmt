//! A handle's address and back ([`super::addr_of`], [`super::handle_at`]).

/// The address gives the handle back, on either width.
#[test]
fn a_handle_comes_back_from_its_address() {
    let handle = 3_u64 << 32 | 91;
    assert_eq!(super::handle_at(super::addr_of(handle)), handle);
}

/// A 32-bit build keeps the newest generation of an index.
///
/// A stale handle (a handler's, after its object went and the index was reused) leaves the live
/// object's in place.
#[cfg(target_pointer_width = "32")]
#[test]
fn an_older_generation_leaves_the_live_handle() {
    let (live, stale) = (2_u64 << 32 | 77, 1_u64 << 32 | 77);
    assert_eq!(super::addr_of(live), 77);
    assert_eq!(super::addr_of(stale), 77);
    assert_eq!(super::handle_at(77), live);
}
