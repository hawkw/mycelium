/// x86 memory segmentation structures.
use crate::{cpu, task};
use core::{arch::asm, mem};
use mycelium_util::{
    bits::{self, Pack64, Packing64, Pair64},
    fmt,
};

bits::bitfield! {
    /// A segment selector.
    ///
    /// These values are stored in a segmentation register (`ss`, `cs`, `ds`, `es`,
    /// `gs`, or `fs`) to select the current segment in that register. A selector
    /// consists of two bits indicating the privilege ring of that segment, a bit
    /// indicating whether it selects a [GDT] or LDT, and a 5-bit index into [GDT]
    /// or LDT indicating which segment is selected.
    ///
    /// Refer to sections 3.4.3 and 3.4.4 in [Vol. 3A of the _Intel® 64 and IA-32
    /// Architectures Developer's Manual_][manual] for details.
    ///
    /// [GDT]: Gdt
    /// [manual]: https://www.intel.com/content/www/us/en/architecture-and-technology/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.html
    #[derive(Eq, PartialEq)]
    pub struct Selector<u16> {
        /// The first 2 least-significant bits are the selector's priveliege ring.
        const RING: cpu::Ring;
        /// The next bit is set if this is an LDT segment selector.
        const IS_LDT: bool;
        /// The remaining bits are the index in the GDT/LDT.
        const INDEX = 5;
    }
}

bits::bitfield! {
    /// A 64-bit mode user segment descriptor.
    ///
    /// A segment descriptor is an entry in a [GDT] or LDT that provides the
    /// processor with the size and location of a segment, as well as access control
    /// and status information.
    ///
    /// Refer to section 3.4.5 in [Vol. 3A of the _Intel® 64 and IA-32 Architectures
    /// Developer's Manual_][manual] for details.
    ///
    /// [GDT]: Gdt
    /// [manual]: https://www.intel.com/content/www/us/en/architecture-and-technology/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.html
    #[derive(Eq, PartialEq)]
    pub struct Descriptor<u64> {
        /// First 16 bits of the limit field (ignored in 64-bit mode)
        const LIMIT_LOW = 16;
        /// First 24 bits of the base field (ignored in 64-bit mode)
        const BASE_LOW = 24;

        // Access flags (5 bits).
        // In order, least to most significant:
        // - `ACCESSED`
        // - `READABLE`/`WRITABLE`
        // - `DIRECTION`/`CONFORMING`
        // - `EXECUTABLE` (code/data)
        // - `TYPE` (user/system)

        /// Set by the processor if this segment has been accessed. Only cleared by software.
        /// _Setting_ this bit in software prevents GDT writes on first use.
        const ACCESSED: bool;

        /// Readable bit for code segments/writable bit for data segments.
        const READABLE: bool;
        /// Direction bit for code segments/conforming bit for data segments.
        const CONFORMING: bool;
        /// Executable bit (if 1, this is a code segment)
        const IS_CODE_SEGMENT: bool;
        /// Descriptor type bit (if 1, this is a user segment)
        const IS_USER_SEGMENT: bool;
        /// Priveliege ring.
        const RING: cpu::Ring;
        /// Present bit
        const IS_PRESENT: bool;
        /// High 4 bits of the limit (ignored in 64-bit mode)
        const LIMIT_HIGH = 4;

        // High flags (5 bits).
        // In order, least to most significant:
        // - `AVAILABLE`
        // - `LONG_MODE`
        // - `DEFAULT_SIZE`
        // - `GRANULARITY`

        /// Available for use by the Operating System
        const AVAILABLE: bool;

        /// Must be set for 64-bit code segments, unset otherwise.
        const IS_LONG_MODE: bool;

        /// Use 32-bit (as opposed to 16-bit) operands. If [`LONG_MODE`][Self::LONG_MODE] is set,
        /// this must be unset. In 64-bit mode, ignored for data segments.
        const IS_32_BIT: bool;

        /// Limit field is scaled by 4096 bytes. In 64-bit mode, ignored for all segments.
        const GRANULARITY: bool;

        /// Highest 8 bits of the base field
        const BASE_MID = 8;
    }
}

/// A [Global Descriptor Table (GDT)][gdt].
///
/// This can have up to 65535 entries, but in 64-bit mode, you don't need most
/// of those (since you can't do real segmentation), so it defaults to 8.
///
/// Refer to section 3.5.1 in [Vol. 3A of the _Intel® 64 and IA-32 Architectures
/// Developer's Manual_][manual] for details.
///
/// [gdt]: https://wiki.osdev.org/Global_Descriptor_Table
/// [manual]: https://www.intel.com/content/www/us/en/architecture-and-technology/64-ia-32-architectures-software-developer-vol-3a-part-1-manual.html
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

// === impl Segment ===

impl Selector {
    pub const fn null() -> Self {
        Self(0)
    }

    pub const fn from_index(u: u16) -> Self {
        Self(Self::INDEX.pack_truncating(0, u))
    }

    pub const fn from_raw(u: u16) -> Self {
        Self(u)
    }

    pub fn ring(self) -> cpu::Ring {
        self.get(Self::RING)
    }

    /// Returns which descriptor table (GDT or LDT) this selector references.
    ///
    /// # Note
    ///
    /// This will never return [`cpu::DescriptorTable::Idt`], as a segment
    /// selector only references segmentation table descriptors.
    pub fn table(&self) -> cpu::DescriptorTable {
        if self.is_gdt() {
            cpu::DescriptorTable::Gdt
        } else {
            cpu::DescriptorTable::Idt
        }
    }

    /// Returns true if this is an LDT segment selector.
    pub fn is_ldt(&self) -> bool {
        self.get(Self::IS_LDT)
    }

    /// Returns true if this is a GDT segment selector.
    #[inline]
    pub fn is_gdt(&self) -> bool {
        !self.is_ldt()
    }

    /// Returns the index into the LDT or GDT this selector refers to.
    pub const fn index(&self) -> u16 {
        Self::INDEX.unpack_bits(self.0)
    }

    pub fn set_gdt(&mut self) -> &mut Self {
        self.set(Self::IS_LDT, false)
    }

    pub fn set_ldt(&mut self) -> &mut Self {
        self.set(Self::IS_LDT, true)
    }

    pub fn set_ring(&mut self, ring: cpu::Ring) -> &mut Self {
        tracing::trace!("before set_ring: {:b}", self.0);
        self.set(Self::RING, ring);
        tracing::trace!("after set_ring: {:b}", self.0);
        self
    }

    pub fn set_index(&mut self, index: u16) -> &mut Self {
        self.set(Self::INDEX, index)
    }

    /// Returns the current selector in the `cs` (code segment) register
    pub fn current_cs() -> Self {
        let sel: u16;
        unsafe {
            asm!("mov {0:x}, cs", out(reg) sel, options(nomem, nostack, preserves_flags));
        }
        Self(sel)
    }

    /// Sets `self` as the current code segment selector in the `cs` register.
    ///
    /// # Notes
    ///
    /// In 64-bit long mode, the code segment selector *must* have a base
    /// address of 0 and limit 2^64.
    ///
    /// # Safety
    ///
    /// lol
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

    /// Sets `self` as the current stack segment selector in the `ss` register.
    ///
    /// # Notes
    ///
    /// In 64-bit long mode, the stack segment selector *must* have a base
    /// address of 0 and limit 2^64.
    ///
    /// # Safety
    ///
    /// lol
    pub unsafe fn set_ss(self) {
        asm!("mov ss, {:x}", in(reg) self.0, options(nostack, preserves_flags));
        tracing::trace!(selector = fmt::alt(self), "set stack segment");
    }

    /// Sets `self` as the current data segment selector in the `ds` register.
    ///
    /// # Notes
    ///
    /// In 64-bit long mode, the data segment selector *must* have a base
    /// address of 0 and limit 2^64.
    ///
    /// # Safety
    ///
    /// lol
    pub unsafe fn set_ds(self) {
        asm!("mov ds, {:x}", in(reg) self.0, options(nostack, preserves_flags));
        tracing::trace!(selector = fmt::alt(self), "set data segment");
    }

    /// Sets `self` as the current segment in the general-purpose data segment
    /// register `es` ("Extra Segment").
    ///
    /// # Notes
    ///
    /// In 64-bit long mode, the extra segment selector *must* have a base
    /// address of 0 and limit 2^64.
    ///
    /// # Safety
    ///
    /// lol
    pub unsafe fn set_es(self) {
        asm!("mov es, {:x}", in(reg) self.0, options(nostack, preserves_flags));
        tracing::trace!(selector = fmt::alt(self), "set extra segment");
    }

    /// Sets `self` as the current segment in the general-purpose data segment
    /// register `fs` ("File Segment").
    ///
    /// # Notes
    ///
    /// Unlike the `cs`, `ss`, `ds`, and `es` registers, the `fs` register need
    /// not be zeroed in long mode, and can be used by the operating system. In
    /// particular, the `gs` and `fs` registers may be useful for storing
    /// thread- or CPU-local data.
    ///
    /// # Safety
    ///
    /// lol
    pub unsafe fn set_fs(self) {
        asm!("mov fs, {:x}", in(reg) self.0, options(nostack, preserves_flags));
        tracing::trace!(selector = fmt::alt(self), "set fs");
    }
    /// Sets `self` as the current segment in the general-purpose data segment
    /// register `gs` ("G Segment").
    ///
    /// # Notes
    ///
    /// Unlike the `cs`, `ss`, `ds`, and `es` registers, the `gs` register need
    /// not be zeroed in long mode, and can be used by the operating system. In
    /// particular, the `gs` and `fs` registers may be useful for storing
    /// thread- or CPU-local data.
    ///
    /// # Safety
    ///
    /// lol
    pub unsafe fn set_gs(self) {
        asm!("mov gs, {:x}", in(reg) self.0, options(nostack, preserves_flags));
        tracing::trace!(selector = fmt::alt(self), "set gs");
    }
}

// === impl Gdt ===

impl<const SIZE: usize> Gdt<SIZE> {
    /// Sets `self` as the current GDT.
    ///
    /// This method is safe, because the `'static` bound on `self` ensures that
    /// the pointed GDT doesn't go away while it's active. Therefore, this
    /// method is a safe wrapper around the `lgdt` CPU instruction
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

    /// Returns a new `Gdt` with all entries zeroed.
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
        let selector = Selector::null()
            .with(Selector::INDEX, idx)
            .with(Selector::RING, cpu::Ring::Ring0);
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

// === impl Descriptor ===

impl Descriptor {
    const LIMIT_LOW_PAIR: Pair64 = Self::LIMIT_LOW.pair_with(limit::LOW);
    const LIMIT_HIGH_PAIR: Pair64 = Self::LIMIT_HIGH.pair_with(limit::HIGH);

    const BASE_LOW_PAIR: Pair64 = Self::BASE_LOW.pair_with(base::LOW);
    const BASE_MID_PAIR: Pair64 = Self::BASE_MID.pair_with(base::MID);

    // Access flags (5 bits).
    // In order, least to most significant:
    // - `ACCESSED`
    // - `READABLE`/`WRITABLE`
    // - `DIRECTION`/`CONFORMING`
    // - `EXECUTABLE` (code/data)
    // - `TYPE` (user/system)
    // TODO(eliza): add a nicer `mycelium-bitfield` API for combining pack specs...
    const ACCESS_FLAGS: Pack64 = Self::BASE_LOW.typed::<u64, ()>().next(5);

    // hahaha lol no limits
    const DEFAULT_BITS: u64 = Packing64::new(0)
        .set_all(&Self::LIMIT_LOW)
        .set_all(&Self::LIMIT_HIGH)
        .bits();

    const USER_FLAGS: u64 = Packing64::new(0)
        .set_all(&Self::IS_USER_SEGMENT)
        .set_all(&Self::IS_PRESENT)
        .set_all(&Self::READABLE)
        .set_all(&Self::ACCESSED)
        .set_all(&Self::GRANULARITY)
        .bits();

    const CODE_FLAGS: u64 = Packing64::new(Self::USER_FLAGS)
        .set_all(&Self::IS_CODE_SEGMENT)
        .bits();
    const DATA_FLAGS: u64 = Packing64::new(Self::USER_FLAGS)
        .set_all(&Self::IS_32_BIT)
        .bits();

    /// Returns a new segment descriptor for a 64-bit code segment.
    pub const fn code() -> Self {
        Self(Self::DEFAULT_BITS | Self::CODE_FLAGS | Self::IS_LONG_MODE.raw_mask())
    }

    /// Returns a new segment descriptor for a 32-bit code segment.
    pub const fn code_32() -> Self {
        Self(Self::DEFAULT_BITS | Self::CODE_FLAGS | Self::IS_32_BIT.raw_mask())
    }

    /// Returns a new segment descriptor for data segment.
    pub const fn data() -> Self {
        Self(Self::DEFAULT_BITS | Self::DATA_FLAGS)
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
        Self::RING.unpack_bits(self.0) as u8
    }

    pub const fn with_ring(self, ring: cpu::Ring) -> Self {
        Self(Self::RING.pack_truncating(ring as u8 as u64, self.0))
    }
}

impl SystemDescriptor {
    const BASE_HIGH: bits::Pack64 = bits::Pack64::least_significant(32);
    const BASE_HIGH_PAIR: Pair64 = Self::BASE_HIGH.pair_with(base::HIGH);

    pub fn tss(tss: &'static task::StateSegment) -> Self {
        let tss_addr = tss as *const _ as u64;
        tracing::trace!(tss_addr = fmt::hex(tss_addr), "making TSS descriptor...");

        // limit (-1 because the bound is inclusive)
        let limit = (mem::size_of::<task::StateSegment>() - 1) as u64;

        let low = Pack64::pack_in(0)
            .pack(true, &Descriptor::IS_PRESENT)
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
            .field("ring", &Descriptor::RING.unpack(self.low))
            .field("present", &Descriptor::IS_PRESENT.unpack(self.low))
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
    fn prettyprint() {
        let selector = Selector::null()
            .with(Selector::INDEX, 30)
            .with(Selector::RING, cpu::Ring::Ring0);
        println!("{selector}");
    }

    #[test]
    fn segment_selector_is_correct_size() {
        assert_eq!(size_of::<Selector>(), 2);
    }

    #[test]
    fn selector_pack_specs_valid() {
        Selector::assert_valid()
    }

    #[test]
    fn descriptor_pack_specs_valid() {
        Descriptor::assert_valid();
        assert_eq!(Descriptor::IS_PRESENT.raw_mask(), 1 << 47);
    }

    #[test]
    fn descriptor_pack_pairs_valid() {
        Descriptor::LIMIT_LOW_PAIR.assert_valid();
        Descriptor::LIMIT_HIGH_PAIR.assert_valid();
        Descriptor::BASE_LOW_PAIR.assert_valid();
        Descriptor::BASE_MID_PAIR.assert_valid();
    }

    #[test]
    fn sys_descriptor_pack_pairs_valid() {
        SystemDescriptor::BASE_HIGH_PAIR.assert_valid()
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
                "\n  left: {:#064b}\n right: {:#064b}\n descr: {:#?}\n  addr: {:#x}\n",
                addr, base, tss_descr, addr
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
