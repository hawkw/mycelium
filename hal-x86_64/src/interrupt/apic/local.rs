//! Local APIC
pub use self::register::{ErrorStatus, Version};
use self::register::{LvtTimer, TimerMode};
use super::{PinPolarity, TriggerMode};
use crate::{
    cpu::{local, FeatureNotSupported, Msr},
    mm::{self, page, size::Size4Kb, PhysPage, VirtPage},
    time::{Duration, InvalidDuration},
};
use core::{
    cell::{Cell, RefCell},
    convert::TryInto,
    marker::PhantomData,
    num::NonZeroU32,
};
use hal_core::{PAddr, VAddr};
use mycelium_util::fmt;
use raw_cpuid::CpuId;
use register::TimerDivisor;
use volatile::{access, Volatile};

/// Local APIC state.
///
///# Notes on Mutability
///
/// In a multi-core x86_64 system, each CPU core has its own local APIC.
/// Therefore, the Mycelium HAL's [`interrupt::Controller`] type uses
/// [core-local storage] to store a reference to each CPU core's local APIC that
/// can be accessed only by that core. The value in core-local storage is a
/// [`RefCell`]`<`[`Option`]`<`[`LocalApic`]`>>`, as the local APIC must be
/// initialized on a per-core basis before it can be used.
///
/// Note that the [`RefCell`] must *only* be [borrowed
/// mutably](RefCell::borrow_mut) in order to *initialize* the `LocalApic`
/// value. Once the local APIC has been initialized, it may receive interrupts,
/// and may also be accessed by interrupt *handlers* in order to send an
/// end-of-interrupt. This means that mutably borrowing the `RefCell` once
/// interrupts are enabled may result in a panic if an ISR attempts to borrow
/// the local APIC immutably to send an EOI while the local APIC is borrowed
/// mutably.
///
/// Therefore, all local APIC operations that mutate state, such as setting the
/// timer calibration, must use interior mutability, so that an interrupt
/// handler may access the local APIC's state. Interior mutability need not be
/// thread-safe, however, as the local APIC state is only accessed from the core
/// that owns it.
///
/// [`interrupt::Controller`]: crate::interrupt::Controller
/// [core-local storage]: crate::cpu::local
#[derive(Debug)]
pub struct LocalApic {
    msr: Msr,
    base: VAddr,
    timer_calibration: Cell<Option<TimerCalibration>>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct TimerCalibration {
    frequency_hz: u32,
    divisor: register::TimerDivisor,
}

/// Represents a register in the local APIC's configuration area.
#[derive(Debug)]
pub struct LocalApicRegister<T = u32, A = access::ReadWrite> {
    offset: usize,
    name: &'static str,
    _ty: PhantomData<fn(T, A)>,
}

#[derive(Debug)]
pub(in crate::interrupt) struct Handle(local::LocalKey<RefCell<Option<LocalApic>>>);

pub trait RegisterAccess {
    type Access;
    type Target;
    fn volatile(
        ptr: &'static mut Self::Target,
    ) -> Volatile<&'static mut Self::Target, Self::Access>;
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum LocalApicError {
    /// The system is configured to use the PIC interrupt model rather than the
    /// APIC interrupt model.
    #[error("interrupt model is PIC, not APIC")]
    NoApic,

    /// The local APIC is uninitialized.
    #[error("the local APIC has not been initialized on this core")]
    Uninitialized,
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum TimerError {
    #[error(transparent)]
    InvalidDuration(#[from] InvalidDuration),
    #[error("local APIC timer has not been calibrated on this core")]
    NotCalibrated,
}

impl LocalApic {
    const BASE_PADDR_MASK: u64 = 0xffff_ffff_f000;

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

        Ok(Self {
            msr,
            base,
            timer_calibration: Cell::new(None),
        })
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
        use register::Version;
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

        let version = self.version();
        tracing::info!(
            base = ?self.base,
            spurious_vector,
            version = ?version.get(Version::VERSION),
            max_lvt_entry = ?version.get(Version::MAX_LVT),
            supports_eoi_suppression = ?version.get(Version::EOI_SUPPRESSION),
            "local APIC enabled",
        );
    }

    pub fn calibrate_timer(&self, divisor: TimerDivisor) {
        if let Some(prev) = self.timer_calibration.get() {
            if prev.divisor == divisor {
                tracing::info!(
                    ?divisor,
                    "local APIC timer already calibrated with this divisor"
                );
                return;
            }
        }
        self.timer_calibration.set(Some(TimerCalibration {
            frequency_hz: self.calibrate_frequency_hz(divisor),
            divisor,
        }));
    }

    /// Reads the local APIC's version register.
    ///
    /// The returned [`Version`] struct indicates the version of the local APIC,
    /// the maximum LVT entry index, and whether or not EOI suppression is
    /// supported.
    pub fn version(&self) -> register::Version {
        unsafe { self.register(register::VERSION) }.read()
    }

    fn calibrate_frequency_hz(&self, divisor: TimerDivisor) -> u32 {
        // How sloppy do we expect the PIT frequency calibration to be?
        // If the delta between the CPUID frequency and the frequency we
        // determined using PIT calibration is > this value, we'll yell about
        // it.
        const PIT_SLOPPINESS: u32 = 1000;

        // Start out by calibrating the APIC frequency using the PIT.
        let pit_frequency_hz = self.calibrate_frequency_hz_pit(divisor);

        // See if we can get something from CPUID.
        let Some(cpuid_frequency_hz) = Self::calibrate_frequency_hz_cpuid(divisor) else {
            tracing::info!(
                pit.frequency_hz = pit_frequency_hz,
                "CPUID does not indicate APIC timer frequency; using PIT \
                 calibration only"
            );
            return pit_frequency_hz;
        };

        // Cross-check the PIT calibration result and CPUID value.
        let distance = if pit_frequency_hz > cpuid_frequency_hz {
            pit_frequency_hz - cpuid_frequency_hz
        } else {
            cpuid_frequency_hz - pit_frequency_hz
        };
        if distance > PIT_SLOPPINESS {
            tracing::warn!(
                pit.frequency_hz = pit_frequency_hz,
                cpuid.frequency_hz = cpuid_frequency_hz,
                distance,
                ?divisor,
                "APIC timer frequency from PIT calibration differs substantially \
                 from CPUID!"
            );
        }

        cpuid_frequency_hz
    }

    /// Calibrate the timer frequency using the PIT.
    #[inline(always)]
    fn calibrate_frequency_hz_pit(&self, divisor: TimerDivisor) -> u32 {
        tracing::debug!(?divisor, "calibrating APIC timer frequency using PIT...");
        const ATTEMPTS: usize = 5;

        // lock the PIT now, before actually starting the timer IRQ, so that we
        // don't include any time spent waiting for the PIT lock.
        //
        // since we only run this code on startup, before any other cores have
        // been started, this probably never actually waits for a lock. but...we
        // should do it the right way, anyway.
        let mut pit = crate::time::PIT.lock();

        let mut sum = 0;
        let mut min = u32::MAX;
        let mut max = 0;
        for attempt in 1..ATTEMPTS + 1 {
            unsafe {
                // start the timer

                // set timer divisor to 16
                self.write_register(register::TIMER_DIVISOR, divisor);
                self.register(register::LVT_TIMER).update(|lvt_timer| {
                    lvt_timer
                        .set(LvtTimer::MODE, TimerMode::OneShot)
                        .set(LvtTimer::MASKED, false);
                });
                // set initial count to -1
                self.write_register(register::TIMER_INITIAL_COUNT, -1i32 as u32);
            }

            // use the PIT to sleep for 10ms
            pit.sleep_blocking(Duration::from_millis(10))
                .expect("the PIT should be able to send a 10ms interrupt...");

            unsafe {
                // stop the timer
                self.register(register::LVT_TIMER).update(|lvt_timer| {
                    lvt_timer.set(LvtTimer::MASKED, true);
                });
            }

            let elapsed_ticks = unsafe { self.register(register::TIMER_CURRENT_COUNT).read() };
            // since we slept for ten milliseconds, each tick is 10 kHz. we don't
            // need to account for the divisor since we ran the timer at that
            // divisor already.
            let ticks_per_10ms = (-1i32 as u32).wrapping_sub(elapsed_ticks);
            // convert the frequency to Hz.
            let frequency_hz = ticks_per_10ms * 100;
            // TODO(eliza): throw out attempts that differ too substantially
            // from the min/max?
            sum += frequency_hz;
            min = core::cmp::min(min, frequency_hz);
            max = core::cmp::max(max, frequency_hz);
            tracing::trace!(attempt, frequency_hz, sum, min, max);
        }
        let frequency_hz = sum / ATTEMPTS as u32;
        tracing::debug!(
            ?divisor,
            frequency_hz,
            min_hz = min,
            max_hz = max,
            "calibrated local APIC timer using PIT"
        );
        frequency_hz
    }

    #[inline(always)]
    fn calibrate_frequency_hz_cpuid(divisor: TimerDivisor) -> Option<u32> {
        let cpuid = CpuId::new();
        if let Some(freq_khz) = cpuid.get_hypervisor_info().and_then(|hypervisor| {
            tracing::trace!("CPUID contains hypervisor info");
            let freq = hypervisor.apic_frequency();
            tracing::trace!(hypervisor.apic_frequency_khz = ?freq);
            NonZeroU32::new(freq?)
        }) {
            // the hypervisor info CPUID leaf expresses the frequency in kHz,
            // and the frequency is not divided by the target timer divisor.
            let frequency_hz = (freq_khz.get() * 1000) / divisor.into_divisor();
            tracing::debug!(
                ?divisor,
                frequency_hz,
                "determined APIC timer frequency from CPUID hypervisor info"
            );
            return Some(frequency_hz);
        }

        // XXX ELIZA THIS IS TSC FREQUENCY, SO IDK IF THAT'S RIGHT?
        /*
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
        */

        // CPUID didn't help, so fall back to calibrating the APIC frequency
        // using the PIT.
        None
    }

    pub fn interrupt_in(&self, duration: Duration, vector: u8) -> Result<(), TimerError> {
        let TimerCalibration {
            frequency_hz,
            divisor,
        } = self
            .timer_calibration
            .get()
            .ok_or(TimerError::NotCalibrated)?;
        let duration_ms: u32 = duration.as_millis().try_into().map_err(|_| {
            InvalidDuration::new(
                duration,
                "local APIC oneshot timer duration exceeds a `u32`",
            )
        })?;
        let ticks = duration_ms
            .checked_mul(frequency_hz / 1000)
            .ok_or_else(|| {
                InvalidDuration::new(
                duration,
                "local APIC oneshot timer duration requires a number of ticks that exceed a `u32`",
            )
            })?;
        unsafe {
            self.configure_timer(divisor, TimerMode::OneShot, vector, ticks);
        }

        Ok(())
    }

    #[tracing::instrument(
        level = tracing::Level::DEBUG,
        name = "LocalApic::start_periodic_timer",
        skip(self, interval),
        fields(?interval, vector),
        err
    )]
    pub fn start_periodic_timer(&self, interval: Duration, vector: u8) -> Result<(), TimerError> {
        let TimerCalibration {
            frequency_hz,
            divisor,
        } = self
            .timer_calibration
            .get()
            .ok_or(TimerError::NotCalibrated)?;
        let ticks_per_ms = frequency_hz / 1000;
        tracing::trace!(
            frequency_hz,
            ticks_per_ms,
            ?divisor,
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
            self.configure_timer(divisor, TimerMode::Periodic, vector, ticks_per_interval);
        }

        tracing::info!(
            ?interval,
            frequency_hz,
            ticks_per_ms,
            vector,
            "started local APIC timer"
        );

        Ok(())
    }

    #[inline(always)]
    unsafe fn configure_timer(
        &self,
        divisor: TimerDivisor,
        mode: TimerMode,
        vector: u8,
        initial_count: u32,
    ) {
        // Set the divisor configuration, update the LVT entry, and set the
        // initial count. Per the OSDev Wiki, we must do this in this specific
        // order (divisor, then unmask the LVT entry, then set the initial
        // count), or else we may miss IRQs or have other issues. I'm not sure
        // why this is, but see:
        // https://wiki.osdev.org/APIC_Timer#Enabling_APIC_Timer
        self.register(register::TIMER_DIVISOR).write(divisor);
        self.register(register::LVT_TIMER).update(|lvt_timer| {
            lvt_timer
                .set(LvtTimer::VECTOR, vector)
                .set(LvtTimer::MODE, mode)
                .set(LvtTimer::MASKED, false);
        });
        self.register(register::TIMER_INITIAL_COUNT)
            .write(initial_count)
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

    /// Reads the error stauts register (`ESR`) of the local APIC.
    ///
    /// Calling this method resets the value of the error status register. Any
    /// currently set error bits are cleared. If the same error bits are present
    /// in a subsequent call to `LocalApic::check_error`, they represent *new*
    /// instances of the same error.
    ///
    /// # Returns
    ///
    /// If any error bits are set, this method returns
    /// [`Err`]`(`[`ErrorStatus`]`)`. Otherwise, if no error bits are present,
    /// this method returns [`Ok`]`()`.
    pub fn check_error(&self) -> Result<(), register::ErrorStatus> {
        let esr = unsafe {
            let mut reg = self.register(register::ERROR_STATUS);

            // Per the Intel SDM, Vol 3A, Ch 7, Section 12.5.3, "Error
            // Handling":
            //
            // > The ESR is a write/read register. Before attempt to read from
            // > the ESR, software should first write to it. (The value written
            // > does not affect the values read subsequently; only zero may be
            // > written in x2APIC mode.) This write clears  any previously
            // > logged errors and updates the ESR with any errors detected
            // > since the last write to the ESR. This write also rearms the
            // > APIC error interrupt triggering mechanism.
            //
            // So, first write a zero.
            reg.write(register::ErrorStatus::default());
            reg.read()
        };

        // Return the ESR value if error bits are set.
        if esr.is_error() {
            Err(esr)
        } else {
            Ok(())
        }
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

impl Handle {
    pub(in crate::interrupt) const fn new() -> Self {
        Self(local::LocalKey::new(|| RefCell::new(None)))
    }

    pub(in crate::interrupt) fn with<T>(
        &self,
        f: impl FnOnce(&LocalApic) -> T,
    ) -> Result<T, LocalApicError> {
        self.0.with(|apic| {
            Ok(f(apic
                .borrow()
                .as_ref()
                .ok_or(LocalApicError::Uninitialized)?))
        })
    }

    pub(in crate::interrupt) unsafe fn initialize<A>(
        &self,
        frame_alloc: &A,
        pagectrl: &mut impl page::Map<Size4Kb, A>,
        spurious_vector: u8,
    ) where
        A: page::Alloc<Size4Kb>,
    {
        self.0.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                // already initialized, bail.
                return;
            }
            let apic = LocalApic::new(pagectrl, frame_alloc);
            apic.enable(spurious_vector);
            *slot = Some(apic);
        })
    }
}

pub mod register {
    use super::*;
    use mycelium_util::bits::{bitfield, enum_from_bits};
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
        VERSION<Version> = 0x030, ReadOnly;

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
        /// **Access**: read/write
        ERROR_STATUS<ErrorStatus> = 0x280, ReadWrite;

        /// LVT Corrected Machine Check Interrupt (CMCI) Register
        ///
        /// *Access**: read/write
        LVT_CMCI = 0x2f0, ReadWrite;

        ICR_LOW = 0x300, ReadWrite;
        ICR_HIGH = 0x310, ReadWrite;

        LVT_TIMER<LvtTimer> = 0x320, ReadWrite;
        LVT_THERMAL<LvtEntry> = 0x330, ReadWrite;
        LVT_PERF<LvtEntry> = 0x340, ReadWrite;
        LVT_LINT0<LvtEntry> = 0x350, ReadWrite;
        LVT_LINT1<LvtEntry> = 0x360, ReadWrite;
        LVT_ERROR<LvtEntry> = 0x370, ReadWrite;

        TIMER_INITIAL_COUNT = 0x380, ReadWrite;
        TIMER_CURRENT_COUNT = 0x390, ReadOnly;
        TIMER_DIVISOR<TimerDivisor> = 0x3e0, ReadWrite;
    }

    bitfield! {
        /// Value of the `VERSION` register in the local APIC.
        pub struct Version<u32> {
            /// The version numbers of the local APIC:
            /// - 0XH: 82489DX discrete APIC.
            /// -10H - 15H: Integrated APIC.
            /// Other values reserved.
            pub const VERSION: u8;
            const _RESERVED_0 = 8;
            /// The maximum number of LVT entries in the local APIC, minus one.
            ///
            /// For the Pentium 4 and Intel Xeon processors (which
            /// have 6 LVT entries), the value returned in the Max LVT field is
            /// 5; for the P6 family processors (which have 5 LVT entries), the
            /// value returned is 4; for the Pentium processor (which has 4 LVT
            /// entries), the value returned is 3. For processors based on the
            /// Nehalem microarchitecture (which has 7 LVT entries) and onward,
            /// the value returned is 6.
            pub const MAX_LVT: u8;
            /// Indicates whether software can inhibit the broadcast of EOI
            /// message by setting bit 12 of the  Spurious Interrupt Vector
            /// Register; see Section 12.8.5 and Section 12.9. in Ch. 7 of Vol.
            /// 3A of the Intel SDM for details.
            pub const EOI_SUPPRESSION: bool;
        }
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
        pub struct LvtEntry<u32> {
            pub const VECTOR: u8;
            const _RESERVED_0 = 2;
            pub const NMI: bool;
            pub const SEND_PENDING: bool;
            pub const POLARITY: PinPolarity;
            pub const REMOTE_IRR: bool;
            pub const TRIGGER: TriggerMode;
            pub const MASKED: bool;
        }
    }

    enum_from_bits! {
        #[derive(Debug, Eq, PartialEq)]
        pub enum TimerMode<u8> {
            /// One-shot mode, program count-down value in an initial-count register.
            OneShot = 0b00,
            /// Periodic mode, program interval value in an initial-count register.
            Periodic = 0b01,
            /// TSC-Deadline mode, program target value in IA32_TSC_DEADLINE MSR.
            TscDeadline = 0b10,
        }
    }

    enum_from_bits! {
        /// Configures the LVT time divisor.
        #[derive(Debug, Eq, PartialEq)]
        pub enum TimerDivisor<u32> {
            By1 = 0b1011,
            By2 = 0b0000,
            By4 = 0b0001,
            By8 = 0b0010,
            By16 = 0b0011,
            By32 = 0b1000,
            By64 = 0b1001,
            By128 = 0b1010,
        }
    }
    impl TimerDivisor {
        /// Returns the numeric value of the divisor configuration.
        pub(super) fn into_divisor(self) -> u32 {
            match self {
                TimerDivisor::By1 => 1,
                TimerDivisor::By2 => 2,
                TimerDivisor::By4 => 4,
                TimerDivisor::By8 => 8,
                TimerDivisor::By16 => 16,
                TimerDivisor::By32 => 32,
                TimerDivisor::By64 => 64,
                TimerDivisor::By128 => 128,
            }
        }
    }

    bitfield! {
        /// Value of the Error Status Register (ESR).
        ///
        /// See Intel SDM Vol. 3A, Ch. 7, Section 12.5.3, "Error Handling".
        #[derive(Default)]
        pub struct ErrorStatus<u32> {
            /// Set when the local APIC detects a checksum error for a message
            /// that it sent on the APIC bus.
            ///
            /// Used only on P6 family and Pentium processors.
            pub const SEND_CHECKSUM_ERROR: bool;

            /// Set when the local APIC detects a checksum error for a message
            /// that it received on the APIC bus.
            ///
            /// Used only on P6 family and Pentium processors.
            pub const RECV_CHECKSUM_ERROR: bool;

            /// Set when the local APIC detects that a message it sent was not
            /// accepted by any APIC on the APIC bus
            ///
            /// Used only on P6 family and Pentium processors.
            pub const SEND_ACCEPT_ERROR: bool;

            /// Set when the local APIC detects that a message it received was
            /// not accepted by any APIC on the APIC bus.
            ///
            /// Used only on P6 family and Pentium processors.
            pub const RECV_ACCEPT_ERROR: bool;

            /// Set when the local APIC detects an attempt to send an IPI with
            /// the lowest-priority delivery mode and the local APIC does not
            /// support the sending of such IPIs. This bit is used on some Intel
            /// Core and Intel Xeon processors.
            pub const REDIRECTABLE_IPI: bool;

            /// Set when the local APIC detects an illegal vector (one in the
            /// range 0 to 15) in the message that it is sending. This occurs as
            /// the result of a write to the ICR (in both xAPIC and x2APIC
            /// modes) or to SELF IPI register (x2APIC mode only) with an
            /// illegal vector.
            ///
            /// If the local APIC does not support the sending of
            /// lowest-priority IPIs and software writes the ICR to send a
            /// lowest-priority IPI with an illegal vector, the local APIC sets
            /// only the “redirectable IPI” error bit. The interrupt is not
            /// processed and hence the “Send Illegal Vector” bit is not set in
            /// the ESR.
            pub const SEND_ILLEGAL_VECTOR: bool;

            /// Set when the local APIC detects an illegal vector (one in the
            /// range 0 to 15) in an interrupt message it receives or in an
            /// interrupt generated locally from the local vector table or via a
            /// self IPI. Such interrupts are not delivered to the processor;
            /// the local APIC will never set an IRR bit in the range 0 to 15.
            pub const RECV_ILLEGAL_VECTOR: bool;

            /// Set when the local APIC is in xAPIC mode and software attempts
            /// to access a register that is reserved in the processor's
            /// local-APIC register-address space; see Table 10-1. (The
            /// local-APIC register-address spacemprises the 4 KBytes at the
            /// physical address specified in the `IA32_APIC_BASE` MSR.) Used only
            /// on Intel Core, Intel Atom, Pentium 4, Intel Xeon, and P6 family
            /// processors.
            ///
            /// In x2APIC mode, software accesses the APIC registers using the
            /// `RDMSR` and `WRMSR` instructions. Use of one of these
            /// instructions to access a reserved register cause a
            /// general-protection exception (see Section 10.12.1.3). They do
            /// not set the “Illegal Register Access” bit in the ESR.
            pub const ILLEGAL_REGISTER_ACCESS: bool;
        }

    }

    impl ErrorStatus {
        /// Returns `true` if an error is present, or `false` if no error
        /// bits are set.
        pub fn is_error(&self) -> bool {
            // Mask out the reserved bits, just in case they have values in the,
            // (they shouldn't, per the SDM, but...who knows!)
            self.bits() & 0b1111_1111 != 0
        }
    }
}

#[cfg(test)]
mod tests {
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
}
