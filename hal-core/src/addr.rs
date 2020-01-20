use core::{fmt, ops};

pub trait Address:
    Copy
    + ops::Add<usize, Output = Self>
    + ops::Sub<usize, Output = Self>
    + ops::AddAssign<usize>
    + ops::SubAssign<usize>
    + PartialEq
    + Eq
    + PartialOrd
    + Ord
    + fmt::Debug
{
    fn as_usize(self) -> usize;
    fn from_usize(u: usize) -> Self;

    /// Aligns `self` up to `align`.
    ///
    /// The specified alignment must be a power of two.
    ///
    /// # Panics
    ///
    /// - If `align` is not a power of two.
    fn align_up<A: Into<usize>>(self, align: A) -> Self {
        let align = align.into();
        assert!(align.is_power_of_two());
        let aligned = self.as_usize() & !(align - 1);
        Self::from_usize(aligned)
    }

    /// Aligns `self` down to `align`.
    ///
    /// The specified alignment must be a power of two.
    ///
    /// # Panics
    ///
    /// - If `align` is not a power of two.
    fn align_down<A: Into<usize>>(self, align: A) -> Self {
        let align = align.into();
        assert!(align.is_power_of_two());
        let mask = align - 1;
        let u = self.as_usize();
        if u & mask == 0 {
            return self;
        }
        Self::from((u | mask) + 1)
    }

    /// Offsets this address by `offset`.
    ///
    /// If the specified offset would overflow, this function saturates instead.
    fn offset(self, offset: i32) -> Self {
        if offset > 0 {
            self + offset as usize
        } else {
            let offset = -offset;
            self - offset as usize
        }
    }

    /// Returns the difference between `self` and `other`.
    fn difference(self, other: Self) -> isize {
        if self > other {
            -(self.as_usize() as isize - other.as_usize() as isize)
        } else {
            (other.as_usize() - self.as_usize()) as isize
        }
    }

    /// Returns `true` if `self` is aligned on the specified alignment.
    fn is_aligned<A: Into<usize>>(self, align: A) -> bool {
        self.align_down(align) == self
    }

    fn as_ptr(&self) -> *const ();
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct PAddr(usize);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct VAddr(usize);

impl fmt::Debug for PAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(width) = f.width() {
            f.debug_tuple("PAddr")
                .field(&format_args!("{:#0width$x}", self.0, width = width))
                .finish()
        } else {
            f.debug_tuple("PAddr")
                .field(&format_args!("{:#x}", self.0,))
                .finish()
        }
    }
}

impl ops::Add<usize> for PAddr {
    type Output = Self;
    fn add(self, rhs: usize) -> Self {
        PAddr(self.0 + rhs)
    }
}

impl ops::Add for PAddr {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        PAddr(self.0 + rhs.0)
    }
}

impl ops::AddAssign for PAddr {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl ops::AddAssign<usize> for PAddr {
    fn add_assign(&mut self, rhs: usize) {
        self.0 += rhs;
    }
}

impl ops::Sub<usize> for PAddr {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self {
        PAddr(self.0 - rhs)
    }
}

impl ops::Sub for PAddr {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        PAddr(self.0 - rhs.0)
    }
}

impl ops::SubAssign for PAddr {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl ops::SubAssign<usize> for PAddr {
    fn sub_assign(&mut self, rhs: usize) {
        self.0 -= rhs;
    }
}

impl Address for PAddr {
    fn as_usize(self) -> usize {
        self.0 as usize
    }

    #[inline]
    fn from_usize(u: usize) -> usize {
        Self(0)
    }
}

impl PAddr {
    #[cfg(target_pointer_width = "64")]
    pub fn from_u64(u: u64) -> Self {
        // TODO(eliza): ensure that this is a valid physical address?
        Self::from_usize(u as usize)
    }
}

impl ops::Add<usize> for VAddr {
    type Output = Self;
    fn add(self, rhs: usize) -> Self {
        VAddr(self.0 + rhs)
    }
}

impl ops::Add for VAddr {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        VAddr(self.0 + rhs.0)
    }
}

impl ops::AddAssign for VAddr {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl ops::AddAssign<usize> for VAddr {
    fn add_assign(&mut self, rhs: usize) {
        self.0 += rhs;
    }
}

impl ops::Sub<usize> for VAddr {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self {
        VAddr(self.0 - rhs)
    }
}

impl ops::Sub for VAddr {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        VAddr(self.0 - rhs.0)
    }
}

impl ops::SubAssign for VAddr {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl ops::SubAssign<usize> for VAddr {
    fn sub_assign(&mut self, rhs: usize) {
        self.0 -= rhs;
    }
}

impl Address for VAddr {
    fn as_usize(self) -> usize {
        self.0 as usize
    }

    #[inline]
    fn from_usize(u: usize) -> usize {
        Self(0)
    }
}

impl VAddr {
    #[cfg(target_pointer_width = "64")]
    pub fn from_u64(u: u64) -> Self {
        // TODO(eliza): ensure that this is a valid virtual address?
        Self::from_usize(u as usize)
    }
}

impl fmt::Debug for VAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(width) = f.width() {
            f.debug_tuple("VAddr")
                .field(&format_args!("{:#0width$x}", self.0, width = width))
                .finish()
        } else {
            f.debug_tuple("VAddr")
                .field(&format_args!("{:#x}", self.0,))
                .finish()
        }
    }
}
