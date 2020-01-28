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
        let mask = align - 1;
        let u = self.as_usize();
        if u & mask == 0 {
            return self;
        }
        let aligned = (u | mask) + 1;
        Self::from_usize(aligned as usize)
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
        let aligned = self.as_usize() & !(align - 1);
        Self::from_usize(aligned)
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

    /// Returns `true` if `self` is aligned on the alignment of the specified
    /// type.
    #[inline]
    fn is_aligned_for<T>(self) -> bool {
        self.is_aligned(core::mem::align_of::<T>())
    }

    /// # Panics
    ///
    /// - If `self` is not aligned for a `T`-typed value.
    fn as_ptr<T>(self) -> *mut T {
        // Some architectures permit unaligned reads, but Rust considers
        // dereferencing a pointer that isn't type-aligned to be UB.
        assert!(self.is_aligned_for::<T>());
        self.as_usize() as *mut T
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct PAddr(usize);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct VAddr(usize);

macro_rules! impl_addrs {
    ($(impl Address for $name:ty {})+) => {
        $(
            impl fmt::Debug for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    if let Some(width) = f.width() {
                        f.debug_tuple(stringify!($name))
                            .field(&format_args!("{:#0width$x}", self.0, width = width))
                            .finish()
                    } else {
                        f.debug_tuple(stringify!($name))
                            .field(&format_args!("{:#x}", self.0,))
                            .finish()
                    }
                }
            }

            impl ops::Add<usize> for $name {
                type Output = Self;
                fn add(self, rhs: usize) -> Self {
                    Self::from_usize(self.0 + rhs)
                }
            }

            impl ops::Add for $name {
                type Output = Self;
                fn add(self, rhs: Self) -> Self {
                    Self::from_usize(self.0 + rhs.0)
                }
            }

            impl ops::AddAssign for $name {
                fn add_assign(&mut self, rhs: Self) {
                    self.0 += rhs.0;
                }
            }

            impl ops::AddAssign<usize> for $name {
                fn add_assign(&mut self, rhs: usize) {
                    self.0 += rhs;
                }
            }

            impl ops::Sub<usize> for $name {
                type Output = Self;
                fn sub(self, rhs: usize) -> Self {
                    Self::from_usize(self.0 - rhs)
                }
            }

            impl ops::Sub for $name {
                type Output = Self;
                fn sub(self, rhs: Self) -> Self {
                    Self::from_usize(self.0 - rhs.0)
                }
            }

            impl ops::SubAssign for $name {
                fn sub_assign(&mut self, rhs: Self) {
                    self.0 -= rhs.0;
                }
            }

            impl ops::SubAssign<usize> for $name {
                fn sub_assign(&mut self, rhs: usize) {
                    self.0 -= rhs;
                }
            }

            impl Address for $name {
                fn as_usize(self) -> usize {
                    self.0 as usize
                }

                #[inline]
                fn from_usize(u: usize) -> Self {
                    Self::from_usize_checked(u)
                }
            }

            impl $name {
                #[cfg(target_pointer_width = "64")]
                pub fn from_u64(u: u64) -> Self {
                    Self::from_usize(u as usize)
                }

                #[cfg(target_pointer_width = "u32")]
                pub fn from_u32(u: u32) -> Self {
                    Self::from_usize(u as usize)
                }

                /// Aligns `self` up to `align`.
                ///
                /// The specified alignment must be a power of two.
                ///
                /// # Panics
                ///
                /// - If `align` is not a power of two.
                #[inline]
                pub fn align_up<A: Into<usize>>(self, align: A) -> Self {
                    Address::align_up(self, align)
                }

                /// Aligns `self` down to `align`.
                ///
                /// The specified alignment must be a power of two.
                ///
                /// # Panics
                ///
                /// - If `align` is not a power of two.
                #[inline]
                pub fn align_down<A: Into<usize>>(self, align: A) -> Self {
                    Address::align_down(self, align)
                }

                /// Offsets this address by `offset`.
                ///
                /// If the specified offset would overflow, this function saturates instead.
                #[inline]
                pub fn offset(self, offset: i32) -> Self {
                    Address::offset(self, offset)
                }

                /// Returns the difference between `self` and `other`.
                #[inline]
                pub fn difference(self, other: Self) -> isize {
                    Address::difference(self, other)
                }

                /// Returns `true` if `self` is aligned on the specified alignment.
                #[inline]
                pub fn is_aligned<A: Into<usize>>(self, align: A) -> bool {
                    Address::is_aligned(self, align)
                }

                /// Returns `true` if `self` is aligned on the alignment of the specified
                /// type.
                #[inline]
                pub fn is_aligned_for<T>(self) -> bool {
                    Address::is_aligned_for::<T>(self)
                }

                /// # Panics
                ///
                /// - If `self` is not aligned for a `T`-typed value.
                #[inline]
                pub fn as_ptr<T>(self) -> *mut T {
                    Address::as_ptr(self)
                }
            }
        )+
    }
}

impl PAddr {
    #[inline]
    fn from_usize_checked(u: usize) -> Self {
        #[cfg(target_arch = "x86_64")]
        const MASK: usize = 0xFFF0_0000_0000_0000;

        #[cfg(target_arch = "x86_64")]
        debug_assert_eq!(
            u & MASK,
            0,
            "x86_64 physical addresses may not have the 12 most significant bits set!"
        );
        Self(u)
    }
}

impl VAddr {
    #[inline]
    fn from_usize_checked(u: usize) -> Self {
        #[cfg(target_arch = "x86_64")]
        debug_assert_eq!(
            VAddr(u),
            VAddr(((u << 16) as i64 >> 16) as usize), // sign extend bit 47
            "x86_64 virtual addresses must be in canonical form"
        );
        Self(u)
    }
}

impl_addrs! {
    impl Address for PAddr {}
    impl Address for VAddr {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align_up_1_aligned() {
        // TODO(eliza): eventually, this could be a QuickCheck test that asserts
        // that _all_ addresses align up by 1 to themselves.
        assert_eq!(
            PAddr::from_usize(0x0).align_up(1usize),
            PAddr::from_usize(0x0)
        );
        assert_eq!(
            PAddr::from_usize(0xDEADFACE).align_up(1usize),
            PAddr::from_usize(0xDEADFACE)
        );
        assert_eq!(
            PAddr::from_usize(0x000_F_FFFF_FFFF_FFFF).align_up(1usize),
            PAddr::from_usize(0x000_F_FFFF_FFFF_FFFF)
        );
    }

    #[test]
    fn align_up() {
        assert_eq!(PAddr::from_usize(2).align_up(2usize), PAddr::from_usize(2));
        assert_eq!(
            PAddr::from_usize(123).align_up(2usize),
            PAddr::from_usize(124)
        );
        assert_eq!(
            PAddr::from_usize(0x5555).align_up(16usize),
            PAddr::from_usize(0x5560)
        );
    }
}
