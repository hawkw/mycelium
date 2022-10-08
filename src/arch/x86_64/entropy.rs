//! Hardware entropy sources.
use hal_x86_64::cpu::entropy;
use mycelium_util::sync::Lazy;

/// This is a wrapper type so that we don't globally implement `rand::CryptoRng`
/// for the `hal_core::cpu::entropy::PitEntropy` type, which is...Not a
/// cryptographically secure RNG...
///
/// # Notes
///
/// Use this RNG **only** for calls to `TranscriptRngBuilder::finalize`! It is
/// *not* actually cryptographically secure!
pub(crate) struct RekeyRng(entropy::PitEntropy);

pub(crate) fn fill_bytes(buf: &mut [u8]) -> usize {
    static RDRAND: Lazy<Option<entropy::Rdrand>> = Lazy::new(|| entropy::Rdrand::new().ok());
    // static RDTSC: Lazy<Option<hal_core::cpu::Rdtsc>> = Lazy::new(|| hal_core::cpu::Rdtsc::new().ok());
    let total_len = buf.len();

    if let Some(rdrand) = &*RDRAND {
        fill_bytes_rdrand(rdrand, buf);
        let bytes = total_len - buf.len();
        tracing::trace!(bytes, "generated entropy from RDRAND");
        bytes
    } else {
        todo!("eliza: use other entropy sources?")
    }
}

fn fill_bytes_rdrand(rdrand: &entropy::Rdrand, mut buf: &mut [u8]) {
    while buf.len() >= 8 {
        let random = match rdrand.try_next_u64() {
            Some(random) => random,
            None => return,
        };
        let (chunk, rest) = { buf }.split_at_mut(8);
        buf = rest;

        chunk.copy_from_slice(&random.to_ne_bytes());
    }

    let remaining = buf.len();
    if remaining > 4 {
        let random = match rdrand.try_next_u64() {
            Some(random) => random,
            None => return,
        };

        buf.copy_from_slice(&random.to_ne_bytes()[..remaining]);
    } else {
        let random = match rdrand.try_next_u32() {
            Some(random) => random,
            None => return,
        };

        buf.copy_from_slice(&random.to_ne_bytes()[..remaining]);
    }
}

// === impl RekeyRng ===

impl rand::RngCore for RekeyRng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest)
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.0.try_fill_bytes(dest)
    }
}

/// This impl is necessary so that we can use this RNG for
/// `TranscriptRng::finalize`, since the `merlin` API requires that RNG be a
/// `CryptoRng`. This RNG is **not actually cryptographically secure**, but this
/// is fine, since we are not actually proving a transcript, just using the
/// transcript for its internal `#![no_std]`-compatible STROBE implementation.
impl rand::CryptoRng for RekeyRng {}

impl RekeyRng {
    pub(crate) fn new() -> Self {
        Self(entropy::PitEntropy::new())
    }
}
