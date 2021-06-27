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
    ($(pub struct $Pack:ident { bits: $Bits:ty, packing: $Packing:ident, pair: $Pair:ident $(,)? })+) => {
        $(

            #[doc = concat!(
                "A spec for packing values into selected bit ranges of [`",
                stringify!($Bits),
                "`] values."
            )]
            #[derive(Copy, Clone, PartialEq, Eq)]
            pub struct $Pack {
                mask: $Bits,
                shift: u32,
            }

            #[doc = concat!(
                "Wraps a [`",
                stringify!($Bits),
                "`] to add methods for packing bit ranges specified by [`",
                stringify!($Pack),
                "`]."
            )]
            #[derive(Copy, Clone, PartialEq, Eq)]
            pub struct $Packing($Bits);

            #[derive(Copy, Clone, Debug, PartialEq, Eq)]
            pub struct $Pair {
                src: $Pack,
                dst: $Pack,
                dst_shl: $Bits,
                dst_shr: $Bits,
            }

            impl $Pack {
                // XXX(eliza): why is this always `u32`? ask the stdlib i guess...
                const SIZE_BITS: u32 = <$Bits>::MAX.leading_ones();

                /// Returns a value with the first `n` bits set.
                const fn mask(n: u32) -> $Bits {
                    if n == 0 {
                        return 0
                    };
                    let one: $Bits = 1; // lolmacros
                    let shift = one.wrapping_shl(n - 1);
                    shift | (shift.saturating_sub(1))
                }

                const fn shift_next(&self) -> u32 {
                    Self::SIZE_BITS - self.mask.leading_zeros()
                }

                #[doc = concat!(
                    "Wrap a [`",
                    stringify!($Bits),
                    "`] to add methods for packing bit ranges using [`",
                    stringify!($Pack),
                    "`]."
                )]
                #[doc = ""]
                #[doc = concat!(
                    "This is equivalent to calling [`",
                    stringify!($Packing),
                    "::new`], but only requires importing the packer type."
                )]
                pub const fn pack_in(value: $Bits) -> $Packing {
                    $Packing::new(value)
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
                    let shift = self.shift_next();
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
                pub const fn from_mask(mask: $Bits) -> Self {
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

                /// Returns a pair type for packing bits from the range
                /// specified by `self` at the specified offset `at`, which may
                /// differ from `self`'s offset.
                ///
                /// The packing pair can be used to pack bits from one location
                /// into another location, and vice versa.
                pub const fn pair_at(&self, at: u32) -> $Pair {
                    let dst = Self::starting_at(at, self.bits());
                    let at = at.saturating_sub(1);
                    // TODO(eliza): validate that `at + self.bits() < N_BITS` in
                    // const fn somehow lol
                    let (dst_shl, dst_shr) = if at > self.shift {
                        // If the destination is greater than `self`, we need to
                        // shift left.
                        ((at - self.shift) as $Bits, 0)
                    } else {
                        // Otherwise, shift down.
                        (0, (self.shift - at) as $Bits)
                    };
                    $Pair {
                        src: *self,
                        dst,
                        dst_shl,
                        dst_shr,
                    }
                }

                /// Returns a pair type for packing bits from the range
                /// specified by `self` after the specified packing spec.
                pub const fn pair_after(&self, after: &Self) -> $Pair {
                    self.pair_at(after.shift_next())
                }

                /// Returns the number of bits needed to pack this value.
                pub const fn bits(&self) -> u32 {
                    Self::SIZE_BITS - (self.mask >> self.shift).leading_zeros()
                }

                pub const fn max_value(&self) -> $Bits {
                    (1 << self.bits()) - 1
                }

                /// Pack the [`self.bits()`] least-significant bits from `value` into `base`.
                ///
                /// Any bits more significant than the [`self.bits()`]-th bit are ignored.
                ///
                /// [`self.bits()`]: Self::bits
                pub const fn pack_truncating(&self, value: $Bits, base: $Bits) -> $Bits {
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
                pub fn pack(&self, value: $Bits, base: $Bits) -> $Bits {
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
                pub const fn set_all(&self, base: $Bits) -> $Bits {
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
                pub const fn unset_all(&self, base: $Bits) -> $Bits {
                    // may be slightly faster than actually calling
                    // `self.pack(0, base)` when not const-evaling
                    base & !self.mask
                }

                /// Unpack this packer's bits from `source`.
                pub const fn unpack(&self, src: $Bits) -> $Bits {
                    (src & self.mask) >> self.shift
                }

                /// Asserts that this packing pair is valid.
                ///
                /// Because assertions cannot be made in `const fn`, this
                /// performs validating assertions that would ideally be made
                /// when constructing a new instance of this type. When packing
                /// specs are declared as `const`s, this method can be called in
                /// a unit test to ensure that the spec is valid.
                #[track_caller]
                pub fn assert_valid(&self) {
                    self.assert_valid_inner(&"")
                }

                #[track_caller]
                fn assert_valid_inner(&self, cx: &impl fmt::Display) {
                    assert!(
                        self.shift < Self::SIZE_BITS,
                        "shift may not exceed maximum bits for {} (would wrap)\n\
                         -> while checking validity of {:?}{}",
                        stringify!($Bits),
                        self,
                        cx,
                    );
                    assert!(
                        self.bits() <= Self::SIZE_BITS,
                        "number of bits ({}) may not exceed maximum bits for {} (would wrap)\n\
                        -> while checking validity of {:?}{}",
                        self.bits(),
                        stringify!($Bits),
                        self,
                        cx,
                    );
                    assert!(
                        self.bits() + self.shift <= Self::SIZE_BITS,
                        "shift + number of bits ({} + {} = {}) may not exceed maximum bits for {} (would wrap)\n\
                        -> while checking validity of {:?}{}",
                        self.shift,
                        self.bits(),
                        self.bits() + self.shift,
                        stringify!($Bits),
                        self,
                        cx,
                    );

                }
            }

            impl fmt::Debug for $Pack {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($Pack))
                        .field("mask", &format_args!("{:#b}", self.mask))
                        .field("shift", &self.shift)
                        .finish()
                }
            }

            impl fmt::UpperHex for $Pack {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($Pack))
                        .field("mask", &format_args!("{:#X}", self.mask))
                        .field("shift", &self.shift)
                        .finish()
                }
            }

            impl fmt::LowerHex for $Pack {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($Pack))
                        .field("mask", &format_args!("{:#x}", self.mask))
                        .field("shift", &self.shift)
                        .finish()
                }
            }

            impl fmt::Binary for $Pack {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($Pack))
                        .field("mask", &format_args!("{:#b}", self.mask))
                        .field("shift", &self.shift)
                        .finish()
                }
            }

            impl<R: RangeBounds<u32>> From<R> for $Pack {
                fn from(range: R) -> Self {
                    Self::from_range(range)
                }
            }

            // === packing type ===

            impl $Packing {
                #[doc = concat!(
                    "Wrap a [`",
                    stringify!($Bits),
                    "`] to add methods for packing bit ranges using [`",
                    stringify!($Pack),
                    "`]."
                )]
                pub const fn new(bits: $Bits) -> Self {
                    Self(bits)
                }

                /// Pack bits from `value` into `self`, using the range
                /// specified by `packer`.
                ///
                /// Any bits in `value` outside the range specified by `packer`
                /// are ignored.
                #[inline]
                pub const fn pack_truncating(self, value: $Bits, packer: &$Pack) -> Self {
                    Self(packer.pack_truncating(value, self.0))
                }

                /// Pack bits from `value` into `self`, using the range
                /// specified by `packer`.
                ///
                /// # Panics
                ///
                /// If `value` contains bits outside the range specified by `packer`.
                pub fn pack(self, value: $Bits, packer: &$Pack) -> Self {
                    Self(packer.pack(value, self.0))
                }

                /// Set _all_ bits in the range specified by `packer` to 1 in `self`.
                #[inline]
                pub const fn set_all(self, packer: &$Pack) -> Self {
                    Self(packer.set_all(self.0))
                }

                /// Set _all_ bits in the range specified by `packer` to 0 in
                /// `self`.
                #[inline]
                pub const fn unset_all(self, packer: &$Pack) -> Self {
                    Self(packer.unset_all(self.0))
                }

                /// Finish packing bits into `self`, returning the wrapped
                /// value.
                #[inline]
                pub const fn bits(self) -> $Bits {
                    self.0
                }
            }

            impl fmt::Debug for $Packing {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_tuple(stringify!($Packing))
                        .field(&format_args!("{:#b}", self.0))
                        .finish()
                }
            }

            impl fmt::UpperHex for $Packing {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_tuple(stringify!($Packing))
                        .field(&format_args!("{:X}", self.0))
                        .finish()
                }
            }

            impl fmt::LowerHex for $Packing {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_tuple(stringify!($Packing))
                    .field(&format_args!("{:#x}", self.0))
                    .finish()
                }
            }

            impl fmt::Binary for $Packing {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_tuple(stringify!($Packing))
                        .field(&format_args!("{:#b}", self.0))
                        .finish()
                }
            }

            impl From<$Bits> for $Packing {
                fn from(bits: $Bits) -> Self {
                    Self(bits)
                }
            }

            impl From<$Packing> for $Bits {
                fn from(packing: $Packing) -> Self {
                    packing.0
                }
            }

            // ==== impl Pair ===
            impl $Pair {
                const fn shift_dst(&self, val: $Bits) -> $Bits {
                    (val << self.dst_shl) >> self.dst_shr
                }

                const fn shift_src(&self, val: $Bits) -> $Bits {
                    (val >> self.dst_shl) << self.dst_shr
                }


                /// Returns the "source" member of the packing pair.
                pub const fn src(&self) -> &$Pack {
                    &self.src
                }


                /// Returns the "destination" member of the packing pair.
                pub const fn dst(&self) -> &$Pack {
                    &self.dst
                }

                /// Pack bits from the source location in `src` into the
                /// destination location in `dst`.
                pub const fn pack_from_src(&self, src: $Bits, dst: $Bits) -> $Bits {
                    // extract the bit range from `dst` and shift it over to the
                    // target range in `src`.
                    let bits = self.shift_src(dst & self.dst.mask);
                    // zero packed range in `src`.
                    let src = src & !self.src.mask;
                    src | bits
                }

                /// Pack bits from the destination location in `dst` into the
                /// source location in `src`.
                pub const fn pack_from_dst(&self, src: $Bits, dst: $Bits) -> $Bits {
                    // extract the bit range from `src` and shift it over to
                    // the target range in `dst`.
                    let bits = self.shift_dst(src & self.src.mask);
                    // zero the target range in `dst`.
                    let dst = dst & !self.dst.mask;
                    dst | bits
                }

                /// Asserts that this packing pair is valid.
                ///
                /// Because assertions cannot be made in `const fn`, this
                /// performs validating assertions that would ideally be made
                /// when constructing a new instance of this type. When packing
                /// specs are declared as `const`s, this method can be called in
                /// a unit test to ensure that the spec is valid.
                #[track_caller]
                pub fn assert_valid(&self) {
                    assert_eq!(
                        self.src.bits(), self.dst.bits(),
                        "source and destination packing specs must be the same number of bits wide\n\
                        -> while checking validity of {:?}",
                        self
                    );
                    assert!(
                        self.dst_shl == 0 || self.dst_shr == 0,
                        "destination bits must not be shifted both left and right\n\
                        -> while checking validity of {:?}",
                        self
                    );
                    self.dst.assert_valid_inner(&format_args!("\n-> while checking validity of {:?}", self));
                    self.src.assert_valid_inner(&format_args!("\n-> while checking validity of {:?}", self));
                }
            }

            impl fmt::UpperHex for $Pair {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($Pair))
                        .field("src", &self.src)
                        .field("dst", &self.dst)
                        .field("dst_shl", &self.dst_shl)
                        .field("dst_shr", &self.dst_shr)
                        .finish()
                }
            }

            impl fmt::LowerHex for $Pair {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($Pair))
                        .field("src", &self.src)
                        .field("dst", &self.dst)
                        .field("dst_shl", &self.dst_shl)
                        .field("dst_shr", &self.dst_shr)
                        .finish()
                }
            }

            impl fmt::Binary for $Pair {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.debug_struct(stringify!($Pair))
                        .field("src", &self.src)
                        .field("dst", &self.dst)
                        .field("dst_shl", &self.dst_shl)
                        .field("dst_shr", &self.dst_shr)
                        .finish()
                }
            }
        )+
    }
}

make_packers! {
    pub struct Pack64 { bits: u64, packing: Packing64, pair: Pair64, }
    pub struct Pack32 { bits: u32, packing: Packing32, pair: Pair32, }
    pub struct Pack16 { bits: u16, packing: Packing16, pair: Pair16, }
    pub struct Pack8 { bits: u8, packing: Packing8, pair: Pair8, }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    macro_rules! prop_assert_bits_eq {
        ($left:expr, $right:expr, $state:expr) => {
            let left = $left;
            let right = $right;
            let lstr = stringify!($left);
            let rstr = stringify!($right);
            let expr_len = std::cmp::max(lstr.len(), rstr.len()) + 2;
            let val_len = 80 - (expr_len + 4);
            proptest::prop_assert_eq!(
                left,
                right,
                "\n{:>expr_len$} = {:#0val_len$b}\n{:>expr_len$} = {:#0val_len$b}\n{state}",
                lstr,
                left,
                rstr,
                right,
                expr_len = expr_len,
                val_len = val_len,
                state = $state
            );
        };
        ($left:expr, $right:expr) => {
            prop_assert_bits_eq!($left, $right, "")
        };
    }

    macro_rules! test_pack_unpack {
        ($(fn $fn:ident<$Pack:ident, $Bits:ty>($max:expr);)+) => {
            proptest! {
                $(
                    #[test]
                    fn $fn(
                        (nbits, val1, val2, base) in (1u32..($max/2)).prop_flat_map(|nbits| (
                            Just(nbits),
                            proptest::bits::u64::between(0, nbits as usize - 1),
                            proptest::bits::u64::between(0, nbits as usize - 1),
                            any::<$Bits>(),
                        )),
                    ) {
                        let val1 = val1 as $Bits;
                        let val2 = val2 as $Bits;
                        let pack1 = $Pack::least_significant(nbits);
                        let pack2 = pack1.next(nbits);

                        let packed1 = pack1.pack(val1, base);
                        prop_assert_bits_eq!(pack1.unpack(packed1), val1);

                        let packed2 = pack2.pack(val1, base);
                        prop_assert_bits_eq!(pack2.unpack(packed2), val1);

                        let packed3 = pack1.pack(val1, pack2.pack(val2, base));
                        prop_assert_bits_eq!(pack1.unpack(packed3), val1);
                        prop_assert_bits_eq!(pack2.unpack(packed3), val2);
                    }
                )+
            }
        };
    }

    macro_rules! test_pack_methods {
        ($(fn $fn:ident<$Pack:ident, $Bits:ty>($max:expr);)+) => {
            proptest! {
                $(
                    #[test]
                    fn $fn(
                        (nbits, val1, val2, base) in (1u32..($max/2)).prop_flat_map(|nbits| (
                            Just(nbits),
                            proptest::bits::u64::between(0, nbits as usize - 1),
                            proptest::bits::u64::between(0, nbits as usize - 1),
                            any::<$Bits>(),
                        )),
                    ) {
                        let val1 = val1 as $Bits;
                        let val2 = val2 as $Bits;
                        let pack1 = $Pack::least_significant(nbits);
                        let pack2 = pack1.next(nbits);


                        let packed_methods = $Pack::pack_in(base)
                            .pack(val1, &pack1)
                            .pack(val2, &pack2)
                            .bits();
                        let packed_calls = pack1.pack(val1, pack2.pack(val2, base));
                        prop_assert_bits_eq!(packed_methods, packed_calls);
                    }
                )+
            }
        };
    }

    macro_rules! test_from_range {
        ($(fn $fn:ident<$Pack:ident>($max:expr);)+) => {
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
                        let state = format!(
                            "start={}; len={}; range_inclusive={:?}, range_exclusive={:?}",
                            start, len, range_inclusive, range_exclusive,
                        );
                        let least_sig = $Pack::least_significant(start - 1);
                        let pack_next = least_sig.next(len);
                        let pack_range_inclusive = $Pack::from_range(range_inclusive);
                        let pack_range_exclusive = $Pack::from_range(range_exclusive);

                        prop_assert_bits_eq!(pack_next, pack_range_inclusive, &state);
                        prop_assert_bits_eq!(pack_next, pack_range_exclusive, &state);
                    }
                )+
            }
        };
    }

    // Test packing and unpacking through a pair with other bits zeroed.
    // This just tests that the shift calculations are reasonable.
    macro_rules! test_pair_least_sig_zeroed {
        ($(fn $fn:ident<$Pack:ident, $Bits:ty>($max:expr);)+) => {
            proptest! {
                $(
                    #[test]
                    fn $fn(
                        (src_len, dst_at) in (1u32..($max/2)).prop_flat_map(|nbits| (
                            Just(nbits),
                            (0..$max-nbits),
                        )),
                    ) {
                        let pack_from_src = $Pack::least_significant(src_len);
                        let src = 0;
                        let pack_from_dst = $Pack::starting_at(dst_at, src_len);
                        let dst = pack_from_dst.set_all(0);
                        let pair = pack_from_src.pair_at(dst_at);
                        let state = format!(
                            "src_len={}; dst_at={}; src={:#x}; dst={:#x};\npack_from_dst={:#?}\npair={:#?}",
                            src_len, dst_at, src, dst, pack_from_dst, pair,
                        );

                        let packed = pair.pack_from_src(src, dst);
                        prop_assert_bits_eq!(packed, pack_from_src.set_all(0), state);
                        prop_assert_bits_eq!(pack_from_src.unpack(packed), pack_from_dst.unpack(dst), &state);

                        let dst = <$Bits>::max_value();
                        let packed = pair.pack_from_src(src, dst);
                        prop_assert_bits_eq!(packed, pack_from_src.set_all(0), state);
                        prop_assert_bits_eq!(pack_from_src.unpack(packed), pack_from_dst.unpack(dst), &state);
                    }
                )+
            }
        };
    }

    // Test packing and unpacking through a pair with arbitrary src/dst values.
    // This tests that we don't leave behind unwanted bits, etc.
    macro_rules! test_pair_least_sig_arbitrary {
        ($(fn $fn:ident<$Pack:ident, $Bits:ty>($max:expr);)+) => {
            proptest! {
                $(
                    #[test]
                    fn $fn(
                        (src_len, dst_at, src, dst) in (1u32..($max/2)).prop_flat_map(|nbits| (
                            Just(nbits),
                            (0..$max-nbits),
                            any::<$Bits>(),
                            any::<$Bits>(),
                        )),
                    ) {
                        let pack_from_src = $Pack::least_significant(src_len);
                        let pack_from_dst = $Pack::starting_at(dst_at, src_len);
                        let pair = pack_from_src.pair_at(dst_at);
                        let state = format!(
                            "src_len={}; dst_at={}; src={:#x}; dst={:#x};\npack_from_dst={:#?}\npair={:#?}",
                            src_len, dst_at, src, dst, pack_from_dst, pair,
                        );

                        let packed = pair.pack_from_src(src, dst);
                        prop_assert_bits_eq!(pack_from_src.unpack(packed), pack_from_dst.unpack(dst), &state);

                        let dst_unset = pack_from_dst.unset_all(dst);
                        prop_assert_bits_eq!(pair.pack_from_dst(packed, dst_unset), dst, &state);
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
        fn pack_from_src_range_64<Pack64>(64);
        fn pack_from_src_range_32<Pack32>(32);
        fn pack_from_src_range_16<Pack16>(16);
        fn pack_from_src_range_8<Pack8>(8);
    }

    test_pair_least_sig_zeroed! {
        fn pair_least_sig_zeroed_64<Pack64, u64>(64);
        fn pair_least_sig_zeroed_32<Pack32, u32>(32);
        fn pair_least_sig_zeroed_16<Pack16, u16>(16);
        fn pair_least_sig_zeroed_8<Pack8, u8>(8);
    }

    test_pair_least_sig_arbitrary! {
        fn pair_least_sig_arbitrary_64<Pack64, u64>(64);
        fn pair_least_sig_arbitrary_32<Pack32, u32>(32);
        fn pair_least_sig_arbitrary_16<Pack16, u16>(16);
        fn pair_least_sig_arbitrary_8<Pack8, u8>(8);
    }
}
