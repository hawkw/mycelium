use core::{fmt, ops, ptr};

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
        Self::from_usize(aligned)
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
    ///
    /// # Notes
    /// `align` must be a power of two. This is asserted in debug builds.
    fn is_aligned<A: Into<usize>>(self, align: A) -> bool {
        let align = align.into();
        debug_assert!(
            align.is_power_of_two(),
            "align must be a power of two (actual align: {align})",
        );
        self.as_usize() & (align - 1) == 0
    }

    /// Returns `true` if `self` is aligned on the alignment of the specified
    /// type.
    #[inline]
    fn is_aligned_for<T>(self) -> bool {
        self.is_aligned(core::mem::align_of::<T>())
    }

    /// Converts this address into a const pointer to a value of type `T`.
    ///
    /// # Panics
    ///
    /// - If `self` is not aligned for a `T`-typed value.
    #[inline]
    #[track_caller]
    fn as_ptr<T>(self) -> *const T {
        // Some architectures permit unaligned reads, but Rust considers
        // dereferencing a pointer that isn't type-aligned to be UB.
        assert!(
            self.is_aligned_for::<T>(),
            "assertion failed: self.is_aligned_for::<{}>();\n\tself={self:?}",
            core::any::type_name::<T>(),
        );
        ptr::with_exposed_provenance(self.as_usize())
    }

    /// Converts this address into a mutable pointer to a value of type `T`.
    ///
    /// # Panics
    ///
    /// - If `self` is not aligned for a `T`-typed value.
    #[inline]
    #[track_caller]
    fn as_mut_ptr<T>(self) -> *mut T {
        // Some architectures permit unaligned reads, but Rust considers
        // dereferencing a pointer that isn't type-aligned to be UB.
        assert!(
            self.is_aligned_for::<T>(),
            "assertion failed: self.is_aligned_for::<{}>();\n\tself={self:?}",
            core::any::type_name::<T>(),
        );
        ptr::with_exposed_provenance_mut(self.as_usize())
    }

    /// Converts this address into a `Option<NonNull<T>>` from a
    /// `VAddr`, returning `None` if the address is null.
    ///
    /// # Panics
    ///
    /// - If `self` is not aligned for a `T`-typed value.
    #[inline]
    #[track_caller]
    fn as_non_null<T>(self) -> Option<ptr::NonNull<T>> {
        ptr::NonNull::new(self.as_mut_ptr::<T>())
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct PAddr(usize);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct VAddr(usize);

#[derive(Clone, Debug, thiserror::Error)]
#[error("invalid address {addr:#x} for target architecture: {msg}")]
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
                        write!(f, concat!(stringify!($name), "({:#0width$x})"), self.0, width = width)
                    } else {
                        write!(f, concat!(stringify!($name), "({:#x})"), self.0,)
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
                pub const fn zero() -> Self {
                    Self(0)
                }

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
                #[cfg(target_pointer_width = "32")]
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

                /// Converts this address into a const pointer to a value of type `T`.
                ///
                /// # Panics
                ///
                /// - If `self` is not aligned for a `T`-typed value.
                #[inline]
                #[track_caller]
                pub fn as_ptr<T>(self) -> *const T {
                    Address::as_ptr(self)
                }

                /// Converts this address into a mutable pointer to a value of type `T`.
                ///
                /// # Panics
                ///
                /// - If `self` is not aligned for a `T`-typed value.
                #[inline]
                #[track_caller]
                pub fn as_mut_ptr<T>(self) -> *mut T {
                    Address::as_mut_ptr(self)
                }

                /// Converts this address into a `Option<NonNull<T>>` from a
                /// `VAddr`, returning `None` if the address is null.
                ///
                /// # Panics
                ///
                /// - If `self` is not aligned for a `T`-typed value.
                #[inline]
                #[track_caller]
                pub fn as_non_null<T>(self) -> Option<ptr::NonNull<T>> {
                    ptr::NonNull::new(self.as_mut_ptr::<T>())
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

    /// Constructs a `VAddr` from an arbitrary `usize` value *without* checking
    /// if it's valid.
    ///
    /// Pros of this function:
    /// - can be used in const-eval contexts
    ///
    /// Cons of this function:
    /// - "refer to 'Safety' section"
    ///
    /// # Safety
    ///
    /// u can use dis function to construct invalid addresses. probably dont do
    /// that.
    pub const unsafe fn from_usize_unchecked(u: usize) -> Self {
        Self(u)
    }

    /// Constructs a `VAddr` from a `*const T` pointer, exposing its provenance.
    #[inline]
    pub fn from_ptr<T: ?Sized>(ptr: *const T) -> Self {
        Self::from_usize(ptr.expose_provenance())
    }

    #[inline]
    pub fn of<T: ?Sized>(pointee: &T) -> Self {
        Self::from_ptr(pointee as *const T)
    }
}

impl_addrs! {
    impl Address for PAddr {}
    impl Address for VAddr {}
}

impl InvalidAddress {
    fn new(addr: usize, msg: &'static str) -> Self {
        Self { msg, addr }
    }
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
            PAddr::from_usize(0xDEAD_FACE).align_up(1usize),
            PAddr::from_usize(0xDEAD_FACE)
        );
        assert_eq!(
            PAddr::from_usize(0x000F_FFFF_FFFF_FFFF).align_up(1usize),
            PAddr::from_usize(0x000F_FFFF_FFFF_FFFF)
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
            "{addr:#016x} is valid",
        );
        let addr = 123;
        assert!(
            VAddr::from_usize_checked(addr).is_ok(),
            "{addr:#016x} is valid",
        );
        let addr = 123 | (1 << 47);
        assert!(
            VAddr::from_usize_checked(addr).is_err(),
            "{addr:#016x} is invalid",
        );
        let addr = (0x10101 << 47) | 123;
        assert!(
            VAddr::from_usize_checked(addr).is_err(),
            "{addr:#016x} is invalid",
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x86_64_paddr_validation() {
        let addr = 123;
        assert!(
            PAddr::from_usize_checked(addr).is_ok(),
            "{addr:#016x} is valid",
        );
        let addr = 0xFFF0_0000_0000_0000 | 123;
        assert!(
            PAddr::from_usize_checked(addr).is_err(),
            "{addr:#016x} is invalid",
        );
        let addr = 0x1000_0000_0000_0000 | 123;
        assert!(
            PAddr::from_usize_checked(addr).is_err(),
            "{addr:#016x} is invalid",
        );
        let addr = 0x0010_0000_0000_0000 | 123;
        assert!(
            PAddr::from_usize_checked(addr).is_err(),
            "{addr:#016x} is invalid",
        );
    }
}
