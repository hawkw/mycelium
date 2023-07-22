use super::{PinPolarity, TriggerMode};
use crate::{
    cpu::FeatureNotSupported,
    mm::{self, page, size::Size4Kb, PhysPage, VirtPage},
};
use hal_core::PAddr;
use mycelium_util::bits::{bitfield, enum_from_bits};
use volatile::Volatile;

#[derive(Debug)]
#[must_use]
pub struct IoApic {
    registers: Volatile<&'static mut MmioRegisters>,
}

bitfield! {
    pub struct RedirectionEntry<u64> {
        pub const VECTOR: u8;
        pub const DELIVERY: DeliveryMode;
        /// Destination mode.
        ///
        /// Physical (0) or logical (1). If this is physical mode, then bits
        /// 56-59 should contain an APIC ID. If this is logical mode, then those
        /// bits contain a set of processors.
        pub const DEST_MODE: DestinationMode;
        /// Set if this interrupt is going to be sent, but the APIC is busy. Read only.
        pub const QUEUED: bool;
        pub const POLARITY: PinPolarity;
        /// Remote IRR.
        ///
        /// Used for level triggered interrupts only to show if a local APIC has
        /// received the interrupt (= 1), or has sent an EOI (= 0). Read only.
        pub const REMOTE_IRR: bool;
        pub const TRIGGER: TriggerMode;
        pub const MASKED: bool;
        const _RESERVED = 39;
        /// Destination field.
        ///
        /// If the destination mode bit was clear, then the
        /// lower 4 bits contain the bit APIC ID to sent the interrupt to. If
        /// the bit was set, the upper 4 bits also contain a set of processors.
        /// (See below)
        pub const DESTINATION: u8;
    }
}

enum_from_bits! {
    #[derive(Debug, PartialEq, Eq)]
    pub enum DestinationMode<u8> {
        Physical = 0,
        Logical = 1,
    }
}

enum_from_bits! {
    #[derive(Debug, PartialEq, Eq)]
    pub enum DeliveryMode<u8> {
        /// Normal interrupt delivery.
        Normal = 0b000,
        /// Lowest priority.
        LowPriority = 0b001,
        /// System Management Interrupt (SMI).
        SystemManagement = 0b010,
        /// Non-Maskable Interrupt (NMI).
        NonMaskable = 0b100,
        /// "INIT" (what does this mean? i don't know!)
        Init = 0b101,
        /// External interrupt.
        External = 0b111,
    }
}

/// Memory-mapped IOAPIC registers
#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct MmioRegisters {
    /// Selects the address to read/write from
    address: u32,
    _pad: [u32; 3],
    /// The data to read/write
    data: u32,
}

// === impl IoApic ===

impl IoApic {
    pub(crate) const PS2_KEYBOARD_IRQ: u8 = 0x1;
    pub(crate) const PIT_TIMER_IRQ: u8 = 0x2;
    const REDIRECTION_ENTRY_BASE: u32 = 0x10;

    /// Try to construct an `IoApic`.
    ///
    /// # Arguments
    ///
    /// - `base_addr`: The [`PAddr`] of the I/O APIC's memory-mapped register
    ///   page.
    /// - `pagectrl`: a [page mapper](page::Map) used to ensure that the MMIO
    ///   register page is mapped and writable.
    /// - `frame_alloc`: a [frame allocator](page::Alloc) used to allocate page
    ///   frame(s) while mapping the MMIO register page.
    ///
    /// # Returns
    /// - `Some(IoApic)` if this CPU supports the APIC interrupt model.
    /// - `None` if this CPU does not support APIC interrupt handling.
    pub fn try_new<A>(
        base_paddr: PAddr,
        pagectrl: &mut impl page::Map<Size4Kb, A>,
        frame_alloc: &A,
    ) -> Result<Self, FeatureNotSupported>
    where
        A: page::Alloc<Size4Kb>,
    {
        if !super::is_supported() {
            tracing::warn!("tried to construct an IO APIC, but the CPU does not support the APIC interrupt model");
            return Err(FeatureNotSupported::new("APIC interrupt model"));
        }

        let base = mm::kernel_vaddr_of(base_paddr);
        tracing::debug!(?base, ?base_paddr, "found I/O APIC base address");

        unsafe {
            // ensure the I/O APIC's MMIO page is mapped and writable.
            let virt = VirtPage::<Size4Kb>::containing_fixed(base);
            let phys = PhysPage::<Size4Kb>::containing_fixed(base_paddr);
            tracing::debug!(?virt, ?phys, "mapping I/O APIC MMIO page...");
            pagectrl
                .map_page(virt, phys, frame_alloc)
                .set_writable(true)
                .commit();
            tracing::debug!("mapped I/O APIC MMIO page!");
        }

        let registers = unsafe { Volatile::new(&mut *base.as_ptr::<MmioRegisters>()) };
        let mut ioapic = Self { registers };
        tracing::info!(
            addr = ?base,
            id = ioapic.id(),
            version = ioapic.version(),
            max_entries = ioapic.max_entries(),
            "I/O APIC enabled"
        );
        Ok(ioapic)
    }

    #[inline]
    pub fn new<A>(addr: PAddr, pagectrl: &mut impl page::Map<Size4Kb, A>, frame_alloc: &A) -> Self
    where
        A: page::Alloc<Size4Kb>,
    {
        Self::try_new(addr, pagectrl, frame_alloc).unwrap()
    }

    /// Map all ISA interrupts starting at `base`.
    #[tracing::instrument(level = tracing::Level::DEBUG, skip(self))]
    pub fn map_isa_irqs(&mut self, base: u8) {
        let flags = RedirectionEntry::new()
            .with(RedirectionEntry::DELIVERY, DeliveryMode::Normal)
            .with(RedirectionEntry::POLARITY, PinPolarity::High)
            .with(RedirectionEntry::REMOTE_IRR, false)
            .with(RedirectionEntry::TRIGGER, TriggerMode::Edge)
            .with(RedirectionEntry::MASKED, true)
            .with(RedirectionEntry::DESTINATION, 0xff);
        for irq in 0..16 {
            let entry = flags.with(RedirectionEntry::VECTOR, base + irq);
            self.set_entry(irq, entry);
        }
    }

    /// Returns the IO APIC's ID.
    #[must_use]
    pub fn id(&mut self) -> u8 {
        let val = self.read(0);
        (val >> 24) as u8
    }

    /// Returns the IO APIC's version.
    #[must_use]
    pub fn version(&mut self) -> u8 {
        self.read(0x1) as u8
    }

    /// Returns the maximum number of redirection entries.
    #[must_use]
    pub fn max_entries(&mut self) -> u8 {
        (self.read(0x1) >> 16) as u8
    }

    #[must_use]
    pub fn entry(&mut self, irq: u8) -> RedirectionEntry {
        let register_low = self
            .entry_offset(irq)
            .expect("IRQ number exceeds max redirection entries");
        self.entry_raw(register_low)
    }

    pub fn set_entry(&mut self, irq: u8, entry: RedirectionEntry) {
        tracing::debug!(irq, ?entry, "setting IOAPIC redirection entry");
        let register_low = self
            .entry_offset(irq)
            .expect("IRQ number exceeds max redirection entries");
        let bits = entry.bits();
        let low = bits as u32;
        let high = (bits >> 32) as u32;
        self.write(register_low, low);
        self.write(register_low + 1, high);
    }

    /// Convenience function to mask/unmask an IRQ.
    pub fn set_masked(&mut self, irq: u8, masked: bool) {
        tracing::debug!(irq, masked, "IoApic::set_masked");
        self.update_entry(irq, |entry| entry.with(RedirectionEntry::MASKED, masked))
    }

    pub fn update_entry(
        &mut self,
        irq: u8,
        update: impl FnOnce(RedirectionEntry) -> RedirectionEntry,
    ) {
        let register_low = self
            .entry_offset(irq)
            .expect("IRQ number exceeds max redirection entries");
        let entry = self.entry_raw(register_low);
        let new_entry = update(entry);
        self.set_entry_raw(register_low, new_entry);
    }

    fn entry_offset(&mut self, irq: u8) -> Option<u32> {
        let max_entries = self.max_entries();
        if irq > max_entries {
            tracing::warn!("tried to access redirection entry {irq}, but the IO APIC only supports supports up to {max_entries}");
            return None;
        }

        Some(Self::REDIRECTION_ENTRY_BASE + irq as u32 * 2)
    }

    #[inline]
    fn entry_raw(&mut self, register_low: u32) -> RedirectionEntry {
        let low = self.read(register_low);
        let high = self.read(register_low + 1);
        RedirectionEntry::from_bits((high as u64) << 32 | low as u64)
    }

    #[inline]
    fn set_entry_raw(&mut self, register_low: u32, entry: RedirectionEntry) {
        let bits = entry.bits();
        let low = bits as u32;
        let high = (bits >> 32) as u32;
        self.write(register_low, low);
        self.write(register_low + 1, high);
    }

    #[must_use]

    fn read(&mut self, offset: u32) -> u32 {
        self.set_offset(offset);
        self.registers.map_mut(|ioapic| &mut ioapic.data).read()
    }

    fn write(&mut self, offset: u32, value: u32) {
        self.set_offset(offset);
        self.registers
            .map_mut(|ioapic| &mut ioapic.data)
            .write(value)
    }

    fn set_offset(&mut self, offset: u32) {
        assert!(offset <= 0xff, "invalid IOAPIC register offset {offset:#x}",);
        self.registers
            .map_mut(|ioapic| &mut ioapic.address)
            .write(offset);
    }
}

// === impl DeliveryMode ===

impl Default for DeliveryMode {
    fn default() -> Self {
        Self::Normal
    }
}

mod test {
    use super::*;

    #[test]
    fn redirection_entry_is_valid() {
        RedirectionEntry::assert_valid();

        let entry = RedirectionEntry::new()
            .with(RedirectionEntry::DELIVERY, DeliveryMode::Normal)
            .with(RedirectionEntry::POLARITY, PinPolarity::High)
            .with(RedirectionEntry::TRIGGER, TriggerMode::Edge)
            .with(RedirectionEntry::MASKED, true)
            .with(RedirectionEntry::DESTINATION, 0xff)
            .with(RedirectionEntry::VECTOR, 0x30);
        println!("{entry}");
    }

    #[test]
    fn redirection_entry_offsets() {
        assert_eq!(
            RedirectionEntry::DELIVERY.least_significant_index(),
            8,
            "delivery"
        );
        assert_eq!(
            RedirectionEntry::DEST_MODE.least_significant_index(),
            11,
            "destination mode"
        );
        assert_eq!(
            RedirectionEntry::QUEUED.least_significant_index(),
            12,
            "queued"
        );
        assert_eq!(
            RedirectionEntry::POLARITY.least_significant_index(),
            13,
            "pin polarity"
        );
        assert_eq!(
            RedirectionEntry::REMOTE_IRR.least_significant_index(),
            14,
            "remote IRR"
        );
        assert_eq!(
            RedirectionEntry::TRIGGER.least_significant_index(),
            15,
            "trigger mode"
        );
        assert_eq!(
            RedirectionEntry::MASKED.least_significant_index(),
            16,
            "masked"
        );
        assert_eq!(
            RedirectionEntry::DESTINATION.least_significant_index(),
            56,
            "destination field"
        );
    }

    #[test]
    fn offsetof() {
        let mmregs = MmioRegisters {
            address: 0,
            _pad: [0, 0, 0],
            data: 0,
        };
        let addrof = core::ptr::addr_of!(mmregs.data);
        assert_eq!(
            addrof as *const () as usize,
            (&mmregs as *const _ as usize) + 0x10
        )
    }
}
