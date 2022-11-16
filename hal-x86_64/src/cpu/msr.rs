//! x86_64 [Model-Specific Register][msr] (MSR)s.
//!
//! Model-specific registers are used to configure features of the CPU that may
//! not be available on all x86 processors, such as memory type-range,
//! sysenter/sysexit, local APIC, et cetera.
//!
//! This module contains the [`Msr`] type for accessing model-specific
//! registers. In addition, since most MSRs contain bitflags, this module also
//! contains bitflags types defining the flags that can be set in a particular
//! MSR.
//!
//! See the documentation for the [`Msr`] type for details on using MSRs.
//!
//! [msr]: https://wiki.osdev.org/MSR
#![warn(missing_docs)]
use core::{arch::asm, marker::PhantomData};
use mycelium_util::{
    bits::{bitfield, FromBits},
    fmt,
};
use raw_cpuid::CpuId;

/// An x86_64 [Model-Specific Register][msr] (MSR).
///
/// Model-specific registers are used to configure features of the CPU that may
/// not be available on all x86 processors, such as memory type-range,
/// sysenter/sysexit, local APIC, et cetera. MSRs are available on P6 and later
/// x86 processors (and are present on all 64-bit x86 CPUs). The
/// [`Msr::has_msrs`] method can be used to check if the CPU has MSRs. Note
/// that this method does *not* check whether a particular MSR is available.
///
/// See [sandpile.org's MSR list][sandpile] for a list of documented MSRs and
/// their values.
///
/// # Typed MSRs
///
/// MSRs may be accessed as raw `u64` values (using [`Msr::read_raw`] and
/// [`Msr::write_raw`]), or may be constructed with a type parameter
/// implementing the [`mycelium_util::bits::FromBits`] trait, which is
/// automatically converted to and from its binary representation when
/// reading/writing to the MSR.
///
/// When a typed representation of a MSR's value is available, a special
/// constructor is provided for accessing that MSR in a typed manner.
///
/// # MSR Constructors
///
/// This type provides a number of constructors which construct a [`Msr`] for
/// accessing a specific model-specific register by name. The following
/// constructors are currently provided:
///
/// - [`Msr::ia32_apic_base`] for accessing the [`IA32_APIC_BASE`] MSR, which
///   stores the base address of the local APIC's memory-mapped configuration
///   area.
///
/// - [`Msr::ia32_gs_base`] for accessing the [`IA32_GS_BASE`] MSR, which stores
///   the base address of the `GS` segment.
///
/// - [`Msr::ia32_efer`] for accessing the [Extended Flags Enable Register
///   (EFER)][efer], which contains flags for enabling long mode and controlling
///   long-mode-specific features.
///
///   Flags for the `IA32_EFER` MSR are represented by the [`Efer`] type.
///
/// [sandpile]: https://sandpile.org/x86/msr.htm
/// [msr]: https://wiki.osdev.org/MSR
/// [efer]: https://wiki.osdev.org/CPU_Registers_x86-64#IA32_EFER
/// [`IA32_APIC_BASE`]: https://wiki.osdev.org/APIC#Local_APIC_configuration
/// [`IA32_GS_BASE`]: https://wiki.osdev.org/CPU_Registers_x86-64#FS.base.2C_GS.base
pub struct Msr<V = u64> {
    pub(crate) num: u32,
    name: Option<&'static str>,
    _ty: PhantomData<fn(V)>,
}

bitfield! {
    /// Bit flags for the [Extended Feature Enable Register (EFER)][efer] [`Msr`].
    ///
    /// This MSR was added by AMD in the K6 processor, and became part of the
    /// architecture in AMD64. It controls features related to entering long mode.
    ///
    /// To access the EFER, use the [`Msr::ia32_efer`] constructor.
    ///
    /// [efer]: https://wiki.osdev.org/CPU_Registers_x86-64#IA32_EFER
    pub struct Efer<u64> {
        /// System Call Extensions (SCE).
        ///
        /// This enables the `SYSCALL` and `SYSRET` instructions.
        pub const SYSTEM_CALL_EXTENSIONS: bool;
        const _RESERVED_0 = 7;
        /// Long Mode Enable (LME).
        ///
        /// Setting this bit enables long mode.
        pub const LONG_MODE_ENABLE: bool;
        const _RESERVED_1 = 1;
        /// Long Mode Active (LMA).
        ///
        /// This bit is set if the processor is in long mode.
        pub const LONG_MODE_ACTIVE: bool;
        /// No-Execute Enable (NXE).
        pub const NO_EXECUTE_ENABLE: bool;
        /// Secure Virtual Machine Enable (SVME).
        pub const SECURE_VM_ENABLE: bool;
        /// Long Mode Segment Limit Enable (LMSLE).
        pub const LONG_MODE_SEGMENT_LIMIT_ENABLE: bool;
        /// Fast `FXSAVE`/`FXRSTOR` (FFXSR).
        pub const FAST_FXSAVE_FXRSTOR: bool;
        /// Translation Cache Extension (TCE).
        pub const TRANSLATION_CACHE_EXTENSION: bool;
    }
}

impl Msr {
    /// Returns `true` if this processor has MSRs.
    ///
    /// # Notes
    ///
    /// This does *not* check whether the given MSR number is valid on this platform.
    #[must_use]
    pub fn has_msrs() -> bool {
        CpuId::new()
            .get_feature_info()
            .map(|features| features.has_msr())
            .unwrap_or(false)
    }

    /// Returns a new `Msr` for reading/writing to the given MSR number, or
    /// `None` if this CPU does not support MSRs.
    ///
    /// # Notes
    ///
    /// This does *not* check whether the given MSR number is valid on this platform.
    #[inline]
    #[must_use]
    pub fn try_new(num: u32) -> Option<Self> {
        if Self::has_msrs() {
            Some(Msr {
                num,
                name: None,
                _ty: PhantomData,
            })
        } else {
            None
        }
    }

    /// Returns a new `Msr` for reading/writing to the given MSR number.
    ///
    /// # Panics
    ///
    /// If this CPU does not support MSRs.
    ///
    /// # Notes
    ///
    /// This does *not* check whether the given MSR number is valid on this platform.
    pub fn new(num: u32) -> Self {
        Self::try_new(num)
            .expect("CPU does not support model-specific registers (must be pre-Pentium...)")
    }

    /// Returns a `Msr` for reading and writing to the [`IA32_APIC_BASE`]
    /// model-specific register.
    ///
    /// This register has MSR number 0x1B, and stores the base address of the
    /// [local APIC] memory-mapped configuration area.
    ///
    /// [`IA32_APIC_BASE`]: https://wiki.osdev.org/APIC#Local_APIC_configuration
    /// [local APIC]: crate::interrupt::apic::LocalApic
    #[must_use]
    pub const fn ia32_apic_base() -> Self {
        Self {
            name: Some("IA32_APIC_BASE"),
            num: 0x1b,
            _ty: PhantomData,
        }
    }

    /// Returns a `Msr` for reading and writing to the [`IA32_GS_BASE`]
    /// model-specific register.
    ///
    /// This register has MSR number 0xC0000101, and contains the base address
    /// of the `GS` segment.
    ///
    /// [`IA32_GS_BASE`]: https://wiki.osdev.org/CPU_Registers_x86-64#FS.base.2C_GS.base
    #[must_use]
    pub const fn ia32_gs_base() -> Self {
        Self {
            name: Some("IA32_GS_BASE"),
            num: 0xc000_0101,
            _ty: PhantomData,
        }
    }

    /// Returns a `Msr` for reading and writing to the [`IA32_EFER` (Extended
    /// Flags Enable Register)][efer] MSR.
    ///
    /// The EFER register has MSR number 0xC0000080, and contains flags for
    /// enabling the `SYSCALL` and `SYSRET` instructions, and for entering and
    /// exiting long mode, and for enabling features related to long mode.
    ///
    /// Flags for the `IA32_EFER` MSR are represented by the [`Efer`]
    /// type.
    ///
    /// [efer]: https://wiki.osdev.org/CPU_Registers_x86-64#IA32_EFER
    #[must_use]
    pub const fn ia32_efer() -> Msr<Efer> {
        Msr {
            name: Some("IA32_EFER"),
            num: 0xc0000080,
            _ty: PhantomData,
        }
    }
}

impl<V: FromBits<u64>> Msr<V> {
    /// Attempt to read a `V`-typed value from the MSR, returning an error if
    /// that value is an invalid bit pattern for a `V`-typed value.
    ///
    /// # Returns
    ///
    /// - [`Ok`]`(V`)` if a `V`-typed value was successfully read from the MSR.
    /// - [`Err`]`(V::Error)` if the value read from the MSR was an invalid bit
    ///   pattern for a `V`, as determined by `V`'s implementation of the
    ///   [`FromBits::try_from_bits`]) method.
    pub fn try_read(self) -> Result<V, V::Error> {
        V::try_from_bits(self.read_raw())
    }

    /// Read a `V`-typed value from the MSR.
    ///
    /// # Panics
    ///
    /// If the bits in the MSR are an invalid bit pattern for a `V`-typed value
    /// (as determined by `V`'s implementation of the
    /// [`FromBits::try_from_bits`]) method).
    #[must_use]
    pub fn read(self) -> V {
        match self.try_read() {
            Ok(value) => value,
            Err(error) => panic!("invalid value for {}: {}", self, error),
        }
    }

    /// Write a value to this MSR.
    ///
    /// # Safety
    ///
    /// The caller is responsible for ensuring that writing the provided value
    /// to this MSR doesn't violate memory safety.
    pub unsafe fn write(self, value: V) {
        self.write_raw(value.into_bits());
    }

    /// Read this MSR's current value, modify it using a closure, and write back
    /// the modified value.
    ///
    /// This is a convenience method for cases where some bits in a MSR should
    /// be changed while leaving other values in place.
    ///
    /// # Safety
    ///
    /// The caller is responsible for ensuring that writing the provided value
    /// to this MSR doesn't violate memory safety.
    pub unsafe fn update(self, f: impl FnOnce(V) -> V) {
        self.write(f(self.read()));
    }
}

impl<V> Msr<V> {
    /// Reads this MSR, returning the raw `u64` value.
    #[inline]
    #[must_use]
    pub fn read_raw(self) -> u64 {
        let (hi, lo): (u32, u32);
        unsafe {
            asm!(
                "rdmsr",
                in("ecx") self.num,
                out("eax") lo,
                out("edx") hi,
                options(nomem, nostack, preserves_flags)
            );
        }
        let result = (hi as u64) << 32 | (lo as u64);
        tracing::trace!(rdmsr = %self, value = fmt::hex(result));
        result
    }

    /// Writes the given raw `u64` value to this MSR.
    ///
    /// # Safety
    ///
    /// The caller is responsible for ensuring that writing the provided value
    /// to this MSR doesn't violate memory safety.
    pub unsafe fn write_raw(self, value: u64) {
        tracing::trace!(wrmsr = %self, value = fmt::hex(value));
        let lo = value as u32;
        let hi = (value >> 32) as u32;
        asm!(
            "wrmsr",
            in("ecx") self.num,
            in("eax") lo,
            in("edx") hi,
            options(nostack, preserves_flags)
        );
    }
}

impl<V> Clone for Msr<V> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            num: self.num,
            name: self.name,
            _ty: PhantomData,
        }
    }
}

impl<V> Copy for Msr<V> {}

impl<V> PartialEq for Msr<V> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.num == other.num
    }
}

impl<V> Eq for Msr<V> {}

impl<V> fmt::Debug for Msr<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { num, name, .. } = self;
        match name {
            Some(name) => write!(f, "Msr({num:#x}, {name})"),
            None => write!(f, "Msr({num:#x})"),
        }
    }
}

impl<V> fmt::Display for Msr<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { num, name, .. } = self;
        match name {
            Some(name) => write!(f, "MSR {num:#x} ({name})"),
            None => write!(f, "MSR {num:#x})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn efer_valid() {
        Efer::assert_valid();
        println!("{}", Efer::new())
    }

    #[test]
    fn efer_bitoffsets() {
        assert_eq!(
            Efer::LONG_MODE_ENABLE.least_significant_index(),
            8,
            "LONG_MODE_ENABLE LSB",
        );
        assert_eq!(
            Efer::TRANSLATION_CACHE_EXTENSION.least_significant_index(),
            15,
            "TCE LSB"
        );
    }
}
