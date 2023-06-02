use super::{PinPolarity, TriggerMode};
use crate::{
    cpu::{FeatureNotSupported, Msr},
    mm::{self, page, size::Size4Kb, PhysPage, VirtPage},
    time::{Duration, InvalidDuration},
};
use core::{convert::TryInto, marker::PhantomData, num::NonZeroU32};
use hal_core::{PAddr, VAddr};
use mycelium_util::fmt;
use raw_cpuid::CpuId;
use volatile::{access, Volatile};

#[derive(Debug)]
pub struct LocalApic {
    msr: Msr,
    base: VAddr,
}

/// Represents a register in the local APIC's configuration area.
#[derive(Debug)]
pub struct LocalApicRegister<T = u32, A = access::ReadWrite> {
    offset: usize,
    name: &'static str,
    _ty: PhantomData<fn(T, A)>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum IpiTarget {
    /// The interrupt is sent to the processor with the provided local APIC ID.
    ///
    /// This must be a valid local APIC ID for the current system, and may
    /// not exceed 4 bits in length.
    ApicId(u8),
    /// The interrupt is sent to this processor.
    Current,
    /// The interrupt is sent to all processors, *including* the current one.
    All,
    /// The interrupt is sent to all processors *except* the current one.
    Others,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum IpiKind {
    /// A Startup IPI (SIPI) is used to start an AP processor.
    Startup(mm::PhysPage<mm::size::Size4Kb>),

    /// INIT assert or de-assert.
    Init {
        /// Whether the INIT line should be asserted or deasserted.
        assert: bool,
    },

    /// An IPI is used to send an interrupt to another processor.
    Interrupt {
        /// The interrupt vector to trigger.
        vector: u8,
        // TODO(eliza): rest of this.
    },
}
pub trait RegisterAccess {
    type Access;
    type Target;
    fn volatile(
        ptr: &'static mut Self::Target,
    ) -> Volatile<&'static mut Self::Target, Self::Access>;
}

impl LocalApic {
    const BASE_PADDR_MASK: u64 = 0xffff_ffff_f000;

    // divisor for the APIC timer.
    //
    // it would be nicer if we could set this to 1, but apparently some
    // platforms "don't like" that...
    const TIMER_DIVISOR: u32 = 16;

    /// Try to construct a `LocalApic`.
    ///
    /// # Arguments
    ///
    /// - `pagectrl`: a [page mapper](page::Map) used to ensure that the local
    ///   APIC's memory-mapped register page is mapped and writable.
    /// - `frame_alloc`: a [frame allocator](page::Alloc) used to allocate page
    ///   frame(s) while mapping the MMIO register page.
    ///
    /// # Returns
    ///
    /// - [`Ok`]`(LocalApic)` if this CPU supports the APIC interrupt model.
    /// - [`Err`]`(`[`FeatureNotSupported`]`)` if this CPU does not support APIC
    ///   interrupt handling.
    pub fn try_new<A>(
        pagectrl: &mut impl page::Map<Size4Kb, A>,
        frame_alloc: &A,
    ) -> Result<Self, FeatureNotSupported>
    where
        A: page::Alloc<Size4Kb>,
    {
        if !super::is_supported() {
            return Err(FeatureNotSupported::new("APIC interrupt model"));
        }

        let msr = Msr::ia32_apic_base();
        let base_paddr = PAddr::from_u64(msr.read() & Self::BASE_PADDR_MASK);
        let base = mm::kernel_vaddr_of(base_paddr);
        tracing::debug!(?base, "found local APIC base address");
        assert_ne!(base, VAddr::from_u64(0));

        unsafe {
            // ensure the local APIC's MMIO page is mapped and writable.
            let virt = VirtPage::<Size4Kb>::containing_fixed(base);
            let phys = PhysPage::<Size4Kb>::containing_fixed(base_paddr);
            tracing::debug!(?virt, ?phys, "mapping local APIC MMIO page...");
            pagectrl
                .map_page(virt, phys, frame_alloc)
                .set_writable(true)
                .commit();
            tracing::debug!("mapped local APIC MMIO page");
        }

        Ok(Self { msr, base })
    }

    #[must_use]
    #[inline]
    pub fn new<A>(pagectrl: &mut impl page::Map<Size4Kb, A>, frame_alloc: &A) -> Self
    where
        A: page::Alloc<Size4Kb>,
    {
        Self::try_new(pagectrl, frame_alloc).unwrap()
    }

    pub fn enable(&self, spurious_vector: u8) {
        /// Writing this to the IA32_APIC_BASE MSR enables the local APIC.
        const MSR_ENABLE: u64 = 0x800;
        /// Bit 8 in the spurious interrupt vector register enables the APIC.
        const SPURIOUS_VECTOR_ENABLE_BIT: u32 = 1 << 8;

        // Write the enable bit to the MSR
        unsafe {
            self.msr.update(|base| base | MSR_ENABLE);
        }

        // Enable the APIC by writing the spurious vector to the APIC's
        // SPURIOUS_VECTOR register.
        let value = spurious_vector as u32 | SPURIOUS_VECTOR_ENABLE_BIT;
        unsafe { self.register(register::SPURIOUS_VECTOR).write(value) }
        tracing::info!(base = ?self.base, spurious_vector, "local APIC enabled");
    }

    #[tracing::instrument(
        level = tracing::Level::DEBUG,
        name = "LocalApic::start_periodic_timer",
        skip(self, interval),
        fields(?interval, vector),
        err
    )]
    pub fn start_periodic_timer(
        &self,
        interval: Duration,
        vector: u8,
    ) -> Result<(), InvalidDuration> {
        let timer_frequency_hz = self.timer_frequency_hz();
        let ticks_per_ms = timer_frequency_hz / 1000;
        tracing::trace!(
            timer_frequency_hz,
            ticks_per_ms,
            "starting local APIC timer"
        );
        let interval_ms: u32 = interval.as_millis().try_into().map_err(|_| {
            InvalidDuration::new(
                interval,
                "local APIC periodic timer interval exceeds a `u32`",
            )
        })?;
        let ticks_per_interval = interval_ms.checked_mul(ticks_per_ms).ok_or_else(|| {
            InvalidDuration::new(
                interval,
                "local APIC periodic timer interval requires a number of ticks that exceed a `u32`",
            )
        })?;

        unsafe {
            let lvt_entry = register::LvtTimer::new()
                .with(register::LvtTimer::VECTOR, vector)
                .with(register::LvtTimer::MODE, register::TimerMode::Periodic);
            // set the divisor to 16. (ed. note: how does 3 say this? idk lol...)
            self.write_register(register::TIMER_DIVISOR, 0b11);
            self.write_register(register::LVT_TIMER, lvt_entry);
            self.write_register(register::TIMER_INITIAL_COUNT, ticks_per_interval);
        }

        tracing::info!(
            ?interval,
            timer_frequency_hz,
            ticks_per_ms,
            vector,
            "started local APIC timer"
        );

        Ok(())
    }

    /// Sends an IPI (inter-processor interrupt) to another CPU.
    #[tracing::instrument(
        level = tracing::Level::DEBUG,
        name = "LocalApic::send_ipi",
        skip(self),
        err(Display)
    )]
    pub fn send_ipi(&self, target: IpiTarget, kind: IpiKind) -> Result<(), &'static str> {
        use register::{IcrFlags, IcrTarget, IpiInit, IpiMode};

        let mut flags = IcrFlags::new();
        match kind {
            IpiKind::Startup(page) => {
                let page_num: u8 = page
                    .number()
                    .try_into()
                    .map_err(|_| "SIPI page number must be <= 255")?;
                flags
                    .set(IcrFlags::VECTOR, page_num)
                    .set(IcrFlags::MODE, IpiMode::Sipi)
                    // This is set for all IPIs except INIT level deassert.
                    .set(IcrFlags::INIT_ASSERT_DEASSERT, IpiInit::Assert);
            }
            IpiKind::Init { assert } => {
                let assert = match assert {
                    true => IpiInit::Assert,
                    false => IpiInit::Deassert,
                };

                flags
                    .set(IcrFlags::MODE, IpiMode::InitDeinit)
                    .set(IcrFlags::INIT_ASSERT_DEASSERT, assert);
            }
            IpiKind::Interrupt { vector: _ } => {
                unreachable!("non-SIPI inter-processor interrupts are not currently implemented...")
            }
        }

        match target {
            IpiTarget::ApicId(apic_id) => {
                flags
                    .set(IcrFlags::DESTINATION, register::IpiDest::Target)
                    // when targeting a local APIC ID, the destination mode is
                    // "physical" (per osdev wiki).
                    .set(IcrFlags::IS_LOGICAL, false);
                assert!(
                    apic_id < 0b0000_1111,
                    "target APIC ID may not be more than 4 bits, got {apic_id:#b}"
                );
                let target_reg = IcrTarget::new().with(IcrTarget::APIC_ID, apic_id as u32);
                unsafe { self.write_register(register::ICR_TARGET, target_reg) };
            }
            // TODO(eliza): there are probably some IPIs that are invalid to
            // send to yourself...should we handle them here? e.g. i assume you
            // can't send yourself a SIPI. tbqh, sending any IPI to yourself
            // feels..onanic, at best.
            IpiTarget::Current => {
                flags.set(IcrFlags::DESTINATION, register::IpiDest::Current);
            }
            IpiTarget::All => {
                flags.set(IcrFlags::DESTINATION, register::IpiDest::All);
            }
            IpiTarget::Others => {
                flags.set(IcrFlags::DESTINATION, register::IpiDest::Others);
            }
        };

        tracing::debug!(?flags, "sending IPI...");
        unsafe {
            // write the ICR flags
            self.write_register(register::ICR_FLAGS, flags);

            // wait for the interrupt to be delivered.
            while self
                .register(register::ICR_FLAGS)
                .read()
                .get(IcrFlags::SEND_PENDING)
            {
                // spin
                core::hint::spin_loop();
            }
        };
        tracing::info!(?flags, ?target, "IPI sent!");

        Ok(())
    }

    /// Sends an End of Interrupt (EOI) to the local APIC.
    ///
    /// This should be called by an interrupt handler after handling a local
    /// APIC interrupt.
    ///
    /// # Safety
    ///
    /// This should only be called when an interrupt has been triggered by this
    /// local APIC.
    pub unsafe fn end_interrupt(&self) {
        // Write a 0 to the EOI register.
        self.register(register::END_OF_INTERRUPT).write(0);
    }

    unsafe fn write_register<T, A>(&self, register: LocalApicRegister<T, A>, value: T)
    where
        LocalApicRegister<T, A>: RegisterAccess<Target = T, Access = A>,
        A: access::Writable,
        T: Copy + fmt::Debug + 'static,
    {
        tracing::trace!(%register, write = ?value);
        self.register(register).write(value);
    }

    fn timer_frequency_hz(&self) -> u32 {
        use register::*;

        let cpuid = CpuId::new();

        if let Some(undivided_freq_khz) = cpuid.get_hypervisor_info().and_then(|hypervisor| {
            tracing::trace!("CPUID contains hypervisor info");
            let freq = hypervisor.apic_frequency();
            tracing::trace!(hypervisor.apic_frequency = ?freq);
            NonZeroU32::new(freq?)
        }) {
            // the hypervisor info CPUID leaf expresses the frequency in kHz,
            // and the frequency is not divided by the target timer divisor.
            let frequency_hz = undivided_freq_khz.get() / 1000 / Self::TIMER_DIVISOR;
            tracing::debug!(
                frequency_hz,
                "determined APIC frequency from CPUID hypervisor info"
            );
            return frequency_hz;
        }

        if let Some(undivided_freq_hz) = cpuid.get_tsc_info().and_then(|tsc| {
            tracing::trace!("CPUID contains TSC info");
            let freq = tsc.nominal_frequency();
            NonZeroU32::new(freq)
        }) {
            // divide by the target timer divisor.
            let frequency_hz = undivided_freq_hz.get() / Self::TIMER_DIVISOR;
            tracing::debug!(
                frequency_hz,
                "determined APIC frequency from CPUID TSC info"
            );
            return frequency_hz;
        }

        // CPUID didn't help, so fall back to calibrating the APIC frequency
        // using the PIT.
        tracing::debug!("calibrating APIC timer frequency using PIT...");
        unsafe {
            // set timer divisor to 16
            self.write_register(TIMER_DIVISOR, 0b11);
            // set initial count to -1
            self.write_register(TIMER_INITIAL_COUNT, -1i32 as u32);

            // start the timer
            self.write_register(
                LVT_TIMER,
                LvtTimer::new().with(LvtTimer::MODE, TimerMode::OneShot),
            );
        }

        // use the PIT to sleep for 10ms
        crate::time::PIT
            .lock()
            .sleep_blocking(Duration::from_millis(10))
            .expect("the PIT should be able to send a 10ms interrupt...");

        unsafe {
            // stop the timer
            self.write_register(
                LVT_TIMER,
                LvtTimer::new().with(LvtTimer::MODE, TimerMode::Periodic),
            );
        }

        let elapsed_ticks = unsafe { self.register(TIMER_CURRENT_COUNT).read() };
        // since we slept for ten milliseconds, each tick is 10 kHz. we don't
        // need to account for the divisor since we ran the timer at that
        // divisor already.
        let ticks_per_10ms = (-1i32 as u32).wrapping_sub(elapsed_ticks);
        tracing::debug!(?ticks_per_10ms);
        // convert the frequency to Hz.
        let frequency_hz = ticks_per_10ms * 100;
        tracing::debug!(frequency_hz, "calibrated local APIC timer using PIT");
        frequency_hz
    }

    #[must_use]
    unsafe fn register<T, A>(
        &self,
        register: LocalApicRegister<T, A>,
    ) -> Volatile<&'static mut T, A>
    where
        LocalApicRegister<T, A>: RegisterAccess<Target = T, Access = A>,
    {
        let addr = self.base + register.offset;
        assert!(
            addr.is_aligned(16usize),
            "Local APIC memory-mapped registers must be 16-byte aligned!"
        );
        let reference = &mut *addr.as_ptr::<T>();
        LocalApicRegister::<T, A>::volatile(reference)
    }
}

pub mod register {
    use super::*;
    use mycelium_util::bits::{bitfield, FromBits};
    use volatile::access::*;

    impl<T> RegisterAccess for LocalApicRegister<T, ReadOnly> {
        type Access = ReadOnly;
        type Target = T;
        fn volatile(
            ptr: &'static mut Self::Target,
        ) -> Volatile<&'static mut Self::Target, Self::Access> {
            Volatile::new_read_only(ptr)
        }
    }

    impl<T> RegisterAccess for LocalApicRegister<T, ReadWrite> {
        type Access = ReadWrite;
        type Target = T;
        fn volatile(
            ptr: &'static mut Self::Target,
        ) -> Volatile<&'static mut Self::Target, Self::Access> {
            Volatile::new(ptr)
        }
    }

    impl<T> RegisterAccess for LocalApicRegister<T, WriteOnly> {
        type Access = WriteOnly;
        type Target = T;
        fn volatile(
            ptr: &'static mut Self::Target,
        ) -> Volatile<&'static mut Self::Target, Self::Access> {
            Volatile::new_write_only(ptr)
        }
    }

    impl<T, A> fmt::Display for LocalApicRegister<T, A> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let Self { name, offset, _ty } = self;
            write!(f, "{name} ({offset:#x})")
        }
    }

    macro_rules! registers {
        ( $( $(#[$m:meta])* $NAME:ident$(<$T:ty>)? = $offset:literal, $access:ident);+ $(;)? ) => {
            $(
                registers! {@ $(#[$m])* $NAME$(<$T>)? = $offset, $access }
            )+
        };
        (@ $(#[$m:meta])* $NAME:ident = $offset:literal, $access:ident) => {
            registers!{@ $(#[$m])* $NAME<u32> = $offset, $access }
        };
        (@ $(#[$m:meta])* $NAME:ident<$T:ty> = $offset:literal, $access:ident )=> {
            $(#[$m])*
            pub const $NAME: LocalApicRegister<$T, $access> = LocalApicRegister {
                offset: $offset,
                name: stringify!($NAME),
                _ty: PhantomData,
            };
        };
    }

    registers! {

        /// Local APIC ID
        ///
        /// **Access**: read/write
        ID = 0x020, ReadWrite;

        /// Local APIC version
        ///
        /// **Access**: read-only
        VERSION = 0x030, ReadOnly;

        /// Task Priority Register (TPR)
        ///
        /// **Access**: read/write
        TASK_PRIORITY = 0x080, ReadWrite;

        /// Arbitration Priority Register (APR)
        ///
        /// **Access**: read-only
        ARBITRATION_PRIORITY = 0x090, ReadOnly;

        /// Processor Priority Register (APR)
        ///
        /// **Access**: read-only
        PROCESSOR_PRIORITY = 0x0a0, ReadOnly;

        /// End of Interrupt (EOI) Register
        ///
        /// **Access**: write-only
        END_OF_INTERRUPT = 0x0b0, WriteOnly;

        /// Remote Read Register (RRD)
        ///
        /// **Access**: read-only
        REMOTE_READ = 0x0c0, ReadOnly;

        /// Logical Destination Register
        ///
        /// **Access**: read/write
        LOGICAL_DEST = 0x0d0, ReadWrite;

        /// Destination Format Register
        ///
        /// **Access**: read/write
        DEST_FORMAT = 0x0e0, ReadWrite;

        /// Spurious Interrupt Vector Register
        ///
        /// **Access**: read/write
        SPURIOUS_VECTOR = 0x0f0, ReadWrite;

        /// In-Service Register (ISR) 0
        ///
        /// **Access**: read-only
        IN_SERVICE_0 = 0x100, ReadOnly;

        /// Error Status Register (ESR)
        ///
        /// **Access**: read-only
        ERROR_STATUS = 0x280, ReadOnly;

        /// LVT Corrected Machine Check Interrupt (CMCI) Register
        ///
        /// *Access**: read/write
        LVT_CMCI = 0x2f0, ReadWrite;

        /// Interrupt Command Register (ICR), Low Half (Flags)
        ///
        /// Bitflags for this register are represented by the [`IcrFlags`] type.
        ///
        /// The interrupt command register (ICR) is used for sending
        /// inter-processor interrupts (IPIs) to other processors. It consists
        /// of two separate 32-bit registers, the low half ([`ICR_FLAGS`]) at
        /// 0x300, and the high half ([`ICR_TARGET`]) at 0x310. The low half
        /// contains flags describing the / inter-processor interrupt, while the
        /// high half contains the local APIC ID of the target processor.
        ///
        /// Note that the interrupt is sent when 0x300 ([`ICR_FLAGS`]) is
        /// written to, so in order to send an interrupt, 0x310 should be
        /// written prior to writing to 0x300.
        ICR_FLAGS<IcrFlags> = 0x300, ReadWrite;

        /// Interrupt Command Register (ICR), High Half (Target)
        ///
        /// The interrupt command register (ICR) is used for sending
        /// inter-processor interrupts (IPIs) to other processors. It consists
        /// of two separate 32-bit registers, the low half ([`ICR_FLAGS`]) at
        /// 0x300, and the high half ([`ICR_TARGET`]) at 0x310. The low half
        /// contains flags describing the / inter-processor interrupt, while the
        /// high half contains the local APIC ID of the target processor.
        ///
        /// Note that the interrupt is sent when 0x300 ([`ICR_FLAGS`]) is
        /// written to, so in order to send an interrupt, 0x310 should be
        /// written prior to writing to 0x300.
        ICR_TARGET<IcrTarget> = 0x310, ReadWrite;

        /// LVT Local APIC Timer
        ///
        /// This register is used to configure an interrupt in the [Local Vector
        /// Table (LVT)][lvt]. This register configures the LVT's local APIC timer
        /// interrupt.
        ///
        /// Bitflags for this register are represented by the [`LvtTimer`] type.
        ///
        /// [lvt]: https://wiki.osdev.org/APIC#Local_Vector_Table_Registers
        LVT_TIMER<LvtTimer> = 0x320, ReadWrite;

        /// LVT Thermal Sensor Interrupt
        ///
        /// This register is used to configure an interrupt in the [Local Vector
        /// Table (LVT)][lvt]. This register configures the LVT interrupts
        /// generated by the CPU's thermal sensor.
        ///
        /// Bitflags for this register are represented by the [`LvtEntry`] type.
        ///
        /// [lvt]: https://wiki.osdev.org/APIC#Local_Vector_Table_Registers
        LVT_THERMAL<LvtEntry> = 0x330, ReadWrite;

        /// LVT Performance Monitoring Counters
        ///
        /// This register is used to configure an interrupt in the [Local Vector
        /// Table (LVT)][lvt]. This register configures the LVT interrupts
        /// generated by performance monitoring counters.
        ///
        /// Bitflags for this register are represented by the [`LvtEntry`] type.
        ///
        /// [lvt]: https://wiki.osdev.org/APIC#Local_Vector_Table_Registers
        LVT_PERF<LvtEntry> = 0x340, ReadWrite;

        /// LVT Local Interrupt 0 (LINT0)
        ///
        /// This register is used to configure an interrupt in the [Local Vector
        /// Table (LVT)][lvt]. This register configures the LVT's Local
        /// Interrupt 0 (LINT0) interrupt.
        ///
        /// Bitflags for this register are represented by the [`LvtEntry`] type.
        ///
        /// [lvt]: https://wiki.osdev.org/APIC#Local_Vector_Table_Registers
        LVT_LINT0<LvtEntry> = 0x350, ReadWrite;

        /// LVT Local Interrupt 1 (LINT1)
        ///
        /// This register is used to configure an interrupt in the [Local Vector
        /// Table (LVT)][lvt]. This register configures the LVT's Local
        /// Interrupt 1 (LINT1) interrupt.
        ///
        /// Bitflags for this register are represented by the [`LvtEntry`] type.
        ///
        /// [lvt]: https://wiki.osdev.org/APIC#Local_Vector_Table_Registers
        LVT_LINT1<LvtEntry> = 0x360, ReadWrite;


        /// LVT Error Interrupt
        ///
        /// This register is used to configure an interrupt in the [Local Vector
        /// Table (LVT)][lvt].  This register configures the LVT error interrupt.
        ///
        /// Bitflags for this register are represented by the [`LvtEntry`] type.
        ///
        /// [lvt]: https://wiki.osdev.org/APIC#Local_Vector_Table_Registers
        LVT_ERROR<LvtEntry> = 0x370, ReadWrite;

        TIMER_INITIAL_COUNT = 0x380, ReadWrite;
        TIMER_CURRENT_COUNT = 0x390, ReadOnly;
        TIMER_DIVISOR = 0x3e0, ReadWrite;
    }

    bitfield! {
        pub struct LvtTimer<u32> {
            pub const VECTOR: u8;
            const _RESERVED_0 = 4;
            pub const SEND_PENDING: bool;
            const _RESERVED_1 = 3;
            pub const MASKED: bool;
            pub const MODE: TimerMode;
        }
    }

    bitfield! {
        /// Bitflags configuring an interrupt entry in the [Local Vector Table
        /// (LVT)][lvt].
        ///
        /// This type represents the values of the [`LVT_PERF`],
        /// [`LVT_THERMAL`], [`LVT_LINT0`], [`LVT_LINT1`], and [`LVT_ERROR`]
        /// registers.
        ///
        /// [lvt]: https://wiki.osdev.org/APIC#Local_Vector_Table_Registers
        pub struct LvtEntry<u32> {
            /// The vector number for this interrupt.
            pub const VECTOR: u8;
            const _RESERVED_0 = 2;
            /// If set, this interrupt will trigger a Non-Maskable Interrupt
            /// (NMI).
            pub const NMI: bool;
            const _RESERVED_1 = 1;
            /// Set if this interrupt is currently pending.
            pub const PENDING: bool;
            /// Configures the pin polarity of this interrupt.
            ///
            /// If this bit is set, the interrupt is low-triggered.
            pub const POLARITY: PinPolarity;
            /// Remote IRR
            ///
            /// XXX(eliza): what does this mean? OSDev Wiki just says that this
            /// bit means "remote IRR" but doesn't say what a "remote IRR"
            /// is...or even what "IRR" stands for...
            pub const REMOTE_IRR: bool;
            /// Trigger mode.
            ///
            /// If this bit is set, the interrupt is level-triggered.
            pub const TRIGGER: TriggerMode;
            /// If set, this interrupt is masked.
            pub const MASKED: bool;
        }
    }

    // === TimerMode ===

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u8)]
    pub enum TimerMode {
        /// One-shot mode, program count-down value in an initial-count register.
        OneShot = 0b00,
        /// Periodic mode, program interval value in an initial-count register.
        Periodic = 0b01,
        /// TSC-Deadline mode, program target value in IA32_TSC_DEADLINE MSR.
        TscDeadline = 0b10,
    }

    impl FromBits<u32> for TimerMode {
        const BITS: u32 = 2;
        type Error = &'static str;

        fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
            match bits {
                bits if bits as u8 == Self::OneShot as u8 => Ok(Self::OneShot),
                bits if bits as u8 == Self::Periodic as u8 => Ok(Self::Periodic),
                bits if bits as u8 == Self::TscDeadline as u8 => Ok(Self::TscDeadline),
                _ => Err("0b11 is not a valid local APIC timer mode"),
            }
        }

        fn into_bits(self) -> u32 {
            self as u8 as u32
        }
    }

    bitfield! {
        /// Interrupt Command Register (ICR) flags.
        ///
        /// This is the value of the ([`ICR_FLAGS`]) register.
        #[derive(Eq, PartialEq)]
        pub struct IcrFlags<u32> {
            /// The vector number, or starting page number for SIPIs.
            pub const VECTOR: u8;
            /// The destination mode.
            ///
            /// 0 is normal, 1 is lowest priority, 2 is SMI, 4 is NMI, 5 can be
            /// INIT or INIT level de-assert, 6 is a SIPI.
            pub const MODE: IpiMode;
            /// The destination mode.
            ///
            /// Clear for a physical destination, or set for a logical
            /// destination. If the bit is clear, then the destination field in
            /// 0x310 is treated normally.
            /// XXX(eliza): what does "normally" mean? this came from osdev wiki
            /// lol.
            pub const IS_LOGICAL: bool;
            /// Delivery status.
            ///
            /// Cleared when the interrupt has been accepted by the target. You
            /// should usually wait until this bit clears after sending an
            /// interrupt.
            pub const SEND_PENDING: bool;
            const _RESERVED = 1;
            pub const INIT_ASSERT_DEASSERT: IpiInit;

            /// Destination selection.
            ///
            /// If this is > 0 then the destination field in 0x310 is ignored. 1
            /// will always send the interrupt to itself, 2 will send it to all
            /// processors, and 3 will send it to all processors aside from the
            /// current one. It is best to avoid using modes 1, 2 and 3, and stick with 0.
            pub const DESTINATION: IpiDest;
        }
    }

    bitfield! {
        /// Interrupt Command Register (ICR) target register.
        ///
        /// This is the value of the ([`ICR_TARGET`]) register.
        #[derive(Eq, PartialEq)]
        pub struct IcrTarget<u32> {
            /// XXX(eliza): if this is an x2APIC, the destination starts at bit
            /// 0 i think?
            const _RESERVED_0 = 24;

            /// The local APIC ID of the target processor.
            pub const APIC_ID = 4;
        }
    }

    /// Inter-processor Interrupt (IPI) Destination Modes.
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u8)]
    pub enum IpiMode {
        /// Normal priority.
        Normal = 0,
        /// Lowest priority.
        LowPrio = 1,
        /// System Management Interrupt (SMI)
        Smi = 2,
        /// Non-Maskable Interrupt (NMI)
        Nmi = 4,
        // INIT or INIT level de-assert
        InitDeinit = 5,
        /// SIPI
        Sipi = 6,
    }

    /// Inter-processor Interrupt (IPI) Init level assert/de-assert.
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u8)]
    pub enum IpiInit {
        /// Assert INIT level (or any other ICR command if not sending an INIT).
        Assert = 0x01,
        /// Deassert INIT level.
        Deassert = 0x10,
    }

    /// Inter-processor Interrupt (IPI) destination selection.
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u8)]
    pub enum IpiDest {
        /// The interrupt is sent to the target local APIC in the [`ICR_TARGET`]
        /// register.
        Target = 0,
        /// The interrupt is sent to this processor.
        Current = 1,
        /// The interrupt is sent to all processors, *including* the current one.
        All = 2,
        /// The interrupt is sent to all processors *except* the current one.
        Others = 3,
    }

    impl FromBits<u32> for IpiMode {
        const BITS: u32 = 3;
        type Error = &'static str;

        fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
            match bits {
                bits if bits as u8 == Self::Normal as u8 => Ok(Self::Normal),
                bits if bits as u8 == Self::LowPrio as u8 => Ok(Self::LowPrio),
                bits if bits as u8 == Self::Smi as u8 => Ok(Self::Smi),
                bits if bits as u8 == Self::Nmi as u8 => Ok(Self::Nmi),
                bits if bits as u8 == Self::InitDeinit as u8 => Ok(Self::InitDeinit),
                bits if bits as u8 == Self::Sipi as u8 => Ok(Self::Sipi),
                _ => Err("not a valid IPI mode"),
            }
        }

        fn into_bits(self) -> u32 {
            self as u8 as u32
        }
    }

    impl FromBits<u32> for IpiInit {
        const BITS: u32 = 2;
        type Error = &'static str;

        fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
            match bits {
                bits if bits as u8 == Self::Assert as u8 => Ok(Self::Assert),
                bits if bits as u8 == Self::Deassert as u8 => Ok(Self::Deassert),
                _ => Err("not a valid ICR INIT assert/deassert mode"),
            }
        }

        fn into_bits(self) -> u32 {
            self as u8 as u32
        }
    }

    impl FromBits<u32> for IpiDest {
        const BITS: u32 = 2;
        type Error = &'static str;

        fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
            match bits {
                bits if bits as u8 == Self::Target as u8 => Ok(Self::Target),
                bits if bits as u8 == Self::Current as u8 => Ok(Self::Current),
                bits if bits as u8 == Self::All as u8 => Ok(Self::All),
                bits if bits as u8 == Self::Others as u8 => Ok(Self::Others),
                _ => unreachable!("2 bits has 4 possible values!"),
            }
        }

        fn into_bits(self) -> u32 {
            self as u8 as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::register::{IcrFlags, IcrTarget, IpiInit, IpiMode};
    use super::*;

    #[test]
    fn lvt_entry_is_valid() {
        register::LvtEntry::assert_valid();
    }

    #[test]
    fn lvt_timer_is_valid() {
        register::LvtTimer::assert_valid();
    }

    #[test]
    fn lvt_timer_offsets() {
        assert_eq!(
            register::LvtTimer::VECTOR.least_significant_index(),
            0,
            "vector LSB"
        );
        assert_eq!(
            register::LvtTimer::VECTOR.most_significant_index(),
            8,
            "vector MSB"
        );
        assert_eq!(
            register::LvtTimer::SEND_PENDING.least_significant_index(),
            12,
            "send pending"
        );
        assert_eq!(
            register::LvtTimer::MASKED.least_significant_index(),
            16,
            "masked MSB"
        );
        assert_eq!(
            register::LvtTimer::MODE.least_significant_index(),
            17,
            "mode LSB"
        );
    }

    #[test]
    fn init_icr() {
        let icr = IcrFlags::new()
            .with(IcrFlags::VECTOR, 0)
            .with(IcrFlags::MODE, IpiMode::InitDeinit)
            .with(IcrFlags::INIT_ASSERT_DEASSERT, IpiInit::Assert);
        println!("{icr}");
        assert_eq!(icr, IcrFlags::from_bits(0x4500))
    }

    #[test]
    fn sipi_icr() {
        let icr = IcrFlags::new()
            .with(IcrFlags::VECTOR, 8) // page 8
            .with(IcrFlags::MODE, IpiMode::Sipi)
            .with(IcrFlags::INIT_ASSERT_DEASSERT, IpiInit::Assert);
        println!("{icr}");
        assert_eq!(icr, IcrFlags::from_bits(0x4600 | 8))
    }

    #[test]
    fn icr_target() {
        let icr = IcrTarget::new().with(IcrTarget::APIC_ID, 1);
        assert_eq!(icr, IcrTarget::from_bits(1 << 24));
    }
}
