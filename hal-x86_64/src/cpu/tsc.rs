use super::{intrinsics, FeatureNotSupported};
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
    pub fn read_timestamp(&self) -> u64 {
        unsafe {
            // safety: if an instance of this type was constructed, then we
            // checked that `rdtsc` is supported via `cpuid`.
            intrinsics::rdtsc()
        }
    }
}
