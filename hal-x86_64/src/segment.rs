use crate::{cpu, task};
use core::{arch::asm, mem};
use mycelium_util::{
    bits::{self, Pack16, Pack64},
    fmt,
};

#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct Selector(u16);

/// A 64-bit mode user segment descriptor.
#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct Descriptor(u64);

/// A Global Descriptor Table (GDT).
///
/// This can have up to 65535 entries, but in 64-bit mode, you don't need most
/// of those (since you can't do real segmentation), so it defaults to 8.
///
// TODO(eliza): i'd like to make the size a u16 to enforce this limit and cast
//   it to `usize` in the array, but this requires unstable const generics
//   features and i didn't want to mess with it...
#[derive(Clone)]
// rustfmt eats default parameters in const generics for some reason (probably a
// bug...)
#[rustfmt::skip]
pub struct Gdt<const SIZE: usize = 8> {
    entries: [u64; SIZE],
    sys_segments: [bool; SIZE],
    push_at: usize,
}

/// A 64-bit mode descriptor for a system segment (such as an LDT or TSS
/// descriptor).
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct SystemDescriptor {
    low: u64,
    high: u64,
}

/// Returns the current code segment selector in `%cs`.
pub fn code_segment() -> Selector {
    let value: u16;
    unsafe { asm!("mov {0:x}, cs", out(reg) value) };
    Selector(value)
}

// === impl Gdt ===

impl<const SIZE: usize> Gdt<SIZE> {
    pub fn load(&'static self) {
        // Create the descriptor table pointer with *just* the actual table, so
        // that the next push index isn't considered a segment descriptor!
        let ptr = cpu::DtablePtr::new(&self.entries);
        tracing::trace!(?ptr, "loading GDT");
        unsafe {
            // Safety: the `'static` bound ensures the GDT isn't going away
            // unless you did something really evil.
            cpu::intrinsics::lgdt(ptr)
        }
        tracing::trace!("loaded GDT!");
    }

    pub const fn new() -> Self {
        Gdt {
            entries: [0; SIZE],
            sys_segments: [false; SIZE],
            push_at: 1,
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    pub fn add_segment(&mut self, segment: Descriptor) -> Selector {
        let ring = segment.ring_bits();
        let idx = self.push(segment.0);
        let selector = Selector::from_raw(Selector::from_index(idx).0 | ring as u16);
        tracing::trace!(idx, ?selector, "added segment");
        selector
    }

    #[tracing::instrument(level = "trace", skip(self))]
    pub fn add_sys_segment(&mut self, segment: SystemDescriptor) -> Selector {
        tracing::trace!(?segment, "Gdt::add_add_sys_segment");
        let idx = self.push(segment.low);
        self.sys_segments[idx as usize] = true;
        self.push(segment.high);
        // sys segments are always ring 0
        let selector = Selector::new(idx, cpu::Ring::Ring0);
        tracing::trace!(idx, ?selector, "added system segment");
        selector
    }

    const fn push(&mut self, entry: u64) -> u16 {
        let idx = self.push_at;
        self.entries[idx] = entry;
        self.push_at += 1;
        idx as u16
    }
}

impl<const SIZE: usize> fmt::Debug for Gdt<SIZE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct GdtEntries<'a, const SIZE: usize>(&'a Gdt<SIZE>);
        impl<const SIZE: usize> fmt::Debug for GdtEntries<'_, SIZE> {
            #[inline]
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut sys0 = None;
                let mut entries = f.debug_list();
                for (&entry, &is_sys) in self.0.entries[..self.0.push_at]
                    .iter()
                    .zip(self.0.sys_segments.iter())
                {
                    if let Some(low) = sys0.take() {
                        entries.entry(&SystemDescriptor { low, high: entry });
                    } else if is_sys {
                        sys0 = Some(entry);
                    } else {
                        entries.entry(&Descriptor(entry));
                    }
                }

                entries.finish()
            }
        }

        f.debug_struct("Gdt")
            .field("capacity", &SIZE)
            .field("len", &(self.push_at - 1))
            .field("entries", &GdtEntries(self))
            .finish()
    }
}

// === impl Selector ===

impl Selector {
    /// The first 2 least significant bits are the selector's priveliege ring.
    const RING: Pack16 = Pack16::least_significant(2);
    /// The next bit is set if this is an LDT segment selector.
    const LDT_BIT: Pack16 = Self::RING.next(1);
    /// The remaining bits are the index in the GDT/LDT.
    const INDEX: Pack16 = Self::LDT_BIT.next(5);

    /// Used by multiple `fmt::Debug` impls, `const`-ified to prevent typos.
    const NAME: &'static str = "segment::Selector";

    pub const fn null() -> Self {
        Self(0)
    }

    pub fn new(idx: u16, ring: cpu::Ring) -> Self {
        Self(
            Pack16::pack_in(0)
                .pack(idx, &Self::INDEX)
                .pack(ring as u8 as u16, &Self::RING)
                .bits(),
        )
    }

    pub const fn from_index(u: u16) -> Self {
        Self(Self::INDEX.pack_truncating(u, 0))
    }

    pub const fn from_raw(u: u16) -> Self {
        Self(u)
    }

    pub fn ring(self) -> cpu::Ring {
        cpu::Ring::from_u8(self.ring_bits())
    }

    /// Separated out from constructing the `cpu::Ring` for use in `const fn`s
    /// (since `Ring::from_u8` panics), but shouldn't be public because it
    /// performs no validation.
    const fn ring_bits(self) -> u8 {
        Self::RING.unpack(self.0) as u8
    }

    /// Returns true if this is an LDT segment selector.
    pub const fn is_ldt(&self) -> bool {
        Self::LDT_BIT.contained_in_any(self.0)
    }

    /// Returns true if this is a GDT segment selector.
    #[inline]
    pub const fn is_gdt(&self) -> bool {
        !self.is_ldt()
    }

    /// Returns the index into the LDT or GDT this selector refers to.
    pub const fn index(&self) -> u16 {
        Self::INDEX.unpack(self.0)
    }

    pub fn set_gdt(&mut self) -> &mut Self {
        Self::LDT_BIT.unset_all_in(&mut self.0);
        self
    }

    pub fn set_ldt(&mut self) -> &mut Self {
        Self::LDT_BIT.set_all_in(&mut self.0);
        self
    }

    pub fn set_ring(&mut self, ring: cpu::Ring) -> &mut Self {
        tracing::trace!("before set_ring: {:b}", self.0);
        Self::RING.pack_into(ring as u16, &mut self.0);
        tracing::trace!("after set_ring: {:b}", self.0);
        self
    }

    pub fn set_index(&mut self, index: u16) -> &mut Self {
        Self::INDEX.pack_into(index, &mut self.0);
        self
    }

    /// Returns this selector's bits as a `u16`.
    pub fn bits(self) -> u16 {
        self.0
    }

    /// Returns the current selector in the `cs` (code segment) register
    pub fn cs() -> Self {
        let sel: u16;
        unsafe {
            asm!("mov {0:x}, cs", out(reg) sel, options(nomem, nostack, preserves_flags));
        }
        Self(sel)
    }

    /// # Safety
    /// lol
    #[inline]
    pub unsafe fn set_cs(self) {
        // because x86 is a very well designed and normal CPU architecture, you
        // can set the value of the `cs` register with a normal `mov`
        // instruction, just like you can with every other segment register.
        //
        // HA HA JUST KIDDING LOL. you can't set the value of `cs` with a `mov`.
        // the only way to set the value of `cs` is by doing a ljmp, a lcall, or
        // a lret with a `cs` selector on the stack (or triggering an interrupt).
        //
        // a thing i think is very cool about the AMD64 CPU Architecture is how
        // we have to do all this cool segmentation bullshit in long mode even
        // though we ... can't ... actually use memory segmentation.
        //
        // see https://wiki.osdev.org/Far_Call_Trick
        tracing::trace!("setting code segment...");
        asm!(
            "push {selector}",
            "lea {retaddr}, [1f + rip]",
            "push {retaddr}",
            "retfq",
            "1:",
            selector = in(reg) self.0 as u64,
            retaddr = lateout(reg) _,
            options(preserves_flags),
        );

        tracing::trace!(selector = fmt::alt(self), "set code segment");
    }

    /// # Safety
    /// lol
    pub unsafe fn set_ss(self) {
        asm!("mov ss, {:x}", in(reg) self.0, options(nostack, preserves_flags));
        tracing::trace!(selector = fmt::alt(self), "set stack segment");
    }

    /// # Safety
    /// lol
    pub unsafe fn set_ds(self) {
        asm!("mov ds, {:x}", in(reg) self.0, options(nostack, preserves_flags));
        tracing::trace!(selector = fmt::alt(self), "set data segment");
    }

    /// # Safety
    /// lol
    pub unsafe fn set_es(self) {
        asm!("mov es, {:x}", in(reg) self.0, options(nostack, preserves_flags));
        tracing::trace!(selector = fmt::alt(self), "set extra segment");
    }

    /// # Safety
    /// lol
    pub unsafe fn set_fs(self) {
        asm!("mov fs, {:x}", in(reg) self.0, options(nostack, preserves_flags));
        tracing::trace!(selector = fmt::alt(self), "set fs");
    }

    /// # Safety
    /// lol
    pub unsafe fn set_gs(self) {
        asm!("mov gs, {:x}", in(reg) self.0, options(nostack, preserves_flags));
        tracing::trace!(selector = fmt::alt(self), "set gs");
    }
}

impl fmt::Debug for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(Self::NAME)
            .field("ring", &self.ring())
            .field("index", &self.index())
            .field("is_gdt", &self.is_gdt())
            .field("bits", &format_args!("{:#b}", self.0))
            .finish()
    }
}

impl fmt::UpperHex for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(Self::NAME)
            .field(&format_args!("{:#X}", self.0))
            .finish()
    }
}

impl fmt::LowerHex for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(Self::NAME)
            .field(&format_args!("{:#x}", self.0))
            .finish()
    }
}

impl fmt::Binary for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(Self::NAME)
            .field(&format_args!("{:#b}", self.0))
            .finish()
    }
}

// === impl Descriptor ===

impl Descriptor {
    /// First 16 bits of the limit field (ignored in 64-bit mode)
    const LIMIT_LOW: Pack64 = Pack64::least_significant(16);
    /// First 24 bits of the base field (ignored in 64-bit mode)
    const BASE_LOW: Pack64 = Self::LIMIT_LOW.next(24);
    /// In order, least to most significant:
    /// - accessed bit
    /// - readable bit for code/writable bit for data
    /// - direction/conforming for code/data
    /// - executable bit (code segment if 1)
    /// - descriptor type (1 for user, 0 for system segments)
    const ACCESS_FLAGS: Pack64 = Self::BASE_LOW.next(5);
    const RING: Pack64 = Self::ACCESS_FLAGS.next(2);
    const PRESENT_BIT: Pack64 = Self::RING.next(1);
    /// High 4 bits of the limit (ignored in 64-bit mode)
    const LIMIT_HIGH: Pack64 = Self::PRESENT_BIT.next(4);
    const FLAGS: Pack64 = Self::LIMIT_HIGH.next(4);
    /// Highest 8 bits of the base field
    const BASE_MID: Pack64 = Self::FLAGS.next(8);

    const LIMIT_LOW_PAIR: bits::Pair64 = Self::LIMIT_LOW.pair_with(limit::LOW);
    const LIMIT_HIGH_PAIR: bits::Pair64 = Self::LIMIT_HIGH.pair_with(limit::HIGH);

    const BASE_LOW_PAIR: bits::Pair64 = Self::BASE_LOW.pair_with(base::LOW);
    const BASE_MID_PAIR: bits::Pair64 = Self::BASE_MID.pair_with(base::MID);

    // hahaha lol no limits
    const DEFAULT_BITS: u64 = Self::LIMIT_LOW.set_all(Self::LIMIT_HIGH.set_all(0));

    /// Used by multiple `fmt::Debug` impls, `const`-ified to prevent typos.
    const NAME: &'static str = "segment::Descriptor";

    /// Returns a new segment descriptor for a 64-bit code segment.
    pub const fn code() -> Self {
        Self(Self::DEFAULT_BITS | DescriptorFlags::CODE.bits() | DescriptorFlags::LONG_MODE.bits())
    }

    /// Returns a new segment descriptor for a 32-bit code segment.
    pub const fn code_32() -> Self {
        Self(
            Self::DEFAULT_BITS
                | DescriptorFlags::CODE.bits()
                | DescriptorFlags::DEFAULT_SIZE.bits(),
        )
    }

    /// Returns a new segment descriptor for data segment.
    pub const fn data() -> Self {
        Self(Self::DEFAULT_BITS | DescriptorFlags::DATA.bits())
    }

    pub fn ring(&self) -> cpu::Ring {
        cpu::Ring::from_u8(self.ring_bits())
    }

    pub const fn limit(&self) -> u64 {
        Pack64::pack_in(0)
            .pack_from_dst(self.0, &Self::LIMIT_LOW_PAIR)
            .pack_from_dst(self.0, &Self::LIMIT_HIGH_PAIR)
            .bits()
    }

    pub const fn base(&self) -> u64 {
        Pack64::pack_in(0)
            .pack_from_dst(self.0, &Self::BASE_LOW_PAIR)
            .pack_from_dst(self.0, &Self::BASE_MID_PAIR)
            .bits()
    }

    /// Separated out from constructing the `cpu::Ring` for use in `const fn`s
    /// (since `Ring::from_u8` panics), but shouldn't be public because it
    /// performs no validation.
    const fn ring_bits(&self) -> u8 {
        Self::RING.unpack(self.0) as u8
    }

    pub const fn with_ring(self, ring: cpu::Ring) -> Self {
        Self(Self::RING.pack_truncating(ring as u8 as u64, self.0))
    }

    /// Returns a `Descriptor` with `flags` set.
    pub const fn with_flags(self, flags: &DescriptorFlags) -> Self {
        Self(self.0 | flags.bits())
    }

    pub const fn flags(&self) -> DescriptorFlags {
        DescriptorFlags::from_bits_truncate(self.0)
    }
}

impl fmt::Debug for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // pretty-print the flags and other data
        f.debug_struct(Self::NAME)
            .field("ring", &self.ring())
            .field("flags", &self.flags())
            .field("limit", &self.limit())
            .field("base", &format_args!("{:#x}", self.base()))
            .field("bits", &format_args!("{:#x}", self.0))
            .finish()
    }
}

impl fmt::UpperHex for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(Self::NAME)
            .field(&format_args!("{:#X}", self.0))
            .finish()
    }
}

impl fmt::LowerHex for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(Self::NAME)
            .field(&format_args!("{:#x}", self.0))
            .finish()
    }
}

impl fmt::Binary for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(Self::NAME)
            .field(&format_args!("{:#b}", self.0))
            .finish()
    }
}

bitflags::bitflags! {
    /// Flags for a GDT descriptor. Not all flags are valid for all descriptor types.
    pub struct DescriptorFlags: u64 {
        /// Set by the processor if this segment has been accessed. Only cleared by software.
        /// _Setting_ this bit in software prevents GDT writes on first use.
        const ACCESSED          = 1 << 40;
        /// For 32-bit data segments, sets the segment as writable. For 32-bit code segments,
        /// sets the segment as _readable_. In 64-bit mode, ignored for all segments.
        const WRITABLE          = 1 << 41;
        /// For code segments, sets the segment as “conforming”, influencing the
        /// privilege checks that occur on control transfers. For 32-bit data segments,
        /// sets the segment as "expand down". In 64-bit mode, ignored for data segments.
        const CONFORMING        = 1 << 42;
        /// This flag must be set for code segments and unset for data segments.
        const EXECUTABLE        = 1 << 43;
        /// This flag must be set for user segments (in contrast to system segments).
        const USER_SEGMENT      = 1 << 44;
        /// Must be set for any segment, causes a segment not present exception if not set.
        const PRESENT           = 1 << 47;
        /// Available for use by the Operating System
        const AVAILABLE         = 1 << 52;
        /// Must be set for 64-bit code segments, unset otherwise.
        const LONG_MODE         = 1 << 53;
        /// Use 32-bit (as opposed to 16-bit) operands. If [`LONG_MODE`][Self::LONG_MODE] is set,
        /// this must be unset. In 64-bit mode, ignored for data segments.
        const DEFAULT_SIZE      = 1 << 54;
        /// Limit field is scaled by 4096 bytes. In 64-bit mode, ignored for all segments.
        const GRANULARITY       = 1 << 55;
    }
}

impl DescriptorFlags {
    const BASE: Self = Self::from_bits_truncate(
        Self::USER_SEGMENT.bits()
            | Self::PRESENT.bits()
            | Self::WRITABLE.bits()
            | Self::ACCESSED.bits()
            | Self::GRANULARITY.bits(),
    );

    const CODE: Self = Self::from_bits_truncate(Self::BASE.bits() | Self::EXECUTABLE.bits());
    const DATA: Self = Self::from_bits_truncate(Self::BASE.bits() | Self::DEFAULT_SIZE.bits());
}

impl SystemDescriptor {
    const BASE_HIGH: bits::Pack64 = bits::Pack64::least_significant(32);
    const BASE_HIGH_PAIR: bits::Pair64 = Self::BASE_HIGH.pair_with(base::HIGH);

    pub fn tss(tss: &'static task::StateSegment) -> Self {
        let tss_addr = tss as *const _ as u64;
        tracing::trace!(tss_addr = fmt::hex(tss_addr), "making TSS descriptor...");

        // limit (-1 because the bound is inclusive)
        let limit = (mem::size_of::<task::StateSegment>() - 1) as u64;

        let low = Pack64::pack_in(DescriptorFlags::PRESENT.bits())
            .pack_from_src(limit, &Descriptor::LIMIT_LOW_PAIR)
            // base addr (low 24 bits)
            .pack_from_src(tss_addr, &Descriptor::BASE_LOW_PAIR)
            .pack_from_src(limit, &Descriptor::LIMIT_HIGH_PAIR)
            .pack_truncating(0b1001, &Descriptor::ACCESS_FLAGS)
            // base addr (mid 8 bits)
            .pack_from_src(tss_addr, &Descriptor::BASE_MID_PAIR)
            .bits();

        let high = Pack64::pack_in(0)
            // base addr (highest 32 bits)
            .pack_from_src(tss_addr, &Self::BASE_HIGH_PAIR)
            .bits();

        Self { high, low }
    }

    pub fn base(&self) -> u64 {
        Pack64::pack_in(0)
            .pack_from_dst(self.low, &Descriptor::BASE_LOW_PAIR)
            .pack_from_dst(self.low, &Descriptor::BASE_MID_PAIR)
            .pack_from_dst(self.high, &Self::BASE_HIGH_PAIR)
            .bits()
    }

    pub const fn limit(&self) -> u64 {
        Pack64::pack_in(0)
            .pack_from_dst(self.low, &Descriptor::LIMIT_LOW_PAIR)
            .pack_from_dst(self.low, &Descriptor::LIMIT_HIGH_PAIR)
            .bits()
    }
}

impl fmt::Debug for SystemDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("segment::SystemDescriptor")
            .field(
                "limit_low",
                &fmt::hex(Descriptor::LIMIT_LOW.unpack(self.low)),
            )
            .field("base_low", &fmt::hex(Descriptor::BASE_LOW.unpack(self.low)))
            .field("type", &fmt::bin(Descriptor::ACCESS_FLAGS.unpack(self.low)))
            .field("ring", &fmt::bin(Descriptor::RING.unpack(self.low)))
            .field(
                "present",
                &fmt::bin(Descriptor::PRESENT_BIT.unpack(self.low)),
            )
            .field(
                "limit_high",
                &fmt::bin(Descriptor::LIMIT_HIGH.unpack(self.low)),
            )
            .field("base_mid", &fmt::hex(Descriptor::BASE_MID.unpack(self.low)))
            .field("base_high", &fmt::hex(Self::BASE_HIGH.unpack(self.high)))
            .field("low_bits", &fmt::hex(self.low))
            .field("high_bits", &fmt::hex(self.high))
            .field("limit", &self.limit())
            .field("base", &fmt::hex(self.base()))
            .finish()
    }
}

mod limit {
    use mycelium_util::bits::Pack64;
    pub(super) const LOW: Pack64 = Pack64::least_significant(16);
    pub(super) const HIGH: Pack64 = LOW.next(4);
}

mod base {
    use mycelium_util::bits::Pack64;
    pub(super) const LOW: Pack64 = Pack64::least_significant(24);
    pub(super) const MID: Pack64 = LOW.next(8);
    pub(super) const HIGH: Pack64 = MID.next(32);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;
    use proptest::prelude::*;

    #[test]
    fn segment_selector_is_correct_size() {
        assert_eq!(size_of::<Selector>(), 2);
    }

    #[test]
    fn selector_pack_specs_valid() {
        Selector::RING.assert_valid();
        Selector::LDT_BIT.assert_valid();
        Selector::INDEX.assert_valid();
    }

    #[test]
    fn descriptor_pack_specs_valid() {
        Descriptor::ACCESS_FLAGS.assert_valid();
        Descriptor::PRESENT_BIT.assert_valid();
        Descriptor::FLAGS.assert_valid();
        Descriptor::RING.assert_valid();

        Descriptor::BASE_LOW_PAIR.assert_valid();
        Descriptor::BASE_MID_PAIR.assert_valid();
        SystemDescriptor::BASE_HIGH_PAIR.assert_valid();

        Descriptor::LIMIT_LOW_PAIR.assert_valid();
        Descriptor::LIMIT_HIGH_PAIR.assert_valid();
    }

    #[test]
    fn default_descriptor_flags_match_linux() {
        use cpu::Ring::*;
        // are our default flags reasonable? stolen from linux: arch/x86/kernel/cpu/common.c
        assert_eq!(
            dbg!(Descriptor::code().with_ring(Ring0)),
            Descriptor(0x00af9b000000ffff),
        );
        assert_eq!(
            dbg!(Descriptor::code_32().with_ring(Ring0)),
            Descriptor(0x00cf9b000000ffff)
        );
        assert_eq!(
            dbg!(Descriptor::data().with_ring(Ring0)),
            Descriptor(0x00cf93000000ffff)
        );
        assert_eq!(
            dbg!(Descriptor::code().with_ring(Ring3)),
            Descriptor(0x00affb000000ffff)
        );
        assert_eq!(
            dbg!(Descriptor::code_32().with_ring(Ring3)),
            Descriptor(0x00cffb000000ffff)
        );
        assert_eq!(
            dbg!(Descriptor::data().with_ring(Ring3)),
            Descriptor(0x00cff3000000ffff)
        );
    }

    #[test]
    fn debug_base_pack_pairs() {
        dbg!(Descriptor::BASE_LOW_PAIR);
        dbg!(Descriptor::BASE_MID_PAIR);
        dbg!(SystemDescriptor::BASE_HIGH_PAIR);
    }

    proptest! {
        #[test]
        fn system_segment_tss_base(addr: u64) {
            let tss_addr = unsafe {
                // safety: this address will never be dereferenced...
                &*(addr as *const task::StateSegment)
            };
            let tss_descr = SystemDescriptor::tss(tss_addr);
            let base = tss_descr.base();
            prop_assert_eq!(
                base, addr,
                "expected={:x}; actual={:x}\ndescr={:#?}",
                addr, base, tss_descr,
            );
        }

        #[test]
        fn system_segment_tss_limit(addr: u64) {
            let tss_addr = unsafe {
                // safety: this address will never be dereferenced...
                &*(addr as *const task::StateSegment)
            };
            let tss_descr = SystemDescriptor::tss(tss_addr);
            prop_assert_eq!(
                tss_descr.limit(),
                mem::size_of::<task::StateSegment>() as u64 - 1,
                "limit; descr={:#?}", tss_descr
            );
        }
    }
}
