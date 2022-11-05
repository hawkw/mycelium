#![warn(missing_docs)]
use core::{arch::asm, marker::PhantomData};
use mycelium_util::{bits::FromBits, fmt};
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
/// constructor is provided for accessing that MSR in a typed manner. Currently,
/// the following typed MSR constructors are abailable:
///
/// - [`Msr::ia32_apic_base`] for accessing the `IA32_APIC_BASE` MSR, which
///   stores the base address of the local APIC's memory-mapped configuration area.
///
/// [sandpile]: https://sandpile.org/x86/msr.htm
/// [msr]: https://wiki.osdev.org/MSR
pub struct Msr<V = u64> {
    num: u32,
    name: Option<&'static str>,
    _ty: PhantomData<fn(V)>,
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

    /// Returns a `Msr` for reading and writing to the `IA32_APIC_BASE`
    /// model-specific register.
    ///
    /// This register stores the base address of the local APIC memory-mapped
    /// configuration area.
    #[must_use]
    pub fn ia32_apic_base() -> Self {
        Self {
            name: Some("IA32_APIC_BASE"),
            ..Self::new(0x1b)
        }
    }

    /// Returns a `Msr` for reading and writing to the `IA32_GS_BASE`
    /// model-specific register.
    #[must_use]
    pub fn ia32_gs_base() -> Self {
        Self {
            name: Some("IA32_GS_BASE"),
            ..Self::new(0xc000_0101)
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
