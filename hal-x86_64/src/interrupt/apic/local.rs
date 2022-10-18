use crate::{cpu::Msr, mm};
use core::{convert::TryInto, marker::PhantomData, ops::Deref, time::Duration};
use hal_core::{PAddr, VAddr};
use mycelium_util::fmt;
use volatile::{access, Volatile};

pub struct LocalApic {
    msr: Msr,
    base: VAddr,
}

/// Represents a register in the local APIC's configuration area.
#[derive(Debug)]
pub struct LocalApicRegister<T = u32, A = access::ReadWrite> {
    offset: usize,
    _ty: PhantomData<fn(T, A)>,
}

/// Configures the local APIC timer.
#[derive(Copy, Clone, Debug)]
pub struct TimerConfig {}

pub trait RegisterAccess {
    type Access;
    type Target;
    fn volatile(
        ptr: &'static mut Self::Target,
    ) -> Volatile<&'static mut Self::Target, Self::Access>;
}

impl LocalApic {
    const BASE_PADDR_MASK: u64 = 0xffff_ffff_f000;

    /// Try to construct a `LocalApic`.
    ///
    /// # Returns
    /// - `Some(LocalApic)` if this CPU supports the APIC interrupt model.
    /// - `None` if this CPU does not support APIC interrupt handling.
    #[must_use]
    pub fn try_new() -> Option<Self> {
        if !super::is_supported() {
            return None;
        }

        let msr = Msr::ia32_apic_base();
        let base_paddr = PAddr::from_u64(msr.read() & Self::BASE_PADDR_MASK);
        let base = mm::kernel_vaddr_of(base_paddr);
        tracing::debug!(?base, "LocalApic::new");

        Some(Self { msr, base })
    }

    #[must_use]
    pub fn new() -> Self {
        Self::try_new().expect("CPU does not support APIC interrupt model!")
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

    pub fn start_periodic_timer(&self, interval: Duration, apic_frequency_hz: u32, vector: u8) {
        // divisor for the APIC timer.
        //
        // it would be nicer if we could set this to 1, but apparently some
        // platforms "don't like" that...
        const DIVISOR: u32 = 16;

        let ticks_per_ms = apic_frequency_hz / 1000 / DIVISOR;
        tracing::trace!(
            ?interval,
            apic_frequency_hz,
            vector,
            ticks_per_ms,
            "starting local APIC timer"
        );
        let interval_ms: u32 = interval
            .as_millis()
            .try_into()
            .expect("requested interval exceeds u32!");
        let ticks_per_interval = interval_ms
            .checked_mul(ticks_per_ms)
            .expect("requested interval exceeds u32");

        unsafe {
            let lvt_entry = register::LvtTimer::new()
                .with(register::LvtTimer::VECTOR, vector)
                .with(register::LvtTimer::MODE, register::TimerMode::Periodic);
            // set the divisor to 16.
            self.register(register::TIMER_DIVISOR).write(0b11);
            self.register(register::LVT_TIMER).write(lvt_entry);
            self.register(register::TIMER_INITIAL_COUNT)
                .write(ticks_per_interval);
        }

        tracing::info!(
            ?interval,
            apic_frequency_hz,
            ticks_per_ms,
            vector,
            "started local APIC timer"
        );
    }

    /// Returns the local APIC's base physical address.
    fn base_paddr(&self) -> PAddr {
        let raw = self.msr.read();
        PAddr::from_u64(raw & Self::BASE_PADDR_MASK)
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
        LVT_THERMAL = 0x330, ReadWrite;
        LVT_PERF = 0x340, ReadWrite;
        LVT_LINT0 = 0x350, ReadWrite;
        LVT_LINT1 = 0x360, ReadWrite;
        LVT_ERROR = 0x370, ReadWrite;

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
            pub const MODE: TimerMode;
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
