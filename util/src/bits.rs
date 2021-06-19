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
//! assert_eq!(LOW.unpack(coffee), 0xfee);
//! assert_eq!(MID.unpack(coffee), 0x0f);
//! assert_eq!(HIGH.unpack(coffee), 0xc);
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

use core::{
    fmt,
    ops::{Bound, Range, RangeBounds},
};

macro_rules! make_packers {
    ($(pub struct $name:ident { bits: $bits:ty, packing: $packing:ident, pair: $ident })+) => {
        $(

            #[doc = concat!(
                "A spec for packing values into selected bit ranges of [`",
                stringify!($bits),
                "`] values."
            )]
            #[derive(Copy, Clone, PartialEq, Eq)]
            pub struct $name {
                mask: $bits,
                shift: u32,
            }

            #[doc = concat!(
                "Wraps a [`",
                stringify!($bits),
                "`] to add methods for packing bit ranges specified by [`",
                stringify!($name),
                "`]."
            )]
            #[derive(Copy, Clone, PartialEq, Eq)]
            pub struct $packing($bits);

            #[derive(Copy, Clone, PartialEq, Eq)]
            pub struct $pair {
                src: $name,
                dst_shl: $bits,
                dst_shr: $bits,
            }

            impl $name {
                // XXX(eliza): why is this always `u32`? ask the stdlib i guess...
                const SIZE_BITS: u32 = <$bits>::MAX.leading_ones();

                /// Returns a value with the first `n` bits set.
                const fn mask(n: u32) -> $bits {
                    if n == 0 {
                        return 0
                    };
                    let one: $bits = 1; // lolmacros
                    let shift = one.wrapping_shl(n - 1);
                    shift | (shift.saturating_sub(1))
                }

                #[doc = concat!(
                    "Wrap a [`",
                    stringify!($bits),
                    "`] to add methods for packing bit ranges using [`",
                    stringify!($name),
                    "`]."
                )]
                #[doc = ""]
                #[doc = concat!(
                    "This is equivalent to calling [`",
                    stringify!($packing),
                    "::new`], but only requires importing the packer type."
                )]
                pub const fn pack_in(value: $bits) -> $packing {
                    $packing::new(value)
                }

                /// Returns a packer for packing a value into the first `bits` bits.
                pub const fn least_significant(n: u32) -> Self {
                    Self {
                        mask: Self::mask(n),
                        shift: 0,
                    }
                }

                /// Returns a packer for packing a value into the next more-significant
                /// `n` from `self`.
                pub const fn next(&self, n: u32) -> Self {
                    let shift = Self::SIZE_BITS - self.mask.leading_zeros();
                    let mask = Self::mask(n) << shift;
                    Self { mask, shift }
                }


                /// Returns a packer for packing a value into the next `n` more-significant
                ///  after the `bit`th bit.
                pub const fn starting_at(bit: u32, n: u32) -> Self {
                    let shift = bit.saturating_sub(1);
                    let mask = Self::mask(n) << shift;
                    Self { shift, mask }
                }

                /// Returns a packer that will pack a value into the provided mask.
                pub const fn from_mask(mask: $bits) -> Self {
                    let shift = mask.leading_zeros();
                    let mask = mask >> shift;
                    Self { mask, shift }
                }

                /// This is a `const fn`-compatible equivalent of
                /// [`Self::from_range`]. Note that it can only be used with
                /// [`core::ops::Range`]s, and not with
                /// [`core::ops::RangeInclusive`], [`core::ops::RangeTo`],
                /// [`core::ops::RangeFrom`],  [`core::ops::RangeToInclusive`]. :(
                pub const fn from_const_range(range: Range<u32>) -> Self {
                    Self::starting_at(range.start, range.end.saturating_sub(range.start))
                }

                /// Construct a bit packing spec from a range of bits.
                ///
                /// # Panics
                ///
                /// - If the range does not fit within the integer type packed
                ///   by this packing spec.
                /// - If the range's start > the range's end (although most
                ///   range types should prevent this).
                pub fn from_range(range: impl RangeBounds<u32>) -> Self {
                    use Bound::*;
                    let start = match range.start_bound() {
                        Included(&bit) => bit,
                        Excluded(&bit) => bit + 1,
                        Unbounded => 0,
                    };
                    assert!(
                        start < Self::SIZE_BITS,
                        "range start value ({}) must be less than the maximum number of bits in a `u{}`",
                        start,
                        Self::SIZE_BITS,
                    );
                    let end = match range.end_bound() {
                        Included(&bit) => bit,
                        Excluded(&bit) => bit - 1,
                        Unbounded => Self::SIZE_BITS,
                    };
                    assert!(
                        end <= Self::SIZE_BITS,
                        "range end value ({}) must be less than or equal to the maximum number of bits in a `u{}`",
                        end,
                        Self::SIZE_BITS,
                    );
                    debug_assert!(
                        start <= end,
                        "range end value ({}) may not be greater than range start value ({})",
                        start,
                        end,
                    );
                    Self::starting_at(start, end.saturating_sub(start))
                }

                pub const fn pair_at(&self, at: u32) -> $pair {
                    
                } 

                /// Returns the number of bits needed to pack this value.
                pub const fn bits(&self) -> u32 {
                    Self::SIZE_BITS - (self.mask >> self.shift).leading_zeros()
                }

                pub const fn max_value(&self) -> $bits {
                    (1 << self.bits()) - 1
                }

                /// Pack the [`self.bits()`] least-significant bits from `value` into `base`.
                ///
                /// Any bits more significant than the [`self.bits()`]-th bit are ignored.
                ///
                /// [`self.bits()`]: Self::bits
                pub const fn pack_truncating(&self, value: $bits, base: $bits) -> $bits {
                    let value = value & self.max_value();
                    // other bits from `base` we don't want to touch
                    let rest = base & !self.mask;
                    rest | (value << self.shift)
                }

                /// Pack the [`self.bits()`] least-significant bits from `value` into `base`.
                ///
                /// # Panics
                ///
                /// Panics if any other bits outside of [`self.bits()`] are set
                /// in `value`.
                ///
                /// [`self.bits()`]: Self::bits
                pub fn pack(&self, value: $bits, base: $bits) -> $bits {
                    assert!(
                        value <= self.max_value(),
                        "bits outside of packed range are set!\n     value: {:0x},\n max_value: {:0x}",
                        value,
                        self.max_value(),
                    );
                    self.pack_truncating(value, base)
                }

                /// Set _all_ bits packed by this packer to 1.
                ///
                /// This is a convenience function for
                /// ```rust,ignore
                /// self.pack(self.max_value(), base)
                /// ```
                pub const fn set_all(&self, base: $bits) -> $bits {
                    // Note: this will never truncate (the reason why is left
                    // as an exercise to the reader).
                    self.pack_truncating(self.max_value(), base)
                }

                /// Set _all_ bits packed by this packer to 0.
                ///
                /// This is a convenience function for
                /// ```rust,ignore
                /// self.pack(0, base)
                /// ```
                pub const fn unset_all(&self, base: $bits) -> $bits {
                    // may be slightly faster than actually calling
                    // `self.pack(0, base)` when not const-evaling
                    base & !self.mask
                }

                /// Unpack this packer's bits from `source`.
                pub const fn unpack(&self, src: $bits) -> $bits {
                    (src & self.mask) >> self.shift
                }
            }

            impl fmt::Debug for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($name))
                        .field("mask", &format_args!("{:#b}", self.mask))
                        .field("shift", &self.shift)
                        .finish()
                }
            }

            impl fmt::UpperHex for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($name))
                        .field("mask", &format_args!("{:#X}", self.mask))
                        .field("shift", &self.shift)
                        .finish()
                }
            }

            impl fmt::LowerHex for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($name))
                        .field("mask", &format_args!("{:#x}", self.mask))
                        .field("shift", &self.shift)
                        .finish()
                }
            }

            impl fmt::Binary for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($name))
                        .field("mask", &format_args!("{:#b}", self.mask))
                        .field("shift", &self.shift)
                        .finish()
                }
            }

            impl<R: RangeBounds<u32>> From<R> for $name {
                fn from(range: R) -> Self {
                    Self::from_range(range)
                }
            }

            // === packing type ===

            impl $packing {
                #[doc = concat!(
                    "Wrap a [`",
                    stringify!($bits),
                    "`] to add methods for packing bit ranges using [`",
                    stringify!($name),
                    "`]."
                )]
                pub const fn new(bits: $bits) -> Self {
                    Self(bits)
                }

                /// Pack bits from `value` into `self`, using the range
                /// specified by `packer`.
                ///
                /// Any bits in `value` outside the range specified by `packer`
                /// are ignored.
                #[inline]
                pub const fn pack_truncating(self, value: $bits, packer: &$name) -> Self {
                    Self(packer.pack_truncating(value, self.0))
                }

                /// Pack bits from `value` into `self`, using the range
                /// specified by `packer`.
                ///
                /// # Panics
                ///
                /// If `value` contains bits outside the range specified by `packer`.
                pub fn pack(self, value: $bits, packer: &$name) -> Self {
                    Self(packer.pack(value, self.0))
                }

                /// Set _all_ bits in the range specified by `packer` to 1 in `self`.
                #[inline]
                pub const fn set_all(self, packer: &$name) -> Self {
                    Self(packer.set_all(self.0))
                }

                /// Set _all_ bits in the range specified by `packer` to 0 in
                /// `self`.
                #[inline]
                pub const fn unset_all(self, packer: &$name) -> Self {
                    Self(packer.unset_all(self.0))
                }

                /// Finish packing bits into `self`, returning the wrapped
                /// value.
                #[inline]
                pub const fn bits(self) -> $bits {
                    self.0
                }
            }

            impl fmt::Debug for $packing {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_tuple(stringify!($packing))
                        .field(&format_args!("{:#b}", self.0))
                        .finish()
                }
            }

            impl fmt::UpperHex for $packing {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_tuple(stringify!($packing))
                        .field(&format_args!("{:X}", self.0))
                        .finish()
                }
            }

            impl fmt::LowerHex for $packing {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_tuple(stringify!($packing))
                    .field(&format_args!("{:#x}", self.0))
                    .finish()
                }
            }

            impl fmt::Binary for $packing {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_tuple(stringify!($packing))
                        .field(&format_args!("{:#b}", self.0))
                        .finish()
                }
            }

            impl From<$bits> for $packing {
                fn from(bits: $bits) -> Self {
                    Self(bits)
                }
            }

            impl From<$packing> for $bits {
                fn from(packing: $packing) -> Self {
                    packing.0
                }
            }
        )+
    }
}

make_packers! {
    pub struct Pack64 { bits: u64, packing: Packing64, pair: Pair64, }
    pub struct Pack32 { bits: u32, packing: Packing32, pair: Pair32 }
    pub struct Pack16 { bits: u16, packing: Packing16, pair: Pair16 }
    pub struct Pack8 { bits: u8, packing: Packing8, pair: Pair8 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    macro_rules! test_pack_unpack {
        ($(fn $fn:ident<$typ:ident, $bits_ty:ty>($max:expr);)+) => {
            proptest! {
                $(
                    #[test]
                    fn $fn(
                        (nbits, val1, val2, base) in (1u32..($max/2)).prop_flat_map(|nbits| (
                            Just(nbits),
                            proptest::bits::u64::between(0, nbits as usize - 1),
                            proptest::bits::u64::between(0, nbits as usize - 1),
                            any::<$bits_ty>(),
                        )),
                    ) {
                        let val1 = val1 as $bits_ty;
                        let val2 = val2 as $bits_ty;
                        let pack1 = $typ::least_significant(nbits);
                        let pack2 = pack1.next(nbits);

                        let packed1 = pack1.pack(val1, base);
                        prop_assert_eq!(pack1.unpack(packed1), val1);

                        let packed2 = pack2.pack(val1, base);
                        prop_assert_eq!(pack2.unpack(packed2), val1);

                        let packed3 = pack1.pack(val1, pack2.pack(val2, base));
                        prop_assert_eq!(pack1.unpack(packed3), val1);
                        prop_assert_eq!(pack2.unpack(packed3), val2);
                    }
                )+
            }
        };
    }

    macro_rules! test_pack_methods {
        ($(fn $fn:ident<$typ:ident, $bits_ty:ty>($max:expr);)+) => {
            proptest! {
                $(
                    #[test]
                    fn $fn(
                        (nbits, val1, val2, base) in (1u32..($max/2)).prop_flat_map(|nbits| (
                            Just(nbits),
                            proptest::bits::u64::between(0, nbits as usize - 1),
                            proptest::bits::u64::between(0, nbits as usize - 1),
                            any::<$bits_ty>(),
                        )),
                    ) {
                        let val1 = val1 as $bits_ty;
                        let val2 = val2 as $bits_ty;
                        let pack1 = $typ::least_significant(nbits);
                        let pack2 = pack1.next(nbits);


                        let packed_methods = $typ::pack_in(base)
                            .pack(val1, &pack1)
                            .pack(val2, &pack2)
                            .bits();
                        let packed_calls = pack1.pack(val1, pack2.pack(val2, base));
                        prop_assert_eq!(packed_methods, packed_calls);
                    }
                )+
            }
        };
    }

    macro_rules! test_from_range {
        ($(fn $fn:ident<$typ:ident, $bits_ty:ty>($max:expr);)+) => {
            proptest! {
                $(
                    #[test]
                    fn $fn(
                        (start, len) in (1u32..($max-1)).prop_flat_map(|start| (
                            Just(start),
                            (1..($max - start)),
                        )),
                    ) {
                        let range_inclusive = start..=(start + len);
                        let range_exclusive = start..(start + len + 1);
                        println!("start={}; len={}; range_inclusive={:?}, range_exclusive={:?}", start, len, range_inclusive, range_exclusive);
                        let least_sig = $typ::least_significant(start - 1);
                        let pack_next = least_sig.next(len);
                        let pack_range_inclusive = $typ::from_range(range_inclusive);
                        let pack_range_exclusive = $typ::from_range(range_exclusive);

                        prop_assert_eq!(pack_next, pack_range_inclusive);
                        prop_assert_eq!(pack_next, pack_range_exclusive);
                    }
                )+
            }
        };
    }

    test_pack_unpack! {
        fn pack_unpack_64<Pack64, u64>(64);
        fn pack_unpack_32<Pack32, u32>(32);
        fn pack_unpack_16<Pack16, u16>(16);
        fn pack_unpack_8<Pack8, u8>(8);
    }

    test_pack_methods! {
        fn pack_methods_64<Pack64, u64>(64);
        fn pack_methods_32<Pack32, u32>(32);
        fn pack_methods_16<Pack16, u16>(16);
        fn pack_methods_8<Pack8, u8>(8);
    }

    test_from_range! {
        fn pack_from_range_64<Pack64, u64>(64);
        fn pack_from_range_32<Pack32, u32>(32);
        fn pack_from_range_16<Pack16, u16>(16);
        fn pack_from_range_8<Pack8, u8>(8);
    }
}
