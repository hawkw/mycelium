//! Packing spec types.
//!
//! This module provides a set of types to make packing bit ranges easier. These
//! utilities can be used in `const fn`.
//!
//! The bit packing utilities consist of a type that defines a specification for
//! a bit range to pack into, and a wrapper type for an unsigned integer
//! defining methods to pack bit ranges into it. Packing specs are defined for
//! [`usize`], [`u128`], [`u64`], [`u32`], [`u16`], and [`u8`], as
//! [`PackUsize`], [`Pack128`], [`Pack64`], [`Pack32`], [`Pack16`], and
//! [`Pack8`], respectively.
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
//! use mycelium_bitfield::Pack32;
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
//! use mycelium_bitfield::Pack64;
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
//! # use mycelium_bitfield::Pack64;
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
//! # use mycelium_bitfield::Pack64;
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
//! # use mycelium_bitfield::Pack64;
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
//! use mycelium_bitfield::Pack64;
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
//!
use super::FromBits;
use core::{
    any::type_name,
    fmt,
    marker::PhantomData,
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
            #[doc = ""]
            #[doc = "See the [module-level documentation](crate::pack) for details on using packing specs."]
            pub struct $Pack<T = $Bits, F = ()> {
                mask: $Bits,
                shift: u32,
                _dst_ty: PhantomData<fn(&T, &F)>,
            }

            #[doc = concat!(
                "Wraps a [`",
                stringify!($Bits),
                "`] to add methods for packing bit ranges specified by [`",
                stringify!($Pack),
                "`]."
            )]
            #[doc = ""]
            #[doc = "See the [module-level documentation](crate::pack) for details on using packing specs."]
            #[derive(Copy, Clone, PartialEq, Eq)]
            pub struct $Packing($Bits);

            #[doc = concat!(
                "A pair of [",
                stringify!($Pack),
                "]s, allowing a bit range to be unpacked from one offset in a [",
                stringify!($Bits),
                "] value, and packed into a different offset in a different value."
            )]
            #[doc = ""]
            #[doc = "See the [module-level documentation](crate::pack) for details on using packing specs."]
            pub struct $Pair<T = $Bits> {
                src: $Pack<T>,
                dst: $Pack<T>,
                dst_shl: $Bits,
                dst_shr: $Bits,
            }

            impl $Pack<$Bits> {
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
                        mask: Self::mk_mask(n),
                        shift: 0,
                        _dst_ty: core::marker::PhantomData,
                    }
                }


                /// Returns a packer that will pack a value into the provided mask.
                pub const fn from_mask(mask: $Bits) -> Self {
                    let shift = mask.leading_zeros();
                    let mask = mask >> shift;
                    Self { mask, shift, _dst_ty: core::marker::PhantomData, }
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
            }

            impl<T, F> $Pack<T, F> {
                // XXX(eliza): why is this always `u32`? ask the stdlib i guess...
                const SIZE_BITS: u32 = <$Bits>::MAX.leading_ones();

                /// Returns a value with the first `n` bits set.
                const fn mk_mask(n: u32) -> $Bits {
                    if n == 0 {
                        return 0
                    }
                    let one: $Bits = 1; // lolmacros
                    let shift = one.wrapping_shl(n - 1);
                    shift | (shift.saturating_sub(1))
                }

                const fn shift_next(&self) -> u32 {
                    Self::SIZE_BITS - self.mask.leading_zeros()
                }

                #[doc(hidden)]
                pub const fn typed<T2, F2>(self) -> $Pack<T2, F2>
                where
                    T2: FromBits<$Bits>
                {
                    assert!(T2::BITS >= self.bits());
                    $Pack {
                        shift: self.shift,
                        mask: self.mask,
                        _dst_ty: PhantomData,
                    }
                }


                /// Returns the number of bits needed to pack this value.
                pub const fn bits(&self) -> u32 {
                    Self::SIZE_BITS - (self.mask >> self.shift).leading_zeros()
                }

                /// Returns the maximum value of this packing spec (i.e. a value
                /// with all the bits set)
                pub const fn max_value(&self) -> $Bits {
                    (1 << self.bits()) - 1
                }

                /// Returns a value with the first bit in this packing spec set.
                #[inline]
                pub const fn first_bit(&self) -> $Bits {
                    1 << self.shift
                }

                /// Returns a raw, shifted mask for unpacking this packing spec.
                #[inline]
                pub const fn raw_mask(&self) -> $Bits {
                    self.mask
                }

                /// Pack the [`self.bits()`] least-significant bits from `value` into `base`.
                ///
                /// Any bits more significant than the [`self.bits()`]-th bit are ignored.
                ///
                /// [`self.bits()`]: Self::bits
                #[inline]
                pub const fn pack_truncating(&self, value: $Bits, base: $Bits) -> $Bits {
                    let value = value & self.max_value();
                    // other bits from `base` we don't want to touch
                    let rest = base & !self.mask;
                    rest | (value << self.shift)
                }

                /// Pack the [`self.bits()`] least-significant bits from `value`
                /// into `base`, mutating `base`.
                ///
                /// Any bits more significant than the [`self.bits()`]-th bit are ignored.
                ///
                /// [`self.bits()`]: Self::bits
                #[inline]
                pub fn pack_into_truncating<'base>(&self, value: $Bits, base: &'base mut $Bits) -> &'base mut $Bits {
                    let value = value & self.max_value();
                    *base &= !self.mask;
                    *base |= (value << self.shift);
                    base
                }

                /// Returns a new packer for packing a `T2`-typed value in the
                /// next [`T2::BITS`](crate::FromBits::BITS) bits after `self`.
                pub const fn then<T2>(&self) -> $Pack<T2, F>
                where
                    T2: FromBits<$Bits>
                {
                    self.next(T2::BITS).typed()
                }

                /// Returns a packer for packing a value into the next more-significant
                /// `n` from `self`.
                pub const fn next(&self, n: u32) -> $Pack<$Bits, F> {
                    let shift = self.shift_next();
                    let mask = Self::mk_mask(n) << shift;
                    $Pack { mask, shift, _dst_ty: core::marker::PhantomData, }
                }

                /// Returns a packer for packing a value into all the remaining
                /// more-significant bits after `self`.
                pub const fn remaining(&self) -> $Pack<$Bits, F> {
                    let shift = self.shift_next();
                    let n = Self::SIZE_BITS - shift;
                    let mask = Self::mk_mask(n) << shift;
                    $Pack { mask, shift, _dst_ty: core::marker::PhantomData, }
                }


                /// Set _all_ bits packed by this packer to 1.
                ///
                /// This is a convenience function for
                /// ```rust,ignore
                /// self.pack(self.max_value(), base)
                /// ```
                #[inline]
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
                #[inline]
                pub const fn unset_all(&self, base: $Bits) -> $Bits {
                    // may be slightly faster than actually calling
                    // `self.pack(0, base)` when not const-evaling
                    base & !self.mask
                }

                /// Set _all_ bits packed by this packer to 1 in `base`.
                ///
                /// This is a convenience function for
                /// ```rust,ignore
                /// self.pack_into(self.max_value(), base)
                /// ```
                #[inline]
                pub fn set_all_in<'base>(&self, base: &'base mut $Bits) -> &'base mut $Bits {
                    // Note: this will never truncate (the reason why is left
                    // as an exercise to the reader).
                    self.pack_into_truncating(self.max_value(), base)
                }

                /// Set _all_ bits packed by this packer to 0.
                ///
                /// This is a convenience function for
                /// ```rust,ignore
                /// self.pack_into(0, base)
                /// ```
                #[inline]
                pub fn unset_all_in<'base>(&self, base: &'base mut $Bits) ->  &'base mut $Bits {
                    // may be slightly faster than actually calling
                    // `self.pack(0, base)` when not const-evaling
                    *base &= !self.mask;
                    base
                }


                /// Unpack this packer's bits from `source`.
                #[inline]
                pub const fn unpack_bits(&self, src: $Bits) -> $Bits {
                    (src & self.mask) >> self.shift
                }


                /// Returns `true` if **any** bits specified by this packing spec
                /// are set in `src`.
                #[inline]
                pub const fn contained_in_any(&self, bits: $Bits) -> bool {
                    bits & self.mask != 0
                }


                /// Returns `true` if **all** bits specified by this packing spec
                /// are set in `src`.
                #[inline]
                pub const fn contained_in_all(&self, bits: $Bits) -> bool {
                    bits & self.mask == self.mask
                }

                /// Asserts that this packing spec is valid.
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


                /// Assert all of a set of packing specs are valid for packing
                /// and unpacking values into the same bitfield.
                ///
                /// This asserts that each individual packing spec is valid (by
                /// calling [`assert_valid`](Self::assert_valid) on that spec),
                /// and asserts that no two packing specs in `specs` overlap
                /// (indicating that they can safely represent a single
                /// bitfield's subranges).
                ///
                /// This function takes a slice of `(&str, Self)` tuples, with
                /// the `&str`s providing a name for each packing spec. This name
                /// is used to refer to that packing spec in panic messages.
                #[track_caller]
                pub fn assert_all_valid(specs: &[(&str, Self)]) {
                    for (name, spec) in specs {
                        spec.assert_valid_inner(&format_args!(" ({name})"));
                        for (other_name, other_spec) in specs {
                            // Don't test if this spec overlaps with itself ---
                            // they obviously overlap.
                            if name == other_name {
                                continue;
                            }
                            if spec.raw_mask() & other_spec.raw_mask() > 0 {
                                let maxlen = core::cmp::max(name.len(), other_name.len());
                                panic!(
                                    "mask for {name} overlaps with {other_name}\n\
                                    {name:>width$} = {this_mask:#b}\n\
                                    {other_name:>width$} = {that_mask:#b}",
                                    name = name,
                                    other_name = other_name,
                                    this_mask = spec.raw_mask(),
                                    that_mask = other_spec.raw_mask(),
                                    width = maxlen + 2,
                                );
                            }
                        }
                    }
                }

                /// Returns the index of the least-significant bit of this
                /// packing spec (i.e. the bit position of the start of the
                /// packed range).
                pub const fn least_significant_index(&self) -> u32 {
                    self.shift
                }

                /// Returns the index of the most-significant bit of this
                /// packing spec (i.e. the bit position of the end of the
                /// packed range).
                ///
                /// This will always be greater than the value returned by
                /// [`least_significant_index`](Self::least_significant_index).
                pub const fn most_significant_index(&self) -> u32 {
                    Self::SIZE_BITS - self.mask.leading_zeros()
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
                    assert_eq!(self.most_significant_index() - self.least_significant_index(), self.bits(),
                    "most_significant_index - least_significant_index ({} + {} = {}) must equal total number of bits ({})\n\
                    -> while checking validity of {:?}{}",
                        self.most_significant_index(),
                        self.least_significant_index(),
                        self.most_significant_index() - self.least_significant_index(), self.bits(),
                        self, cx
                    )
                }
            }

            impl<T, F> $Pack<T, F>
            where
                T: FromBits<$Bits>,
            {
                /// Returns a packing spec for packing a `T`-typed value in the
                /// first [`T::BITS`](FromBits::BITS) least-significant bits.
                pub const fn first() -> Self {
                    $Pack::<$Bits, ()>::least_significant(T::BITS).typed()
                }

                /// Returns a packer for packing a value into the next `n` more-significant
                /// after the `bit`th bit.
                pub const fn starting_at(bit: u32, n: u32) -> Self {
                    let shift = bit.saturating_sub(1);
                    let mask = Self::mk_mask(n) << shift;
                    Self { mask, shift, _dst_ty: PhantomData, }
                }

                /// Returns a pair type for packing bits from the range
                /// specified by `self` at the specified offset `at`, which may
                /// differ from `self`'s offset.
                ///
                /// The packing pair can be used to pack bits from one location
                /// into another location, and vice versa.
                pub const fn pair_at(&self, at: u32) -> $Pair<T> {
                    let dst = Self::starting_at(at, self.bits());
                    self.pair_with(dst)
                }

                /// Returns a pair type for packing bits from the range
                /// specified by `self` after the specified packing spec.
                pub const fn pair_after(&self, after: Self) -> $Pair<T> {
                    self.pair_at(after.shift_next())
                }

                /// Returns a pair type for packing bits from the range
                /// specified by `self` into the range specified by `with`.
                ///
                /// # Note
                ///
                /// The two ranges must be the same size. This can be asserted
                /// by the `assert_valid` method on the returned pair type.
                pub const fn pair_with<F2>(&self, dst: $Pack<T, F2>) -> $Pair<T> {
                    // TODO(eliza): validate that `dst.shift + self.bits() < N_BITS` in
                    // const fn somehow lol
                    let (dst_shl, dst_shr) = if dst.shift > self.shift {
                        // If the destination is greater than `self`, we need to
                        // shift left.
                        ((dst.shift - self.shift) as $Bits, 0)
                    } else {
                        // Otherwise, shift down.
                        (0, (self.shift - dst.shift) as $Bits)
                    };

                    $Pair {
                        src: self.typed(),
                        dst: dst.typed(),
                        dst_shl,
                        dst_shr,
                    }
                }

                /// Pack the [`self.bits()`] least-significant bits from `value` into `base`.
                ///
                /// # Panics
                ///
                /// Panics if any other bits outside of [`self.bits()`] are set
                /// in `value`.
                ///
                /// [`self.bits()`]: Self::bits
                pub fn pack(&self, value: T, base: $Bits) -> $Bits {
                    let value = value.into_bits();
                    assert!(
                        value <= self.max_value(),
                        "bits outside of packed range are set!\n     value: {:#b},\n max_value: {:#b}",
                        value,
                        self.max_value(),
                    );
                    self.pack_truncating(value, base)
                }

                /// Pack the [`self.bits()`] least-significant bits from `value`
                /// into `base`, mutating `base`.
                ///
                /// # Panics
                ///
                /// Panics if any other bits outside of [`self.bits()`] are set
                /// in `value`.
                ///
                /// [`self.bits()`]: Self::bits
                pub fn pack_into<'base>(&self, value: T, base: &'base mut $Bits) -> &'base mut $Bits {
                    let value = value.into_bits();
                    assert!(
                        value <= self.max_value(),
                        "bits outside of packed range are set!\n     value: {:#b},\n max_value: {:#b}",
                        value,
                        self.max_value(),
                    );
                    *base &= !self.mask;
                    *base |= (value << self.shift);
                    base
                }

                /// Attempts to unpack a `T`-typed value from `src`.
                ///
                /// # Returns
                ///
                /// - `Ok(T)` if a `T`-typed value could be constructed from the
                ///   bits in `src`
                /// - `Err(T::Error)` if `src` does not contain a valid bit
                ///   pattern for a `T`-typed value, as determined by `T`'s
                ///   [`FromBits::try_from_bits`] implementation.
                pub fn try_unpack(&self, src: $Bits) -> Result<T, T::Error> {
                    T::try_from_bits(self.unpack_bits(src))
                }

                /// Unpacks a `T`-typed value from `src`.
                ///
                /// # Panics
                ///
                /// This method panics if `src` does not contain a valid bit
                /// pattern for a `T`-typed value, as determined by `T`'s
                /// [`FromBits::try_from_bits`] implementation.
                pub fn unpack(&self, src: $Bits) -> T
                where
                    T: FromBits<$Bits>,
                {
                    let bits = self.unpack_bits(src);
                    match T::try_from_bits(bits) {
                        Ok(value) => value,
                        Err(e) => panic!("failed to construct {} from bits {:#b} ({}): {}", type_name::<T>(), bits, bits, e),
                    }
                }
            }

            impl<T, F> Clone for $Pack<T, F> {
                fn clone(&self) -> Self {
                   *self
                }
            }

            impl<T, F> Copy for $Pack<T, F> {}

            impl<T, F> fmt::Debug for $Pack<T, F> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    let Self { mask, shift, _dst_ty } = self;
                    f.debug_struct(stringify!($Pack))
                        .field("mask", &format_args!("{:#b}", mask))
                        .field("shift", shift)
                        .field("dst_type", &format_args!("{}", type_name::<T>()))
                        .finish()
                }
            }

            impl<T, F> fmt::UpperHex for $Pack<T, F> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    let Self { mask, shift, _dst_ty } = self;
                    f.debug_struct(stringify!($Pack))
                        .field("mask", &format_args!("{:#X}", mask))
                        .field("shift", shift)
                        .field("dst_type", &format_args!("{}", type_name::<T>()))
                        .finish()
                }
            }

            impl<T, F> fmt::LowerHex for $Pack<T, F> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    let Self { mask, shift, _dst_ty } = self;
                    f.debug_struct(stringify!($Pack))
                        .field("mask", &format_args!("{:#x}", mask))
                        .field("shift", shift)
                        .field("dst_type", &format_args!("{}", type_name::<T>()))
                        .finish()
                }
            }

            impl<T, F> fmt::Binary for $Pack<T, F> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    let Self { mask, shift, _dst_ty } = self;
                    f.debug_struct(stringify!($Pack))
                        .field("mask", &format_args!("{:#b}", mask))
                        .field("shift", shift)
                        .field("dst_type", &format_args!("{}", type_name::<T>()))
                        .finish()
                }
            }

            impl<R: RangeBounds<u32>> From<R> for $Pack {
                fn from(range: R) -> Self {
                    Self::from_range(range)
                }
            }

            impl<A, B, F> PartialEq<$Pack<B, F>> for $Pack<A, F> {
                #[inline]
                fn eq(&self, other: &$Pack<B, F>) -> bool {
                    self.mask == other.mask && self.shift == other.shift
                }
            }

            impl<A, B, F> PartialEq<&'_ $Pack<B, F>> for $Pack<A, F> {
                #[inline]
                fn eq(&self, other: &&'_ $Pack<B, F>) -> bool {
                    <Self as PartialEq<$Pack<B, F>>>::eq(self, *other)
                }
            }

            impl<A, B, F> PartialEq<$Pack<B, F>> for &'_ $Pack<A, F> {
                #[inline]
                fn eq(&self, other: &$Pack<B, F>) -> bool {
                    <$Pack<A, F> as PartialEq<$Pack<B, F>>>::eq(*self, other)
                }
            }

            impl<T, F> Eq for $Pack<T, F> {}

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

                /// Pack bits from `src` into `self`, using the packing pair
                /// specified by `pair`, with `self` serving as the "destination" member
                /// of the pair, and `src` serving as the "source" member of the
                /// pair.
                #[inline]
                pub const fn pack_from_src(self, value: $Bits, pair: &$Pair) -> Self {
                    Self(pair.pack_from_src(self.0, value))
                }

                /// Pack bits from `dst` into `self`, using the packing pair
                /// specified by `pair`, with `self` serving as the "siyrce" member
                /// of the pair, and `dst` serving as the "destination" member of the
                /// pair.
                #[inline]
                pub const fn pack_from_dst(self, value: $Bits, pair: &$Pair) -> Self {
                    Self(pair.pack_from_dst(value, self.0))
                }


                /// Pack bits from `value` into `self`, using the range
                /// specified by `packer`.
                ///
                /// # Panics
                ///
                /// If `value` contains bits outside the range specified by `packer`.
                pub fn pack<T: FromBits<$Bits>, F>(self, value: T, packer: &$Pack<T, F>) -> Self {
                    Self(packer.pack(value, self.0))
                }

                /// Set _all_ bits in the range specified by `packer` to 1 in `self`.
                #[inline]
                pub const fn set_all<T, F>(self, packer: &$Pack<T, F>) -> Self {
                    Self(packer.set_all(self.0))
                }

                /// Set _all_ bits in the range specified by `packer` to 0 in
                /// `self`.
                #[inline]
                pub const fn unset_all<T, F>(self, packer: &$Pack<T, F>) -> Self {
                    Self(packer.unset_all(self.0))
                }


                /// Returns `true` if **any** bits specified by `packer` are set
                /// in `self`.
                #[inline]
                pub const fn contains_any<T, F>(self, packer: &$Pack<T, F>) -> bool {
                    packer.contained_in_any(self.0)
                }


                /// Returns `true` if **any** bits specified by `packer` are set
                /// in `self`.
                #[inline]
                pub const fn contains_all<T, F>(self, packer: &$Pack<T, F>) -> bool {
                    packer.contained_in_all(self.0)
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

            impl<T> Clone for $Pair<T> {
                fn clone(&self) -> Self {
                    *self
                }
            }

            impl<T> Copy for $Pair<T> {}

            impl<T> fmt::Debug for $Pair<T> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    let Self { src, dst, dst_shl, dst_shr } = self;
                    f.debug_struct(stringify!($Pair))
                        .field("src", src)
                        .field("dst", dst)
                        .field("dst_shl", dst_shl)
                        .field("dst_shr", dst_shr)
                        .finish()
                }
            }

            impl<T> fmt::UpperHex for $Pair<T> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    let Self { src, dst, dst_shl, dst_shr } = self;
                    f.debug_struct(stringify!($Pair))
                        .field("src", src)
                        .field("dst", dst)
                        .field("dst_shl", dst_shl)
                        .field("dst_shr", dst_shr)
                        .finish()
                }
            }

            impl<T> fmt::LowerHex for $Pair<T> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    let Self { src, dst, dst_shl, dst_shr } = self;
                    f.debug_struct(stringify!($Pair))
                        .field("src", src)
                        .field("dst", dst)
                        .field("dst_shl", dst_shl)
                        .field("dst_shr", dst_shr)
                        .finish()
                }
            }

            impl<T> fmt::Binary for $Pair<T> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    let Self { src, dst, dst_shl, dst_shr } = self;
                    f.debug_struct(stringify!($Pair))
                        .field("src", src)
                        .field("dst", dst)
                        .field("dst_shl", dst_shl)
                        .field("dst_shr", dst_shr)
                        .finish()
                }
            }

            impl<A, B> PartialEq<$Pair<B>> for $Pair<A> {
                #[inline]
                fn eq(&self, other: &$Pair<B>) -> bool {
                    self.src == other.src && self.dst == other.dst
                }
            }

            impl<A, B> PartialEq<&'_ $Pair<B>> for $Pair<A> {
                #[inline]
                fn eq(&self, other: &&'_ $Pair<B>) -> bool {
                    <Self as PartialEq<$Pair<B>>>::eq(self, *other)
                }
            }

            impl<A, B> PartialEq<$Pair<B>> for &'_ $Pair<A> {
                #[inline]
                fn eq(&self, other: &$Pair<B>) -> bool {
                    <$Pair<A> as PartialEq<$Pair<B>>>::eq(*self, other)
                }
            }


            impl<T> Eq for $Pair<T> {}
        )+
    }
}

make_packers! {
    pub struct PackUsize { bits: usize, packing: PackingUsize, pair: PairUsize }
    pub struct Pack128 { bits: u128, packing: Packing128, pair: Pair128 }
    pub struct Pack64 { bits: u64, packing: Packing64, pair: Pair64, }
    pub struct Pack32 { bits: u32, packing: Packing32, pair: Pair32, }
    pub struct Pack16 { bits: u16, packing: Packing16, pair: Pair16, }
    pub struct Pack8 { bits: u8, packing: Packing8, pair: Pair8, }
}

#[cfg(all(test, not(loom)))]
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
                        prop_assert_bits_eq!(pack1.unpack_bits(packed1), val1);

                        let packed2 = pack2.pack(val1, base);
                        prop_assert_bits_eq!(pack2.unpack_bits(packed2), val1);

                        let packed3 = pack1.pack(val1, pack2.pack(val2, base));
                        prop_assert_bits_eq!(pack1.unpack_bits(packed3), val1);
                        prop_assert_bits_eq!(pack2.unpack_bits(packed3), val2);
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
        ($(fn $fn:ident<$Pack:ident, $Bits:ty>($max:expr);)+) => {
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
                        let pack_from_dst = $Pack::<$Bits>::starting_at(dst_at, src_len);
                        let dst = pack_from_dst.set_all(0);
                        let pair = pack_from_src.pair_at(dst_at);
                        let state = format!(
                            "src_len={}; dst_at={}; src={:#x}; dst={:#x};\npack_from_dst={:#?}\npair={:#?}",
                            src_len, dst_at, src, dst, pack_from_dst, pair,
                        );

                        let packed = pair.pack_from_src(src, dst);
                        prop_assert_bits_eq!(packed, pack_from_src.set_all(0), state);
                        prop_assert_bits_eq!(pack_from_src.unpack_bits(packed), pack_from_dst.unpack_bits(dst), &state);

                        let dst = <$Bits>::MAX;
                        let packed = pair.pack_from_src(src, dst);
                        prop_assert_bits_eq!(packed, pack_from_src.set_all(0), state);
                        prop_assert_bits_eq!(pack_from_src.unpack_bits(packed), pack_from_dst.unpack_bits(dst), &state);
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
                        let pack_from_dst = $Pack::<$Bits>::starting_at(dst_at, src_len);
                        let pair = pack_from_src.pair_at(dst_at);
                        let state = format!(
                            "src_len={}; dst_at={}; src={:#x}; dst={:#x};\npack_from_dst={:#?}\npair={:#?}",
                            src_len, dst_at, src, dst, pack_from_dst, pair,
                        );

                        let packed = pair.pack_from_src(src, dst);
                        prop_assert_bits_eq!(pack_from_src.unpack_bits(packed), pack_from_dst.unpack_bits(dst), &state);

                        let dst_unset = pack_from_dst.unset_all(dst);
                        prop_assert_bits_eq!(pair.pack_from_dst(packed, dst_unset), dst, &state);
                    }
                )+
            }
        };
    }

    test_pack_unpack! {
        fn pack_unpack_128<Pack128, u128>(128);
        fn pack_unpack_64<Pack64, u64>(64);
        fn pack_unpack_32<Pack32, u32>(32);
        fn pack_unpack_16<Pack16, u16>(16);
        fn pack_unpack_8<Pack8, u8>(8);
    }

    test_pack_methods! {

        fn pack_methods_128<Pack128, u128>(128);
        fn pack_methods_64<Pack64, u64>(64);
        fn pack_methods_32<Pack32, u32>(32);
        fn pack_methods_16<Pack16, u16>(16);
        fn pack_methods_8<Pack8, u8>(8);
    }

    test_from_range! {
        fn pack_from_src_range_128<Pack128, u128>(128);
        fn pack_from_src_range_64<Pack64, u64>(64);
        fn pack_from_src_range_32<Pack32, u32>(32);
        fn pack_from_src_range_16<Pack16, u16>(16);
        fn pack_from_src_range_8<Pack8, u8>(8);
    }

    test_pair_least_sig_zeroed! {

        fn pair_least_sig_zeroed_128<Pack128, u128>(128);
        fn pair_least_sig_zeroed_64<Pack64, u64>(64);
        fn pair_least_sig_zeroed_32<Pack32, u32>(32);
        fn pair_least_sig_zeroed_16<Pack16, u16>(16);
        fn pair_least_sig_zeroed_8<Pack8, u8>(8);
    }

    test_pair_least_sig_arbitrary! {
        fn pair_least_sig_arbitrary_128<Pack128, u128>(128);
        fn pair_least_sig_arbitrary_64<Pack64, u64>(64);
        fn pair_least_sig_arbitrary_32<Pack32, u32>(32);
        fn pair_least_sig_arbitrary_16<Pack16, u16>(16);
        fn pair_least_sig_arbitrary_8<Pack8, u8>(8);
    }
}
