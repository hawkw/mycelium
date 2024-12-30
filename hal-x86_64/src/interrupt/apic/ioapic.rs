use super::{PinPolarity, TriggerMode};
use crate::{
    cpu::FeatureNotSupported,
    interrupt::IsaInterrupt,
    mm::{self, page, size::Size4Kb, PhysPage, VirtPage},
};
use hal_core::PAddr;
use mycelium_util::{
    bits::{bitfield, enum_from_bits},
    sync::blocking::Mutex,
};
use volatile::Volatile;

#[derive(Debug)]
pub struct IoApicSet {
    ioapics: alloc::vec::Vec<Mutex<IoApic>>,
    isa_map: [IsaOverride; 16],
}

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

#[derive(Copy, Clone, Debug)]
struct IsaOverride {
    apic: u8,
    vec: u8,
}

// === impl IoApicSet ===

impl IoApicSet {
    pub fn new(
        madt: &acpi::platform::interrupt::Apic,
        frame_alloc: &impl hal_core::mem::page::Alloc<mm::size::Size4Kb>,
        pagectrl: &mut crate::mm::PageCtrl,
        isa_base: u8,
    ) -> Self {
        // The ACPI Multiple APIC Descriptor Table (MADT) tells us where to find
        // the I/O APICs, as well as information about how the ISA standard
        // interrupts are routed to I/O APIC inputs on this system.
        //
        // See: https://wiki.osdev.org/MADT

        // There may be multiple I/O APICs in the system, so we'll collect them
        // all into a `Vec`. We're also going to build a table of how the ISA
        // standard interrupts are mapped across those I/O APICs, so we can look
        // up which one a particular interrupt lives on.
        let n_ioapics = madt.io_apics.len();
        tracing::trace!(?madt.io_apics, "found {n_ioapics} IO APICs", );
        assert_ne!(
            n_ioapics, 0,
            "why would a computer have the APIC interrupt model but not \
             have any IO APICs???"
        );
        let mut this = IoApicSet {
            ioapics: alloc::vec::Vec::with_capacity(n_ioapics),
            isa_map: [IsaOverride { apic: 0, vec: 0 }; 16],
        };

        for (n, ioapic) in madt.io_apics.iter().enumerate() {
            let addr = PAddr::from_u64(ioapic.address as u64);
            tracing::debug!(ioapic.paddr = ?addr, "IOAPIC {n}");
            this.ioapics
                .push(Mutex::new(IoApic::new(addr, pagectrl, frame_alloc)));
        }

        // Okay, so here's where it gets ~*weird*~.
        //
        // On the Platonic Ideal Normal Computer, the ISA PC interrupts would be
        // mapped to I/O APIC input pins by number, so ISA IRQ 0 would go to pin
        // 0 on I/O APIC 0, and so on.
        //
        // However, motherboard manufacturers can do whatever they like when
        // routing these interrupts, so they could go to different pins,
        // potentially on different I/O APICs (if the system has more than one).
        // Also, some of these interrupts might have different pin polarity or
        // edge/level-triggered-iness than what we might expect.
        //
        // Fortunately, ACPI is here to help (statements dreamed up by the
        // utterly deranged).
        //
        // The MADT includes a list of "interrupt source overrides" that
        // describe any ISA interrupts that are not mapped to I/O APIC pins in
        // numeric order. Each entry in the interrupt source overrides list will
        // contain:
        // - an ISA IRQ number (naturally),
        // - the "global system interrupt", which is NOT the IDT vector for that
        //   interrupt but instead the I/O APIC input pin number that the IRQ is
        //   routed to,
        // - the polarity and trigger mode of the interrupt pin, if these are
        //   different from the default bus trigger mode.
        //
        // For each of the 16 ISA interrupt vectors, we'll configure the
        // redirection entry in the appropriate I/O APIC and record which I/O
        // APIC (and which pin on that I/O APIC) the interrupt is routed to. We
        // do this by first checking if the MADT contains an override matching
        // that ISA interrupt, using that if so, and if not, falling back to the
        // ISA interrupt number.
        //
        // Yes, this is a big pile of nested loops. But, consider the following:
        //
        // - the MADT override entries can probably come in any order, if the
        //   motherboard firmware chooses to be maximally perverse, so we have
        //   to scan the whole list of them to find out if each ISA interrupt is
        //   overridden.
        // - there are only ever 16 ISA interrupts, so the outer loop iterates
        //   exactly 16 times; and there can't be *more* overrides than there
        //   are ISA interrupts (and there's generally substantially fewer).
        //   similarly, if there's more than 1-8 I/O APICs, you probably have
        //   some kind of really weird computer and should tell me about it
        //   because i bet it's awesome.
        //   so, neither inner loop actually loops that many times.
        // - finally, we only do this once on boot, so who cares?

        let base_entry = RedirectionEntry::new()
            .with(RedirectionEntry::DELIVERY, DeliveryMode::Normal)
            .with(RedirectionEntry::REMOTE_IRR, false)
            .with(RedirectionEntry::MASKED, true)
            .with(RedirectionEntry::DESTINATION, 0xff);
        for irq in IsaInterrupt::ALL {
            // Assume the IRQ is mapped to the I/O APIC pin corresponding to
            // that ISA IRQ number, and is active-high and edge-triggered.
            let mut global_system_interrupt = irq as u8;
            let mut polarity = PinPolarity::High;
            let mut trigger = TriggerMode::Edge;
            // Is there an override for this IRQ? If there is, clobber the
            // assumed defaults with "whatever the override says".
            if let Some(src_override) = madt
                .interrupt_source_overrides
                .iter()
                .find(|o| o.isa_source == irq as u8)
            {
                // put the defaults through an extruder that maybe messes with
                // them.
                use acpi::platform::interrupt::{
                    Polarity as AcpiPolarity, TriggerMode as AcpiTriggerMode,
                };
                tracing::debug!(
                    ?irq,
                    ?src_override.global_system_interrupt,
                    ?src_override.polarity,
                    ?src_override.trigger_mode,
                    "ISA interrupt {irq:?} is overridden by MADT"
                );
                match src_override.polarity {
                    AcpiPolarity::ActiveHigh => polarity = PinPolarity::High,
                    AcpiPolarity::ActiveLow => polarity = PinPolarity::Low,
                    // TODO(eliza): if the MADT override entry says that the pin
                    // polarity is "same as bus", we should probably actually
                    // make it be the same as the bus, instead of just assuming
                    // that it's active high. But...just assuming that "same as
                    // bus" means active high seems to basically work so far...
                    AcpiPolarity::SameAsBus => {}
                }
                match src_override.trigger_mode {
                    AcpiTriggerMode::Edge => trigger = TriggerMode::Edge,
                    AcpiTriggerMode::Level => trigger = TriggerMode::Level,
                    // TODO(eliza): As above, when the MADT says this is "same
                    // as bus", we should make it be the same as the bus instead
                    // of going "ITS EDGE TRIGGERED LOL LMAO" which is what
                    // we're currently doing. But, just like above, this Seems
                    // To Work?
                    AcpiTriggerMode::SameAsBus => {}
                }
                global_system_interrupt = src_override.global_system_interrupt.try_into().expect(
                    "if this exceeds u8::MAX, what the fuck! \
                        that's bigger than the entire IDT...",
                );
            }
            // Now, scan to find which I/O APIC this IRQ corresponds to. if the
            // system only has one I/O APIC, this will always be 0, but we gotta
            // handle systems with more than one. So, we'll this by traversing
            // the list of I/O APICs, seeing if the GSI number is less than the
            // max number of IRQs handled by that I/O APIC, and if it is, we'll
            // stick it in there. If not, keep searching for the next one,
            // subtracting the max number of interrupts handled by the I/O APIC
            // we just looked at.
            let mut entry_idx = global_system_interrupt;
            let mut got_him = false;
            'apic_scan: for (apic_idx, apic) in this.ioapics.iter_mut().enumerate() {
                let apic = apic.get_mut();
                let max_entries = apic.max_entries();
                if entry_idx > max_entries {
                    entry_idx -= max_entries;
                    continue;
                }

                // Ladies and gentlemen...we got him!
                got_him = true;
                tracing::debug!(
                    ?irq,
                    ?global_system_interrupt,
                    ?apic_idx,
                    ?entry_idx,
                    "found IOAPIC for ISA interrupt"
                );
                let entry = base_entry
                    .with(RedirectionEntry::POLARITY, polarity)
                    .with(RedirectionEntry::TRIGGER, trigger)
                    .with(RedirectionEntry::VECTOR, isa_base + irq as u8);
                apic.set_entry(entry_idx, entry);
                this.isa_map[irq as usize] = IsaOverride {
                    apic: apic_idx as u8,
                    vec: entry_idx,
                };
                break 'apic_scan;
            }

            assert!(
                got_him,
                "somehow, we didn't find an I/O APIC for MADT global system \
                 interrupt {global_system_interrupt} (ISA IRQ {irq:?})!\n \
                 this probably means the MADT is corrupted somehow, or maybe \
                 your motherboard is just super weird? i have no idea what to \
                 do in this situation, so i guess i'll die."
            );
        }

        this
    }

    fn for_isa_irq(&self, irq: IsaInterrupt) -> (&Mutex<IoApic>, u8) {
        let isa_override = self.isa_map[irq as usize];
        (&self.ioapics[isa_override.apic as usize], isa_override.vec)
    }

    pub fn set_isa_masked(&self, irq: IsaInterrupt, masked: bool) {
        let (ioapic, vec) = self.for_isa_irq(irq);
        ioapic.with_lock(|ioapic| ioapic.set_masked(vec, masked));
    }
}

// === impl IoApic ===

impl IoApic {
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

#[cfg(test)]
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
