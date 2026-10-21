//! block2's `RcBlock` for the handlers mtld3d gives Metal: a shared closure the receiver keeps a reference to (Metal
//! copies a block it is handed, so the caller's may drop at once) and runs on the facade's retirement thread.

use std::sync::Arc;

/// The block a handler parameter points at.
pub struct DynBlock<F: ?Sized> {
    pub(crate) f: Arc<F>,
}

/// A block's owner; `as_ptr` is what a `*Handler` parameter takes.
pub struct RcBlock<F: ?Sized> {
    block: Box<DynBlock<F>>,
}

/// A closure as the block type it makes (one argument, as every handler mtld3d registers).
pub trait IntoBlock<A> {
    type Dyn: ?Sized;
    fn into_arc(self) -> Arc<Self::Dyn>;
}

impl<A: 'static, F: Fn(A) + 'static> IntoBlock<(A,)> for F {
    type Dyn = dyn Fn(A);
    fn into_arc(self) -> Arc<dyn Fn(A)> {
        Arc::new(self)
    }
}

impl<F: ?Sized> RcBlock<F> {
    /// A block running `closure`.
    pub fn new<A, C: IntoBlock<A, Dyn = F>>(closure: C) -> Self {
        Self { block: Box::new(DynBlock { f: closure.into_arc() }) }
    }

    /// The block, for a handler parameter.
    #[must_use]
    pub fn as_ptr(this: &Self) -> *mut DynBlock<F> {
        core::ptr::from_ref::<DynBlock<F>>(&this.block).cast_mut()
    }
}

/// A handler the facade keeps: the closure behind a block, sent to the thread that runs it.
pub(crate) struct Kept<F: ?Sized>(pub(crate) Arc<F>);

// SAFETY: Metal runs its handlers on threads of its own; mtld3d writes its blocks for that (they touch only
// thread-safe state), and the facade's retirement thread stands in for Metal's.
unsafe impl<F: ?Sized> Send for Kept<F> {}

impl<F: ?Sized> Kept<F> {
    /// The closure behind the block at `block`, kept.
    ///
    /// # Safety
    /// `block` is a live block from [`RcBlock::as_ptr`].
    pub(crate) unsafe fn from_block(block: *mut DynBlock<F>) -> Self {
        // SAFETY: the caller's contract.
        Self(Arc::clone(unsafe { &(*block).f }))
    }
}
