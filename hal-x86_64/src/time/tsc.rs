use crate::cpu::{intrinsics, FeatureNotSupported};
use core::{
    sync::atomic::{AtomicU32, Ordering::*},
    time::Duration,
};
use maitake::time::Clock;
use raw_cpuid::CpuId;

#[derive(Copy, Clone, Debug)]
pub struct Rdtsc(());

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

    /// Returns a [`maitake::time::Clock`] defining a clock that uses `rdtsc`
    /// timestamps to produce `maitake` ticks.
    pub fn into_maitake_clock(self) -> Result<Clock, &'static str> {
        // TODO(eliza): fix this
        #![allow(unreachable_code)]
        return Err("calibration routine doesn't really work yet, sorry!");

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
                    .map_err(|_| "RDTSC Maitake clock has already been calibrated")?;
                return Ok(Clock::new(tick_duration, now).named("CLOCK_RDTSC"));
            } else {
                shift += 1;
            }
        }
    }
}
