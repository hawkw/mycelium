use super::{PinPolarity, TriggerMode};
use crate::{
    cpu::{FeatureNotSupported, Msr},
    mm,
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
    /// # Returns
    /// - [`Ok`]`(LocalApic)` if this CPU supports the APIC interrupt model.
    /// - [`Err`]`(`[`FeatureNotSupported`]`)` if this CPU does not support APIC
    ///   interrupt handling.
    pub fn try_new() -> Result<Self, FeatureNotSupported> {
        if !super::is_supported() {
            return Err(FeatureNotSupported::new("APIC interrupt model"));
        }

        let msr = Msr::ia32_apic_base();
        let base_paddr = PAddr::from_u64(msr.read() & Self::BASE_PADDR_MASK);
        let base = mm::kernel_vaddr_of(base_paddr);
        tracing::debug!(?base, "found local APIC base address");
        assert_ne!(base, VAddr::from_u64(0));

        Ok(Self { msr, base })
    }

    #[must_use]
    pub fn new() -> Self {
        Self::try_new().unwrap()
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

impl Default for LocalApic {
    fn default() -> Self {
        Self::new()
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
