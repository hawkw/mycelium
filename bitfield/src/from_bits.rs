use core::{convert::Infallible, fmt};

/// Trait implemented by values which can be converted to and from raw bits.
///
/// This trait is [implemented by default] for all signed and unsigned integer
/// types, as well as for `bool`s. It can be implemented manually for any
/// user-defined type which has a well-defined bit-pattern representation. For
/// `enum` types with unsigned integer `repr`s, it may also be implemented
/// automatically using the [`enum_from_bits!`] macro.
///
/// [implemented by default]: #foreign-impls
/// [`enum_from_bits!`]: crate::enum_from_bits!
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

/// Generates automatic [`FromBits`] and [`core::convert::TryFrom`]
/// implementations for an `enum` type of [`repr(uN)`], where `uN` is one of
/// [`u8`], [`u16`], [`u32`], [`u64`], or [`u128`].
///
/// This allows an `enum` type to be used with the
/// [`bitfield!`](crate::bitfield!) macro without requiring a manual [`FromBits`]
/// implementation. Essentially, this macro can be thought of as being analogous
/// to `#[derive(FromBits, TryFrom)]`.[^1]
///
/// # Generated Implementations
///
/// This macro will automatically generate a [`FromBits`]`<uN>` and a
/// [`core::convert::TryFrom`]`<uN>` implementation for the defined `enum` type.
/// In addition, [`FromBits`] and [`core::convert::TryFrom`] implementations for
/// each unsized integer type *larger* than `uN` are also automatically
/// generated. The [`Copy`] and [`Clone`] traits are also derived for the
/// generated `enum`, as they are required by the [`FromBits`] implementation..
///
/// Generated `enum` types are [`repr(uN)]`].
///
/// Additional traits may be derived for the `enum` type, such as
/// [`PartialEq`], [`Eq`], and [`Default`]. These traits are not automatically
/// derived, as custom implementations may also be desired, depending on the
/// use-case. For example, the `Default` value for am `enum` may _not_ be all
/// zeroes.
///
/// # Examples
///
/// Basic usage:
///
/// ```rust
/// use mycelium_bitfield::FromBits;
/// use core::convert::TryFrom;
///
/// mycelium_bitfield::enum_from_bits! {
///    /// Doc comments can be added to generated enum types.
///    #[derive(Debug, PartialEq, Eq)] // additional `derive` attributes can be added
///     enum Example<u8> { // generate an enum represented by a u8
///         Foo = 0b0000,
///         Bar = 0b0001,
///         Baz = 0b1000,
///         Qux = 0b0111,
///     }
/// }
///
/// // the generated enum will implement the `FromBits` trait:
/// assert_eq!(Example::try_from_bits(0b1u8), Ok(Example::Bar));
/// assert_eq!(FromBits::<u8>::into_bits(Example::Foo), 0);
///
/// // `core::convert::TryFrom` implementations are also generated:
/// assert_eq!(Example::try_from(0b1000u8), Ok(Example::Baz));
/// assert_eq!(0b0111u32.try_into(), Ok(Example::Qux));
///
/// // invalid bit-patterns return an error:
/// assert!(Example::try_from_bits(0b1001u8).is_err()); // invalid bit pattern
/// assert!(Example::try_from_bits(0b1000_0000u8).is_err()); // too many bits
/// ```
///
/// Only `u8`, `u16`, `u32`, `u64`, and `u128` may be used as `repr`s for
/// generated enums:
///
/// ```rust,compile_fail
/// mycelium_bitfield::enum_from_bits! {
///     /// This won't work. Don't do this.
///     enum InvalidRepr<i32> {
///         This = 0b01,
///         Wont = 0b10,
///         Work = 0b11,
///     }
/// }
/// ```
///
/// [^1]: **Why Not `#[derive(FromBits)]`?** Some readers may be curious about why
///     this is a declarative macro, rather than a procedural `#[derive]` macro.
///     The answer is..."because I felt like it lol". This probably *should* be
///     a proc-macro, since it's essentially just  deriving a trait
///     implementation. However, one of my goals for `mycelium-bitfield` was to
///     see how far I could go using only `macro_rules!` macros. This isn't
///     because I dislike procedural macros, or that I'm concerned about
///     proc-macro compile times --- I just thought it would be a fun challenge
///     to do everything declaratively, if it was possible. And, if you *do*
///     care about the potential build time impact of proc-macro dependencies,
///     this should help. :)
///
/// [`repr(uN)`]:
///     https://doc.rust-lang.org/reference/type-layout.html#primitive-representations
#[macro_export]
macro_rules! enum_from_bits {
    (
        $(#[$meta:meta])* $vis:vis enum $Type:ident<$uN:ident> {
            $(#[$var1_meta:meta])*
            $Variant1:ident = $value1:expr,
            $(
                $(#[$var_meta:meta])*
                $Variant:ident = $value:expr
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[repr($uN)]
        #[derive(Copy, Clone)]
        $vis enum $Type {
            $(#[$var1_meta])*
            $Variant1 = $value1,
            $(
                $(#[$var_meta])*
                $Variant = $value
            ),*
        }

        impl $Type {
            const VARIANTS: &'static [Self] = &[
                Self::$Variant1,
                $(
                    Self::$Variant,
                )*
            ];

            const MAX_VARIANT: Self = {
                // crappy while loop because `for` and iterator adapters don't
                // work in const-eval...
                let mut max = Self::VARIANTS[0];
                let mut i = 0;
                while i < Self::VARIANTS.len() {
                    if Self::VARIANTS[i] as $uN > max as $uN {
                        max = Self::VARIANTS[i];
                    }
                    i += 1;
                }
                max
            };

            const MIN_VARIANT: Self = {
                let mut min = Self::VARIANTS[0];
                let mut i = 0;
                while i < Self::VARIANTS.len() {
                    if (Self::VARIANTS[i] as $uN) < min as $uN {
                        min = Self::VARIANTS[i];
                    }
                    i += 1;
                }
                min
            };

            const NEEDED_BITS: u32 = {
                // we need at least (bit position of `MAX_VARIANT`'s MSB) bits
                // to represent a value of this type.
                let max = Self::MAX_VARIANT as $uN;
                <$uN>::BITS - max.leading_zeros()
            };

            const ERROR: &'static str = concat!(
                "invalid value for ",
                stringify!($Type),
                ": expected one of [",
                stringify!($value1),
                $(
                    ", ", stringify!($value),
                )*
                "]"
            );
        }

        #[automatically_derived]
        impl core::convert::TryFrom<$uN> for $Type {
            type Error = &'static str;

            #[inline]
            fn try_from(value: $uN) -> Result<Self, Self::Error> {
                match value {
                    $value1 => Ok(Self::$Variant1),
                    $(
                        $value => Ok(Self::$Variant),
                    )*
                    _ => Err(Self::ERROR),
                }
            }
        }

        #[automatically_derived]
        impl $crate::FromBits<$uN> for $Type {
            type Error = &'static str;
            const BITS: u32 = Self::NEEDED_BITS;

            #[inline]
            fn try_from_bits(u: $uN) -> Result<Self, Self::Error> {
                Self::try_from(u)
            }

            #[inline]
            fn into_bits(self) -> $uN {
                self as $uN
            }
        }

        $crate::enum_from_bits!(@bigger $uN, $Type);
    };

    (@bigger u8, $Type:ty) =>   {
        $crate::enum_from_bits! { @impl u8, $Type, u16, u32, u64, u128, usize }
    };
    (@bigger u16, $Type:ty) => {
        $crate::enum_from_bits! { @impl u16, $Type, u32, u64, u128, usize }
    };
    (@bigger u32, $Type:ty) => {
        $crate::enum_from_bits! { @impl u32, $Type, u64, u128, usize }
    };
    (@bigger u64, $Type:ty) => {
        $crate::enum_from_bits! { @impl u128 }
    };
    (@bigger $uN:ty, $Type:ty) => {
        compile_error!(
            concat!(
                "repr for ",
                stringify!($Type),
                " must be one of u8, u16, u32, u64, or u128 (got ",
                stringify!($uN),
                ")",
            ));
    };
    (@impl $uN:ty, $Type:ty, $($bigger:ty),+) => {
        $(

            #[automatically_derived]
            impl $crate::FromBits<$bigger> for $Type {
                type Error = &'static str;
                const BITS: u32 = Self::NEEDED_BITS;

                #[inline]
                fn try_from_bits(u: $bigger) -> Result<Self, Self::Error> {
                    Self::try_from(u as $uN)
                }

                #[inline]
                fn into_bits(self) -> $bigger {
                    self as $bigger
                }
            }

            #[automatically_derived]
            impl core::convert::TryFrom<$bigger> for $Type {
                type Error = &'static str;
                #[inline]
                fn try_from(u: $bigger) -> Result<Self, Self::Error> {
                    Self::try_from(u as $uN)
                }
            }
        )+
    };

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
    impl FromBits<u8, u16, u32, u64, u128, usize> for bool {}
}

impl_frombits_for_ty! {
    impl FromBits<u8, u16, u32, u64, u128> for u8 {}
    impl FromBits<u16, u32, u64, u128> for u16 {}
    impl FromBits<u32, u64, u128> for u32 {}
    impl FromBits<u64, u128> for u64 {}
    impl FromBits<u128> for u128 {}

    impl FromBits<u8, u16, u32, u64, u128> for i8 {}
    impl FromBits<u16, u32, u64, u128> for i16 {}
    impl FromBits<u32, u64, u128> for i32 {}
    impl FromBits<u64, u128> for i64 {}

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
    impl FromBits<u128> for usize {}
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

#[cfg(test)]
mod tests {
    use super::*;

    enum_from_bits! {
        #[derive(Debug, PartialEq, Eq)]
        enum Test<u8> {
            Foo = 0b0000,
            Bar = 0b0001,
            Baz = 0b1000,
            Qux = 0b0111,
        }
    }

    #[test]
    fn enum_max_variant() {
        assert_eq!(Test::MAX_VARIANT, Test::Baz);
    }

    #[test]
    fn enum_min_variant() {
        assert_eq!(Test::MIN_VARIANT, Test::Foo);
    }

    #[test]
    fn enum_needed_bits() {
        assert_eq!(Test::NEEDED_BITS, 4);
    }

    #[test]
    fn enum_roundtrips() {
        for variant in [Test::Foo, Test::Bar, Test::Baz, Test::Qux] {
            let bits = dbg!(variant as u8);
            assert_eq!(dbg!(Test::try_from_bits(bits)), Ok(variant));
            assert_eq!(dbg!(Test::try_from_bits(bits as u16)), Ok(variant));
            assert_eq!(dbg!(Test::try_from_bits(bits as u32)), Ok(variant));
            assert_eq!(dbg!(Test::try_from_bits(bits as u64)), Ok(variant));
            assert_eq!(dbg!(Test::try_from_bits(bits as u128)), Ok(variant));
        }
    }

    #[test]
    fn enum_invalid() {
        for value in [0b1001u8, 0b1000_0000u8, 0b1000_0001u8, 0b1111u8] {
            dbg!(value);
            assert!(dbg!(Test::try_from_bits(value)).is_err());
        }
    }
}
