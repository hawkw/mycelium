use crate::{Address, Architecture};
use core::{cmp, fmt, marker::PhantomData, ops, slice};

pub trait Size: Copy + PartialEq + Eq {
    /// The size (in bits) of this frame.
    const SIZE: usize;
}

/// An allocator for physical frames of a given size.
///
/// # Safety
///
/// This trait is unsafe to implement, as implementations are responsible for
/// guaranteeing that allocated frames are unique, and may not be allocated by
/// another frame allocator.
pub unsafe trait Alloc<A: Architecture, S: Size> {
    /// Allocate a single frame.
    ///
    /// Note that an implementation of this method is provided as long as an
    /// implementor of this trait provides `alloc_range`.
    ///
    /// # Returns
    /// - `Ok(Frame)` if a frame was successfully allocated.
    /// - `Err` if no more frames can be allocated by this allocator.
    fn alloc(&mut self) -> Result<Frame<A::PAddr, S>, AllocErr> {
        self.alloc_range(1).map(|r| r.start())
    }

    /// Allocate a range of `len` frames.
    ///
    /// # Returns
    /// - `Ok(FrameRange)` if a range of frames was successfully allocated
    /// - `Err` if the requested range could not be satisfied by this allocator.
    fn alloc_range(&mut self, len: usize) -> Result<FrameRange<A::PAddr, S>, AllocErr>;

    /// Deallocate a single frame.
    ///
    /// Note that an implementation of this method is provided as long as an
    /// implementor of this trait provides `dealloc_range`.
    ///
    /// # Returns
    /// - `Ok(())` if the frame was successfully deallocated.
    /// - `Err` if the requested range could not be deallocated.
    fn dealloc(&mut self, frame: Frame<A::PAddr, S>) -> Result<(), AllocErr> {
        self.dealloc_range(frame.range_inclusive(frame))
    }

    /// Deallocate a range of frames.
    ///
    /// # Returns
    /// - `Ok(())` if a range of frames was successfully deallocated
    /// - `Err` if the requested range could not be deallocated.
    fn dealloc_range(&self, range: FrameRange<A::PAddr, S>) -> Result<(), AllocErr>;
}

/// A memory frame.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Frame<A, S: Size> {
    base: A,
    _size: PhantomData<S>,
}

/// A range of memory frames of the same size.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameRange<A: Address, S: Size> {
    start: Frame<A, S>,
    end: Frame<A, S>,
}

pub struct NotAligned<S> {
    _size: PhantomData<S>,
}

pub struct AllocErr {
    // TODO: eliza
    _p: (),
}

// === impl Frame ===

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

    pub fn range_inclusive(self, end: Frame<A, S>) -> FrameRange<A, S> {
        FrameRange { start: self, end }
    }

    pub fn range_to(self, end: Frame<A, S>) -> FrameRange<A, S> {
        FrameRange {
            start: self,
            end: end - 1,
        }
    }
}

impl<A: Address, S: Size> ops::Add<usize> for Frame<A, S> {
    type Output = Self;
    fn add(self, rhs: usize) -> Self {
        Frame {
            base: self.base + (S::SIZE * rhs),
            _size: PhantomData,
        }
    }
}

impl<A: Address, S: Size> ops::Sub<usize> for Frame<A, S> {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self {
        Frame {
            base: self.base - (S::SIZE * rhs),
            _size: PhantomData,
        }
    }
}

impl<A: Address, S: Size> AsRef<[u8]> for Frame<A, S> {
    fn as_ref(&self) -> &[u8] {
        let start = self.base.as_ptr() as *const u8;
        unsafe { slice::from_raw_parts::<u8>(start, S::SIZE) }
    }
}

impl<A: Address, S: Size> cmp::PartialOrd for Frame<A, S> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        // Since the frames are guaranteed by the type system to be the same
        // we can simply compare the base addresses.
        self.base.partial_cmp(&other.base)
    }
}

impl<A: Address, S: Size> cmp::Ord for Frame<A, S> {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        // Since the frames are guaranteed by the type system to be the same
        // we can simply compare the base addresses.
        self.base.cmp(&other.base)
    }
}

impl<A: fmt::Debug, S: Size> fmt::Debug for Frame<A, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("base", &self.base)
            .field("size", &S::SIZE)
            .finish()
    }
}

// === impl FrameRange ===

impl<A: Address, S: Size> FrameRange<A, S> {
    pub fn start(&self) -> Frame<A, S> {
        self.start
    }

    pub fn end(&self) -> Frame<A, S> {
        self.end
    }

    pub fn len(&self) -> usize {
        unimplemented!("eliza")
    }
}

impl<A: Address, S: Size> Iterator for FrameRange<A, S> {
    type Item = Frame<A, S>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.start > self.end {
            return None;
        }
        let next = self.start;
        self.start = self.start + 1;
        Some(next)
    }
}
