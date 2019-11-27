use crate::Address;
use core::{fmt, marker::PhantomData, slice};

pub trait Size {
    /// The size (in bits) of this frame.
    const SIZE: usize;
}

/// A memory frame.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
pub struct Frame<A, S: Size> {
    base: A,
    _size: PhantomData<S>,
}

pub struct NotAligned<S> {
    _size: PhantomData<S>,
}

impl<A: Address, S: Size> Frame<A, S> {
    /// Returns a frame starting at the given address.
    pub fn starting_at(addr: impl Into<A>) -> Result<Self, NotAligned<S>> {
        let addr = addr.into();
        if !addr.is_aligned(S::SIZE) {
            return Err(NotAligned { _size: PhantomData });
        }
        Ok(Self::containing(addr))
    }

    /// Returns the frame that contains the given address.
    pub fn containing(addr: impl Into<A>) -> Self {
        let base = addr.into().align_down(S::SIZE);
        Self {
            base,
            _size: PhantomData,
        }
    }

    pub fn base_address(&self) -> A {
        self.base
    }

    pub fn end_address(&self) -> A {
        self.base + S::SIZE
    }

    pub fn contains(&self, addr: impl Into<A>) -> bool {
        let addr = addr.into();
        addr >= self.base && addr <= self.end_address()
    }
}

impl<A: Address, S: Size> AsRef<[u8]> for Frame<A, S> {
    fn as_ref(&self) -> &[u8] {
        let start = self.base.as_ptr() as *const u8;
        unsafe { slice::from_raw_parts::<_, u8>(start, S::SIZE) }
    }
}

impl<A: fmt::Debug, S: Size> fmt::Debug for Frame<A, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("base", &self.base)
            .field("size", S::SIZE)
            .finish()
    }
}
