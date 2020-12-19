use core::{fmt, ops};
use mycelium_util::error::Error;

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

    /// Align `self` up to the required alignment for a value of type `T`.
    ///
    /// This is equivalent to
    /// ```rust
    /// # use hal_core::Address;
    /// # fn doc<T: Address>(addr: T) -> T {
    /// addr.align_up(core::mem::align_of::<T>())
    /// # }
    /// ````
    #[inline]
    fn align_up_for<T>(self) -> Self {
        self.align_up(core::mem::align_of::<T>())
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

    /// Align `self` down to the required alignment for a value of type `T`.
    ///
    /// This is equivalent to
    /// ```rust
    /// # use hal_core::Address;
    /// # fn doc<T: Address>(addr: T) -> T {
    /// addr.align_down(core::mem::align_of::<T>())
    /// # }
    /// ````
    #[inline]
    fn align_down_for<T>(self) -> Self {
        self.align_down(core::mem::align_of::<T>())
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
        self.as_usize() % align.into() == 0
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
    #[track_caller]
    fn as_ptr<T>(self) -> *mut T {
        // Some architectures permit unaligned reads, but Rust considers
        // dereferencing a pointer that isn't type-aligned to be UB.
        assert!(
            self.is_aligned_for::<T>(),
            "assertion failed: self.is_aligned_for::<{}>();\n\tself={:?}",
            core::any::type_name::<T>(),
            self
        );
        self.as_usize() as *mut T
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct PAddr(usize);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct VAddr(usize);

#[derive(Clone, Debug)]
pub struct InvalidAddress {
    msg: &'static str,
    addr: usize,
}

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
                /// Offset `self` up by `rhs`.
                ///
                /// # Notes
                ///
                /// * The address will be offset by the minimum addressable unit
                ///   of the target architecture (i.e. probably bytes), *not* by
                ///   by units of a Rust type like `{*const T, *mut T}::add`.
                /// * Therefore, resulting address may have a different
                ///   alignment from the input address.
                ///
                /// # Panics
                ///
                /// * If the resulting address is invalid.
                fn add(self, rhs: usize) -> Self {
                    Self::from_usize(self.0 + rhs)
                }
            }

            impl ops::Add for $name {
                type Output = Self;
                /// Add `rhs` **bytes** to this this address.
                ///
                /// Note that the resulting address may differ in alignment from
                /// the input address!
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

                /// # Panics
                ///
                /// * If debug assertions are enabled and the address is not
                ///   valid for the target architecture.
                #[inline]
                fn from_usize(u: usize) -> Self {
                    if cfg!(debug_assertions) {
                        Self::from_usize_checked(u).unwrap()
                    } else {
                        Self(u)
                    }
                }
            }

            impl $name {
                /// # Panics
                ///
                /// * If debug assertions are enabled and the address is not
                ///   valid for the target architecture.
                #[cfg(target_pointer_width = "64")]
                pub fn from_u64(u: u64) -> Self {
                    Self::from_usize(u as usize)
                }

                /// # Panics
                ///
                /// * If debug assertions are enabled and the address is not
                ///   valid for the target architecture.
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
                /// * If `align` is not a power of two.
                /// * If debug assertions are enabled and the aligned address is
                ///   not valid for the target architecture.
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
                /// * If `align` is not a power of two.
                /// * If debug assertions are enabled and the aligned address is
                ///   not valid for the target architecture.
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
    pub fn from_usize_checked(u: usize) -> Result<Self, InvalidAddress> {
        #[cfg(target_arch = "x86_64")]
        {
            const MASK: usize = 0xFFF0_0000_0000_0000;
            if u & MASK != 0 {
                return Err(InvalidAddress::new(
                    u,
                    "x86_64 physical addresses may not have the 12 most significant bits set!",
                ));
            }
        }

        Ok(Self(u))
    }
}

impl VAddr {
    #[inline]
    pub fn from_usize_checked(u: usize) -> Result<Self, InvalidAddress> {
        #[cfg(target_arch = "x86_64")]
        {
            // sign extend 47th bit
            let s_extend = ((u << 16) as i64 >> 16) as usize;
            if u != s_extend {
                return Err(InvalidAddress::new(
                    u,
                    "x86_64 virtual addresses must be in canonical form",
                ));
            }
        }

        Ok(Self(u))
    }
}

impl_addrs! {
    impl Address for PAddr {}
    impl Address for VAddr {}
}

impl InvalidAddress {
    fn new(addr: usize, msg: &'static str) -> Self {
        Self { addr, msg }
    }
}
impl fmt::Display for InvalidAddress {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "address {:#x} not valid for target architecture: {}",
            self.addr, self.msg
        )
    }
}

impl Error for InvalidAddress {}

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

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x86_64_vaddr_validation() {
        let addr = (0xFFFFF << 47) | 123;
        assert!(
            VAddr::from_usize_checked(addr).is_ok(),
            "{:#016x} is valid",
            addr
        );
        let addr = 123;
        assert!(
            VAddr::from_usize_checked(addr).is_ok(),
            "{:#016x} is valid",
            addr
        );
        let addr = 123 | (1 << 47);
        assert!(
            VAddr::from_usize_checked(addr).is_err(),
            "{:#016x} is invalid",
            addr
        );
        let addr = (0x10101 << 47) | 123;
        assert!(
            VAddr::from_usize_checked(addr).is_err(),
            "{:#016x} is invalid",
            addr
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x86_64_paddr_validation() {
        let addr = 123;
        assert!(
            PAddr::from_usize_checked(addr).is_ok(),
            "{:#016x} is valid",
            addr
        );
        let addr = 0xFFF0_0000_0000_0000 | 123;
        assert!(
            PAddr::from_usize_checked(addr).is_err(),
            "{:#016x} is invalid",
            addr
        );
        let addr = 0x1000_0000_0000_0000 | 123;
        assert!(
            PAddr::from_usize_checked(addr).is_err(),
            "{:#016x} is invalid",
            addr
        );
        let addr = 0x0010_0000_0000_0000 | 123;
        assert!(
            PAddr::from_usize_checked(addr).is_err(),
            "{:#016x} is invalid",
            addr
        );
    }
}
