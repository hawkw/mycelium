use core::{convert::Infallible, fmt};

/// Trait implemented by values which can be converted to and from raw bits.
pub trait FromBits<B>: Sized {
    /// The error type returned by [`Self::try_from_bits`] when an invalid bit
    /// pattern is encountered.
    ///
    /// If all bit patterns possible in [`Self::BITS`] bits are valid bit
    /// patterns for a `Self`-typed value, this should generally be
    /// [`core::convert::Infallible`].
    type Error: fmt::Display;

    /// The number of bits required to represent a value of this type.
    const BITS: u32;

    /// Attempt to convert `bits` into a value of this type.
    ///
    /// # Returns
    ///
    /// - `Ok(Self)` if `bits` contained a valid bit pattern for a value of this
    ///   type.
    /// - `Err(Self::Error)` if `bits` is an invalid bit pattern for a value of
    ///   this type.
    fn try_from_bits(bits: B) -> Result<Self, Self::Error>;

    /// Convert `self` into a raw bit representation.
    ///
    /// In general, this will be a low-cost conversion (e.g., for `enum`s, this
    /// is generally an `as` cast).
    fn into_bits(self) -> B;
}

macro_rules! impl_frombits_for_ty {
   ($(impl FromBits<$($F:ty),+> for $T:ty {})+) => {
        $(

            $(
                impl FromBits<$F> for $T {
                    const BITS: u32 = <$T>::BITS;
                    type Error = Infallible;

                    fn try_from_bits(f: $F) -> Result<Self, Self::Error> {
                        Ok(f as $T)
                    }

                    fn into_bits(self) -> $F {
                        self as $F
                    }
                }
            )*
        )+
    }
}

macro_rules! impl_frombits_for_bool {
    (impl FromBits<$($F:ty),+> for bool {}) => {
        $(
            impl FromBits<$F> for bool {
                const BITS: u32 = 1;
                type Error = Infallible;

                fn try_from_bits(f: $F) -> Result<Self, Self::Error> {
                    Ok(if f == 0 { false } else { true })
                }

                fn into_bits(self) -> $F {
                    if self {
                        1
                    } else {
                        0
                    }
                }
            }
        )+
    }
}

impl_frombits_for_bool! {
    impl FromBits<u8, u16, u32, u64, usize> for bool {}
}

impl_frombits_for_ty! {
    impl FromBits<u8, u16, u32, u64> for u8 {}
    impl FromBits<u16, u32, u64> for u16 {}
    impl FromBits<u32, u64> for u32 {}
    impl FromBits<u64> for u64 {}

    impl FromBits<u8, u16, u32, u64> for i8 {}
    impl FromBits<u16, u32, u64> for i16 {}
    impl FromBits<u32, u64> for i32 {}
    impl FromBits<u64> for i64 {}

    // Rust doesn't support 8 bit targets, so {u,i}size are always at least 16 bit wide,
    // source: https://doc.rust-lang.org/1.45.2/src/core/convert/num.rs.html#134-139
    //
    // This allows the following impls to be supported on all platforms.
    // Impls for {u,i}32 and {u,i}64 however need to be restricted (see below).
    impl FromBits<usize> for u8 {}
    impl FromBits<usize> for i8 {}
    impl FromBits<usize> for u16 {}
    impl FromBits<usize> for i16 {}

    impl FromBits<usize> for usize {}
    impl FromBits<usize> for isize {}
}

#[cfg(target_pointer_width = "16")]
impl_frombits_for_ty! {
    impl FromBits<u16, u32, u64> for usize {}
    impl FromBits<u16, u32, u64> for isize {}
}

#[cfg(target_pointer_width = "32")]
impl_frombits_for_ty! {
    impl FromBits<u32, u64> for usize {}
    impl FromBits<u32, u64> for isize {}

    impl FromBits<usize> for u32 {}
    impl FromBits<usize> for i32 {}
}

#[cfg(target_pointer_width = "64")]
impl_frombits_for_ty! {
    impl FromBits<u64> for usize {}
    impl FromBits<u64> for isize {}

    impl FromBits<usize> for u32 {}
    impl FromBits<usize> for i32 {}
    impl FromBits<usize> for u64 {}
    impl FromBits<usize> for i64 {}
}
