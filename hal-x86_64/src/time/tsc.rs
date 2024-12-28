use crate::cpu::{intrinsics, FeatureNotSupported};
use core::{
    sync::atomic::{AtomicU32, Ordering::*},
    time::Duration,
    u64,
};
use maitake::time::Clock;
use raw_cpuid::CpuId;

#[derive(Copy, Clone, Debug)]
pub struct Rdtsc(());

#[derive(Clone, Debug)]
pub struct TscInitError(&'static str);

impl Rdtsc {
    pub fn is_supported() -> bool {
        CpuId::new()
            .get_feature_info()
            .map(|features| features.has_tsc())
            .unwrap_or(false)
    }

    pub fn new() -> Result<Self, FeatureNotSupported> {
        if Self::is_supported() {
            Ok(Self(()))
        } else {
            Err(FeatureNotSupported::new("rdtsc"))
        }
    }

    /// Reads the current value of the timestamp counter.
    #[inline(always)]
    #[must_use]
    pub fn read_timestamp(&self) -> u64 {
        unsafe {
            // safety: if an instance of this type was constructed, then we
            // checked that `rdtsc` is supported via `cpuid`.
            intrinsics::rdtsc()
        }
    }

    fn cpuid_frequency_hz() -> Result<u64, TscInitError> {
        let cpuid = CpuId::new();

        // First, let's check if we're in a VM...
        if let Some(info) = cpuid.get_hypervisor_info() {
            let hypervisor = info.identify();
            if let Some(freq_khz) = info.tsc_frequency() {
                tracing::info!(
                    ?hypervisor,
                    "hypervisor reports TSC frequency is {freq_khz} kHz"
                );
                return Ok(freq_khz as u64 * 1_000);
            } else {
                // Just keep going.
                tracing::info!(?hypervisor, "hypervisor does not report TSC frequency");
            }
        }

        // EDX[8] in CPUID leaf 0x8000_0007 (APM Info) is "TscInvariant", which
        // indicates that the TSC runs at a constant rate regasrdless of ACPI
        // P-, C-, and T-states.
        let invariant = cpuid
            .get_advanced_power_mgmt_info()
            .ok_or(TscInitError("missing CPUID leaf 0x8000_0007"))?
            .has_invariant_tsc();
        if !invariant {
            // If the TSC is not invariant, we won't use it as the maitake timer
            // clock source.
            return Err(TscInitError("TSC is not invariant"));
        }

        // The CPUID leaves and MSRs that we'll look at to determine the TSC
        // frequency depend on the CPU vendor.
        let vendor = cpuid
            .get_vendor_info()
            .ok_or(TscInitError("no CPUID vendor string"))?;
        let vendor = vendor.as_str();

        if vendor == "GenuineIntel" {
            // CPUID Leaf 0x15, Time Stamp Counter/Core Crystal Clock Information
            let tsc_info = cpuid
                .get_tsc_info()
                .ok_or(TscInitError("intel CPU missing CPUID leaf 0x15 TSC info"))?;
            // N.B.: `raw_cpuid`'s `TscInfo` struct has a `tsc_frequency()`
            // method that *basically* does this, but it doesn't know about
            // Denverton reporting a 0Hz crystal frequency, so we'll do it
            // ourselves to handle that.
            let crystal_hz = tsc_info.nominal_frequency() as u64;
            if crystal_hz == 0 {
                // TODO(eliza): Denverton SoCs will have a 0 here, so we should
                // check for those parts and set this to 25,000 kHz.
                // As seen in linux:
                // https://github.com/torvalds/linux/blob/fd0584d220fe285dc45be43eede55df89ad6a3d9/arch/x86/kernel/tsc.c#L681-L688
                return Err(TscInitError("intel CPU has unknown crystal frequency"));
            }

            let numerator = tsc_info.numerator() as u64;
            let denominator = tsc_info.denominator() as u64;
            if numerator == 0 || denominator == 0 {
                return Err(TscInitError("intel CPU has invalid TSC ratio"));
            }

            return Ok(crystal_hz * numerator / denominator);
        }

        if vendor == "AuthenticAMD" {
            return Err(TscInitError(
                "AMD TSC frequency detection not implemented yet",
            ));
        }

        Err(TscInitError(
            "CPU vendor is 'some other shit', get a real computer",
        ))
    }

    /// Returns a [`maitake::time::Clock`] defining a clock that uses `rdtsc`
    /// timestamps to produce `maitake` ticks.
    pub fn into_maitake_clock(self) -> Result<Clock, TscInitError> {
        // TODO(eliza): fix this
        #![allow(unreachable_code)]

        // First, see if we can figure it out from CPUID.
        match Self::cpuid_frequency_hz() {
            Ok(freq_hz) => {
                tracing::info!("determined TSC frequency via CPUID: {freq_hz} Hz");
            }
            Err(TscInitError(error)) => {
                tracing::warn!(%error, "could not determine TSC frequency from CPUID!");
            }
        };

        return Err(TscInitError(
            "calibration routine doesn't really work yet, sorry!",
        ));

        const NOT_YET_CALIBRATED: u32 = u32::MAX;

        // 50ms is stolen from Linux, so i'm assuming its a reasonable duration
        // for PIT-based calibration:
        // https://github.com/torvalds/linux/blob/4ae004a9bca8bef118c2b4e76ee31c7df4514f18/arch/x86/kernel/tsc.c#L688-L839
        const PIT_SLEEP_DURATION: Duration = Duration::from_millis(50);

        static MAITAKE_TICK_SHIFT: AtomicU32 = AtomicU32::new(NOT_YET_CALIBRATED);

        fn now() -> u64 {
            let rdtsc = unsafe { intrinsics::rdtsc() };
            let shift = MAITAKE_TICK_SHIFT.load(Relaxed);
            rdtsc >> shift
        }

        tracing::info!("calibrating RDTSC Maitake clock from PIT...");
        let mut pit = super::PIT.lock();

        let t0 = self.read_timestamp();
        pit.sleep_blocking(PIT_SLEEP_DURATION)
            .expect("PIT sleep failed for some reason");
        let t1 = self.read_timestamp();

        let elapsed_cycles = t1 - t0;
        tracing::debug!(t0, t1, elapsed_cycles, "slept for {PIT_SLEEP_DURATION:?}");

        let mut shift = 0;
        loop {
            assert!(
                shift < 64,
                "shifted away all the precision in the timestamp counter! \
                this definitely should never happen..."
            );
            let elapsed_ticks = elapsed_cycles >> shift;
            tracing::debug!(shift, elapsed_ticks, "trying a new tick shift");

            let elapsed_ticks: u32 = elapsed_ticks.try_into().expect(
                "there is no way that a 50ms sleep duration is more than \
                    u32::MAX rdtsc cycles...right?",
            );

            let tick_duration = PIT_SLEEP_DURATION / elapsed_ticks;
            if tick_duration.as_nanos() > 0 {
                // Ladies and gentlemen...we got him!
                tracing::info!(?tick_duration, shift, "calibrated RDTSC Maitake clock");
                MAITAKE_TICK_SHIFT
                    .compare_exchange(NOT_YET_CALIBRATED, shift, AcqRel, Acquire)
                    .map_err(|_| TscInitError("RDTSC Maitake clock has already been calibrated"))?;
                return Ok(Clock::new(tick_duration, now).named("CLOCK_RDTSC"));
            } else {
                shift += 1;
            }
        }
    }
}

impl core::fmt::Display for TscInitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}
