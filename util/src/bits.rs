//! bit manipulation utilities
//!
//! This module provides a set of types to make packing bit ranges easier. These
//! utilities can be used in `const fn`.
//!
//! The bit packing utilities consist of a type that defines a specification for
//! a bit range to pack into, and a wrapper type for an unsigned integer
//! defining methods to pack bit ranges into it. Packing specs are defined for
//! [`u64`],  [`u32`], [`u16`], and [`u8`], as [`Pack64`], [`Pack32`],
//! [`Pack16`], and [`Pack8`], respectively.
//!
//! Note that the bit packing utilities are generic using macros, rather than
//! using generics and traits, because they are intended to be usable in
//! const-eval, and trait methods cannot be `const fn`.
//!
//! # Examples
//!
//! Sorry there are no examples on the individual types, I didn't want to figure
//! out how to write doctests inside the macro :)
//!
//! Packing into the least-significant _n_ bits:
//! ```
//! use mycelium_util::bits::Pack32;
//!
//! const LEAST_SIGNIFICANT_8: Pack32 = Pack32::least_significant(8);
//!
//! // the number we're going to pack bits into.
//! let base = 0xface_0000;
//!
//! // pack 0xed into the least significant 8 bits
//! let val = LEAST_SIGNIFICANT_8.pack(0xed, base);
//!
//! assert_eq!(val, 0xface_00ed);
//! ```
//!
//! Packing specs can be defined in relation to each other.
//!
//! ```
//! use mycelium_util::bits::Pack64;
//!
//! const LOW: Pack64 = Pack64::least_significant(12);
//! const MID: Pack64 = LOW.next(8);
//! const HIGH: Pack64 = MID.next(4);
//!
//! let base = 0xfeed000000;
//!
//! // note that we don't need to pack the values in order.
//! let val = HIGH.pack(0xC, base);
//! let val = LOW.pack(0xfee, val);
//! let val = MID.pack(0x0f, val);
//!
//! assert_eq!(val, 0xfeedc0ffee); // i want c0ffee
//! ```
//!
//! The same example can be written a little bit more neatly using methods:
//!
//! ```
//! # use mycelium_util::bits::Pack64;
//! const LOW: Pack64 = Pack64::least_significant(12);
//! const MID: Pack64 = LOW.next(8);
//! const HIGH: Pack64 = MID.next(4);
//!
//! // Wrap a value to pack it using method calls.
//! let coffee = Pack64::pack_in(0)
//!     .pack(0xfee, &LOW)
//!     .pack(0xC, &HIGH)
//!     .pack(0x0f, &MID)
//!     .bits();
//!
//! assert_eq!(coffee, 0xc0ffee); // i still want c0ffee
//! ```
//!
//! Packing specs can be used to extract values from their packed
//! representation:
//!
//! ```
//! # use mycelium_util::bits::Pack64;
//! # const LOW: Pack64 = Pack64::least_significant(12);
//! # const MID: Pack64 = LOW.next(8);
//! # const HIGH: Pack64 = MID.next(4);
//! # let coffee = Pack64::pack_in(0)
//! #     .pack(0xfee, &LOW)
//! #     .pack(0xC, &HIGH)
//! #     .pack(0x0f, &MID)
//! #     .bits();
//! #
//! assert_eq!(LOW.unpack_bits(coffee), 0xfee);
//! assert_eq!(MID.unpack_bits(coffee), 0x0f);
//! assert_eq!(HIGH.unpack_bits(coffee), 0xc);
//! ```
//!
//! Any previously set bit patterns in the packed range will be overwritten, but
//! existing values outside of a packing spec's range are preserved:
//!
//! ```
//! # use mycelium_util::bits::Pack64;
//! # const LOW: Pack64 = Pack64::least_significant(12);
//! # const MID: Pack64 = LOW.next(8);
//! # const HIGH: Pack64 = MID.next(4);
//! // this is not coffee
//! let not_coffee = 0xc0ff0f;
//!
//! let coffee = LOW.pack(0xfee, not_coffee);
//!
//! // now it's coffee
//! assert_eq!(coffee, 0xc0ffee);
//! ```
//!
//! We can also define packing specs for arbitrary bit ranges, in addition to
//! defining them in relation to each other.
//!
//! ```
//! use mycelium_util::bits::Pack64;
//!
//! // pack a 12-bit value starting at the ninth bit
//! let low = Pack64::from_range(9..=21);
//!
//! // pack another value into the next 12 bits following `LOW`.
//! let mid = low.next(12);
//!
//! // pack a third value starting at bit 33 to the end of the `u64`.
//! let high = Pack64::from_range(33..);
//!
//! let val = Pack64::pack_in(0)
//!     .pack(0xc0f, &mid)
//!     .pack(0xfee, &low)
//!     .pack(0xfeed, &high)
//!     .bits();
//!
//! assert_eq!(val, 0xfeedc0ffee00); // starting to detect a bit of a theme here...
//! ```

use core::{convert::Infallible, fmt};

mod pack;
pub use self::pack::*;
mod bitfield;

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
                    const BITS: u32 = <$F>::BITS;
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

impl_frombits_for_ty! {
    impl FromBits<u8, u16, u32, u64, usize> for u64 {}
    impl FromBits<u8, u16, u32, usize> for usize {}
    impl FromBits<u8, u16, u32> for u32 {}
    impl FromBits<u8, u16> for u16 {}
    impl FromBits<u8> for u8 {}

    impl FromBits<u8, u16, u32, u64, usize> for i64 {}
    impl FromBits<u8, u16, u32, usize> for isize {}
    impl FromBits<u8, u16, u32> for i32 {}
    impl FromBits<u8, u16> for i16 {}
    impl FromBits<u8> for i8 {}
}

impl_frombits_for_bool! {
    impl FromBits<u8, u16, u32, u64, usize> for bool {}
}

// mod test_expand {
//     trace_macros!(true);
//     bitfield! {
//         #[allow(dead_code)]
//         struct TestBitfield<u32> {
//             const HELLO = 4;
//             const _RESERVED_1 = 3;
//             const WORLD: bool;
//             const LOTS = 5;
//             const OF = 1;
//             const FUN = 6;
//         }
//     }
//     trace_macros!(false);
// }
