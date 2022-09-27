use super::{intrinsics, FeatureNotSupported, Port};
use core::sync::atomic::{AtomicU16, Ordering};
use mycelium_util::sync::spin;
use raw_cpuid::CpuId;

#[cfg(feature = "rand_core")]
use super::Rdtsc;
#[cfg(feature = "rand_core")]
use mycelium_util::sync::Lazy;
#[cfg(feature = "rand_core")]
use rand_core::{RngCore, SeedableRng};

/// Construct a new [`SeedableRng`] instance from all available entropy sources.
///
/// This function constructs a new seed value for an `R`-typed [`SeedableRng`]
/// using entropy from the following sources:
///
/// - [`Rdrand`], if it is available on this CPU
/// - [`Rdtsc`], if timestamp counters are available on this CPU
/// - [PIT channels 1, 2, and 3](PitEntropy)
///
#[cfg(feature = "rand_core")]
pub fn seed_rng<R: SeedableRng>() -> R {
    let mut seed = R::Seed::default();
    static RDRAND: Lazy<Option<Rdrand>> = Lazy::new(|| Rdrand::new().ok());
    static RDTSC: Lazy<Option<Rdtsc>> = Lazy::new(|| Rdtsc::new().ok());

    for byte in seed.as_mut() {
        // PIT entropy is always available. When RDRAND and RDTSC are available,
        // we'll use the PIT entropy to determine which byte of the RDRAND and
        // RDTSC values to use, as well.
        let pit = {
            let mut dst = [0u8];
            PitEntropy::read_bytes(&mut dst[..]);
            dst[0]
        };

        // If the CPU supports RDRAND, read a random value using that
        // instruction. This provides high-quality entropy.
        //
        // RDRAND returns either a 16, 32, or 64-bit value. Since we only need a
        // byte currently, we'll use the PIT entropy to determine which byte of
        // the RDRAND value to XOR with the current byte.
        if let Some(rdrand) = RDRAND.as_ref() {
            *byte ^= rdrand.next_u32().to_ne_bytes()[pit as usize % 4];
        }

        // If the CPU supports RDTSC, read from the timestamp counter as an
        // additional entropy source. This isn't a very good source of entropy,
        // since it's a counter, but XOR-ing additional entropy is always
        // better.
        //
        // Also, since we select which byte of the timestamp to use based on the
        // PIT timestamp, it's at least a little bit fucked up, which helps.
        if let Some(rdtsc) = RDTSC.as_ref() {
            *byte ^= rdtsc.read_timestamp().to_ne_bytes()[pit as usize % 8];
        }

        // Finally, XOR the current byte with the PIT timestamp. This is always
        // available. A seed generated only from PIT timestamps isn't a great
        // source of entropy, but if the CPU doesn't support RDRAND or RDTSC,
        // it's better than returning all zeros.
        *byte ^= pit;
    }

    R::from_seed(seed)
}

#[derive(Debug, Copy, Clone)]
pub struct Rdrand(());

#[derive(Debug, Copy, Clone)]
pub struct PitEntropy(());

// === impl Rdrand ===

impl Rdrand {
    pub const MAX_RETRIES: usize = 64;

    /// Returns `true` if the CPU supports the `rdrand` instruction.
    ///
    /// This is determined using the [`cpuid`](intrinsics::cpuid) instruction.
    pub fn is_supported() -> bool {
        CpuId::new()
            .get_feature_info()
            .map(|features| features.has_rdrand())
            .unwrap_or(false)
    }

    pub fn new() -> Result<Self, FeatureNotSupported> {
        if Self::is_supported() {
            Ok(Self(()))
        } else {
            Err(FeatureNotSupported::new("rdrand"))
        }
    }

    pub fn try_next_u32(&self) -> Option<u32> {
        let mut res: u32 = 0;
        unsafe {
            match intrinsics::rdrand32_step(&mut res) {
                1 => Some(res),
                0 => None,
                x => unreachable!("rdrand32_step should only return 1 or 0, but got {}", x),
            }
        }
    }

    pub fn next_u32(&self) -> u32 {
        self.with_retry(Self::try_next_u32)
    }

    pub fn try_next_u64(&self) -> Option<u64> {
        let mut res: u64 = 0;
        unsafe {
            match intrinsics::rdrand64_step(&mut res) {
                1 => Some(res),
                0 => None,
                x => unreachable!("rdrand64_step should only return 1 or 0, but got {}", x),
            }
        }
    }

    pub fn next_u64(&self) -> u64 {
        self.with_retry(Self::try_next_u64)
    }

    /// Retry a rdrand instruction until it succeeds, spinning with an
    /// exponential backoff if entropy is unavailable.
    fn with_retry<T: Default>(&self, f: impl Fn(&Self) -> Option<T>) -> T {
        let mut backoff = spin::Backoff::new();

        // Eventually, we do need to bail, since we don't want to be stuck in an
        // infinite loop...
        for _ in 0..Self::MAX_RETRIES {
            if let Some(res) = f(self) {
                return res;
            }
            backoff.spin();
        }
        T::default()
    }
}

#[cfg(feature = "rand_core")]
impl RngCore for Rdrand {
    fn next_u32(&mut self) -> u32 {
        Rdrand::next_u32(&*self)
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        Rdrand::next_u64(&*self)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        rand_core::impls::fill_bytes_via_next(self, dest)
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        use core::num::NonZeroU32;
        let mut left = dest;
        while left.len() >= 8 {
            let (l, r) = { left }.split_at_mut(8);
            left = r;

            let chunk: [u8; 8] = self
                .try_next_u64()
                .ok_or_else(|| rand_core::Error::from(NonZeroU32::new(1).expect("1 is non-zero")))?
                .to_ne_bytes();
            l.copy_from_slice(&chunk);
        }
        let n = left.len();
        if n > 4 {
            let chunk: [u8; 8] = self
                .try_next_u64()
                .ok_or_else(|| rand_core::Error::from(NonZeroU32::new(1).expect("1 is non-zero")))?
                .to_ne_bytes();
            left.copy_from_slice(&chunk[..n]);
        } else if n > 0 {
            let chunk: [u8; 4] = self
                .try_next_u32()
                .ok_or_else(|| rand_core::Error::from(NonZeroU32::new(1).expect("1 is non-zero")))?
                .to_ne_bytes();
            left.copy_from_slice(&chunk[..n]);
        }

        Ok(())
    }
}

#[cfg(feature = "rand_core")]
impl RngCore for Rdtsc {
    fn next_u32(&mut self) -> u32 {
        self.read_timestamp() as u32
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        self.read_timestamp()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        rand_core::impls::fill_bytes_via_next(self, dest)
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl PitEntropy {
    pub const fn new() -> Self {
        Self(())
    }

    pub fn next_u32() -> u32 {
        let mut bytes = [0u8; 4];
        Self::read_bytes(&mut bytes[..]);
        u32::from_be_bytes(bytes)
    }

    pub fn next_u64() -> u64 {
        let mut bytes = [0u8; 8];
        Self::read_bytes(&mut bytes[..]);
        u64::from_be_bytes(bytes)
    }

    fn read_bytes(dest: &mut [u8]) {
        const PIT_BASE: u16 = 0x40;

        // making this global state probably *increases* the amount of entropy,
        // since the next channel to read from will depend on other uses of the
        // entropy source in an unpredictable way...
        static NEXT_CHANNEL: AtomicU16 = AtomicU16::new(0);

        for byte in dest {
            let channel = NEXT_CHANNEL.fetch_add(1, Ordering::Relaxed) % 3;
            *byte = unsafe { Port::at(PIT_BASE + channel).readb() };
        }
    }
}

#[cfg(feature = "rand_core")]
impl RngCore for PitEntropy {
    #[inline]
    fn next_u32(&mut self) -> u32 {
        Self::next_u32()
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        Self::next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        Self::read_bytes(dest);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        Self::read_bytes(dest);
        Ok(())
    }
}
