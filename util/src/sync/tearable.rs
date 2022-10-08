use crate::{
    fmt,
    sync::{
        atomic::{AtomicU32, Ordering::*},
        hint,
    },
};

/// An efficient shareable 64-bit integer value, based on a [sequence
/// lock].
///
/// This type is intended for cases where an atomic 64-bit value is
/// needed on platforms that do not support atomic operations on 64-bit
/// memory locations.
///
/// In addition, a [polyfill type](AtomicU64Polyfill), which uses 64-bit atomic
/// operations on platforms that support them, and tearable 32-bit atomic
/// operations otherwise, is also provided. This type can be used to provide a
/// more efficient implementation when 64-bit atomics are available, while still
/// exposing the same API when they are not.
///
/// # Implementation Details
///
/// A [sequence lock] is a form of reader-writer lock which works by
/// allowing writers to load the locked data at any time, using a
/// sequence number which is incremented both before and after a write
/// operation to determine if a given read of the locked data has
/// observed a torn write. In essence, the sequence lock deliberately
/// performs a potentially racy read, but only *uses* the result of the
/// read if it did not observe a torn write.
///
/// In Rust, sequence locks for arbitrary data cannot currently be
/// implemented soundly, as an unsynchronized (non-atomic) read of a
/// memory location during a write to the same location isu ndefined
/// behavior, even if the *result* of such a read is not actually
/// observed. However, this type is *not* unsound (and in fact does not
/// involve any unsafe code), as it stores a pair of [`AtomicU32`]
/// values, rather than arbitrary data. Therefore, reading the data is
/// always a pair of atomic operations, and potential tearing occurs
/// only between loading the two atomic values. This is potentially
/// racy, but it is not a *data race*, so the seqeunce lock
/// implementation is sound *when specialized specifically for atomic
/// integers*.
///
/// [sequence lock]: https://en.wikipedia.org/wiki/Seqlock
pub struct TearableU64 {
    seq: AtomicU32,
    high: AtomicU32,
    low: AtomicU32,
}

pub use polyfill::AtomicU64Polyfill;

/// Error returned by [`TearableU64::try_store`] that indicates another write
/// operation is in progress.
///
/// This error contains the value that we were attempting to write, which can be
/// retrieved using [`WriteInProgress::into_inner`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteInProgress(u64);

impl TearableU64 {
    loom_const_fn! {
        /// Returns a new `TearableU64` with the provided current `value`.
        #[must_use]
        pub(crate) fn new(value: u64) -> Self {
            let (high, low) = split(value);
            Self {
                seq: AtomicU32::new(0),
                high: AtomicU32::new(high),
                low: AtomicU32::new(low),
            }
        }
    }

    /// Loads the current value.
    ///
    /// This method may spin if a write is in progress or a torn write is
    /// observed.
    #[must_use]
    pub fn load(&self) -> u64 {
        loop {
            // snapshot the sequence number before reading the value, waiting
            // for a write to complete if one is in progress.
            let preread = self.seq.load(Acquire);

            // wait for a current write to complete
            if !is_writing(test_dbg!(preread)) {
                // read the number
                let low = self.low.load(Acquire);
                let high = self.high.load(Acquire);

                // snapshot the sequence number again after the value has been read.
                let postread = self.seq.load(Acquire);

                // if the sequence numbers match, we didn't observe a torn write.
                // return it!
                if test_dbg!(preread) == test_dbg!(postread) {
                    return unsplit(high, low);
                }
            }

            // in the case we *did* observe a torn write, we only issue one spin
            // loop hint, rather than backing off, because if we try another
            // read, we should get something reasonable (unless it's being
            // written to again).
            hint::spin_loop()
        }
    }

    /// Store `value` in this `TearableU64`.
    ///
    /// This method will spin if another write operation ([`store`], [`swap`],
    /// or [`try_store`]) is concurrently in progress, and it will spin until
    /// that write operation completes. Therefore, this method is **not** safe
    /// for use in an interrupt handler if writes to a `TearableU64` may occur
    /// outside of that interrupt handler. For use in an interrupt handler,
    /// consider the fallible [`try_store`] method instead.
    ///
    /// [`store`]: Self::store
    /// [`swap`]: Self::swap
    /// [`try_store`]: Self::try_store
    pub fn store(&self, value: u64) {
        let mut curr = self.seq.load(Relaxed);
        loop {
            // is a write in progress?
            if is_writing(test_dbg!(curr)) {
                hint::spin_loop();
                curr = self.seq.load(Relaxed);
                continue;
            }

            match self.write(curr, value) {
                // write succeeded!
                Ok(_) => return,
                // no joy, try again.
                Err(actual) => curr = actual,
            }
        }
    }

    /// Attempt to store `value` in this `TearableU64`, failing if another write
    /// operation ([`store`], [`swap`], or [`try_store`]) is concurrently in
    /// progress.
    ///
    /// This method will not spin if a write is in progress, but may retry a
    /// failed `compare_exchange_weak`. It is safe to call this method in an
    /// interrupt handler, however, as it will not spin indefinitely if a write
    /// operation is interrupted.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the value was successfully stored.
    /// - `Err(`[`WriteInProgress`]`)` if another write is in progress
    ///
    /// [`store`]: Self::store
    /// [`swap`]: Self::swap
    /// [`try_store`]: Self::try_store
    pub fn try_store(&self, value: u64) -> Result<(), u64> {
        let mut curr = self.seq.load(Relaxed);
        loop {
            // is a write in progress?
            if is_writing(test_dbg!(curr)) {
                return Err(value);
            }

            match self.write(curr, value) {
                // write succeeded!
                Ok(_) => return Ok(()),
                // no joy, try again.
                Err(actual) => curr = actual,
            }
        }
    }

    /// Store a new value, returning the previous one.
    ///
    /// This method may spin if another write is in progress.
    #[must_use = "if the return value of `TearableU64::swap` is not used, consider using `TearableU64::store` instead"]
    pub fn swap(&self, value: u64) -> u64 {
        let mut curr = self.seq.load(Relaxed);
        loop {
            // is a write in progress?
            if is_writing(test_dbg!(curr)) {
                hint::spin_loop();
                curr = self.seq.load(Relaxed);
                continue;
            }

            // try to start a write
            if let Err(actual) = self.try_start_write(curr) {
                curr = actual;
                hint::spin_loop();
                continue;
            }

            // write started!
            let (high, low) = split(value);
            let prev_low = self.low.swap(low, AcqRel);
            let prev_high = self.high.swap(high, AcqRel);

            self.finish_write(curr);
            return unsplit(prev_high, prev_low);
        }
    }

    // TODO(eliza): add a fetch_add for 64-bit vals...
    pub fn fetch_add_u32(&self, value: u32) -> u64 {
        let mut curr = self.seq.load(Relaxed);
        loop {
            // is a write in progress?
            if is_writing(test_dbg!(curr)) {
                hint::spin_loop();
                curr = self.seq.load(Relaxed);
                continue;
            }

            if let Err(actual) = self.try_start_write(curr) {
                curr = actual;
                hint::spin_loop();
                continue;
            }

            let prev_low = self.low.fetch_add(value, AcqRel);
            let overflow = ((prev_low as u64 + value as u64) >> 32) as u32;
            let prev_high = if overflow > 0 {
                let prev_high = self.high.fetch_add(overflow, AcqRel);
                // did adding to the high half wrap?
                if prev_high as u64 + overflow as u64 > u32::MAX as u64 {
                    // NOTE(eliza): this doesn't *need* to be a swap, but it's
                    // nice to be able to check that nobody else is writing...
                    let _low = self.low.swap(prev_high.wrapping_add(overflow), AcqRel);
                    debug_assert_eq!(
                        _low,
                        prev_low.wrapping_add(value),
                        "fetch_add_u32: low value should not be modified \
                        concurrently, since we haven't incremented the seq \
                        number to finish writing yet. this is a bug!"
                    );
                }
                prev_high
            } else {
                self.high.load(Acquire)
            };

            self.finish_write(curr);
            return unsplit(prev_high, prev_low);
        }
    }

    fn write(&self, curr: u32, value: u64) -> Result<(), u32> {
        self.try_start_write(curr)?;

        // incremented the sequence number, go ahead and do a write
        let (high, low) = split(value);
        self.low.store(low, Release);
        self.high.store(high, Release);

        self.finish_write(curr);
        Ok(())
    }

    /// Try to increment the sequence number by one to indicate that
    /// we're starting a write.
    #[inline(always)]
    #[cfg_attr(loom, track_caller)]
    fn try_start_write(&self, curr: u32) -> Result<u32, u32> {
        test_dbg!(self
            .seq
            .compare_exchange_weak(curr, curr.wrapping_add(1), Acquire, Relaxed))
    }

    // Increment the sequence number again to indicate that we have finished
    // a write.
    #[inline(always)]
    #[cfg_attr(loom, track_caller)]
    fn finish_write(&self, curr: u32) {
        test_dbg!(self.seq.store(curr.wrapping_add(2), Release));
    }
}

impl fmt::Debug for TearableU64 {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TearableU64")
            .field("value", &self.load())
            .field("seq", &self.seq.load(Relaxed))
            .finish()
    }
}

// === impl WriteInProgress ===

impl fmt::Display for WriteInProgress {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to store {}; another write is in progress",
            self.0
        )
    }
}

impl WriteInProgress {
    /// Returns the value that the caller of [`TearableU64::try_store`]
    /// attempted to store when this error was returned.
    pub fn into_inner(self) -> u64 {
        self.0
    }
}

// === helpers ===

/// Returns `true` if a sequence number indicates that a write is in progress.

#[inline(always)]
const fn is_writing(seq: u32) -> bool {
    seq & 1 == 1
}

/// Split a 64-bit integer into a `(high, low)` pair of 32-bit integers.
#[inline(always)]
const fn split(val: u64) -> (u32, u32) {
    ((val >> 32) as u32, val as u32)
}

/// Combine a `(high, low)` pair of 32-bit integers into a 64-bit integer.
const fn unsplit(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | (low as u64)
}

/// `AtomicU64` version of the polyfill.
// NOTE: `target_arch` values of "arm", "mips", and
// "powerpc" refer specifically to the 32-bit versions
// of those architectures; the 64-bit architectures get
// the `target_arch` strings "aarch64", "mips64", and
// "powerpc64", respectively.
#[cfg(not(any(
    target_arch = "arm",
    target_arch = "mips",
    target_arch = "powerpc",
    target_arch = "riscv32",
)))]
mod polyfill {
    use super::*;
    use crate::sync::atomic::AtomicU64;

    /// A polyfill implementation of an [`AtomicU64`] that is available
    /// regardless of whether or not the target architecture supports 64-bit
    /// atomic operations.
    ///
    /// This type provides a limited subset of the [`AtomicU64`] API. When
    /// 64-bit atomic operations are available, this type is implemented using
    /// an [`AtomicU64`]. Otherwise, it is implemented using a [`TearableU64`].
    ///
    /// This version of `AtomicU64Polyfill` was compiled for a target
    /// architecture that supports 64-bit atomic operations.
    #[derive(Debug)]
    pub struct AtomicU64Polyfill(AtomicU64);

    impl AtomicU64Polyfill {
        loom_const_fn! {
            #[must_use]
            pub fn new(value: u64) -> Self {
                Self(AtomicU64::new(value))
            }
        }

        /// Load the current value in the cell.
        ///
        /// This method always performs an [`Acquire`] load.
        #[inline]
        #[must_use]
        #[cfg_attr(loom, track_caller)]
        pub fn load(&self) -> u64 {
            self.0.load(Acquire)
        }

        /// Store `value` in the cell.
        ///
        /// This method always performs a [`Release`] store.
        #[inline]
        #[cfg_attr(loom, track_caller)]
        pub fn store(&self, value: u64) {
            self.0.store(value, Release)
        }

        /// Store `value` in the cell, returning the previous value.
        ///
        /// This method always performs an [`AcqRel`] swap.
        #[inline]
        #[must_use]
        #[cfg_attr(loom, track_caller)]
        pub fn swap(&self, value: u64) -> u64 {
            self.0.swap(value, AcqRel)
        }
    }
}

/// Version of the polyfill without `AtomicU64`.
// NOTE: `target_arch` values of "arm", "mips", and
// "powerpc" refer specifically to the 32-bit versions
// of those architectures; the 64-bit architectures get
// the `target_arch` strings "aarch64", "mips64", and
// "powerpc64", respectively.
#[cfg(any(
    target_arch = "arm",
    target_arch = "mips",
    target_arch = "powerpc",
    target_arch = "riscv32",
))]
mod polyfill {
    use super::*;

    /// A polyfill implementation of an [`AtomicU64`] that is available
    /// regardless of whether or not the target architecture supports 64-bit
    /// atomic operations.
    ///
    /// This type provides a limited subset of the [`AtomicU64`] API. When
    /// 64-bit atomic operations are available, this type is implemented using
    /// an [`AtomicU64`]. Otherwise, it is implemented using a [`TearableU64`].
    ///
    /// This version of `AtomicU64Polyfill` was compiled for a target
    /// architecture that does not support 64-bit atomic operations, and
    /// therefore uses a [`TearableU64`].
    #[derive(Debug)]
    pub struct AtomicU64Polyfill(TearableU64);

    impl AtomicU64Polyfill {
        loom_const_fn! {
            #[must_use]
            pub fn new(value: u64) -> Self {
                Self(TearableU64::new(value))
            }
        }

        /// Load the current value in the cell.
        ///
        /// This method always performs an [`Acquire`] load.
        #[inline]
        #[must_use]
        #[cfg_attr(loom, track_caller)]
        pub fn load(&self) -> u64 {
            self.0.load()
        }

        /// Store `value` in the cell.
        ///
        /// This method always performs a [`Release`] store.
        #[inline]
        #[cfg_attr(loom, track_caller)]
        pub fn store(&self, value: u64) {
            self.0.store(value)
        }

        /// Store `value` in the cell, returning the previous value.
        ///
        /// This method always performs an [`AcqRel`] swap.
        #[inline]
        #[must_use]
        #[cfg_attr(loom, track_caller)]
        pub fn swap(&self, value: u64) -> u64 {
            self.0.swap(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::loom::{self, thread};
    use std::sync::Arc;

    use super::*;
    const U32_MAX: u64 = u32::MAX as u64;

    // multiple reader tests hit the loom branch limit really easily, since
    // loom's scheduler may find a path through the execution where yielding
    // in the spin loop makes it ping-pong back and forth between the two
    // reader threads forever, without executing the writer again. this is
    // annoying. so, just test with one reader in `cfg!(loom)`.
    const READERS: usize = if cfg!(loom) { 1 } else { 4 };

    fn spawn_readers(
        tearable: &Arc<TearableU64>,
        vals: &'static [u64],
    ) -> Vec<thread::JoinHandle<()>> {
        (0..READERS)
            .map(|_| {
                let t = tearable.clone();
                thread::spawn(move || {
                    for _ in 0..vals.len() {
                        let value = test_dbg!(t.load());
                        assert!(
                            vals.contains(&value),
                            "\n value: {:?}\n  vals: {:?}",
                            value,
                            vals
                        );
                    }
                })
            })
            .collect::<Vec<_>>()
    }

    #[test]
    fn doesnt_tear() {
        const VALS: &[u64] = &[0, u64::MAX, U32_MAX - 1];

        let _trace = crate::test_util::trace_init();

        loom::model(|| {
            let t = Arc::new(TearableU64::new(0));
            let readers = spawn_readers(&t, VALS);

            for &value in &VALS[1..] {
                t.store(test_dbg!(value));
                thread::yield_now();
            }

            for thread in readers {
                thread.join().unwrap()
            }
        });
    }

    #[test]
    fn fetch_add_u32_would_tear() {
        const VALS: &[u64] = &[U32_MAX, U32_MAX + 1];

        let _trace = crate::test_util::trace_init();

        loom::model(|| {
            let t = Arc::new(TearableU64::new(U32_MAX));
            let readers = spawn_readers(&t, VALS);

            let prev = t.fetch_add_u32(1);
            thread::yield_now();
            assert_eq!(prev, U32_MAX);

            for thread in readers {
                thread.join().unwrap()
            }
        });
    }

    #[test]
    fn fetch_add_u32_no_tear() {
        const VALS: &[u64] = &[0, U32_MAX - 1, U32_MAX];

        let _trace = crate::test_util::trace_init();

        loom::model(|| {
            let t = Arc::new(TearableU64::new(0));

            let readers = spawn_readers(&t, VALS);

            let prev = t.fetch_add_u32(u32::MAX - 1);
            thread::yield_now();
            assert_eq!(prev, 0);

            let prev = t.fetch_add_u32(1);
            thread::yield_now();
            assert_eq!(prev, U32_MAX - 1);

            for thread in readers {
                thread.join().unwrap()
            }
        });
    }

    #[test]
    fn fetch_add_u32_wrap_low() {
        const VALS: &[u64] = &[U32_MAX - 1, U32_MAX + 99];

        let _trace = crate::test_util::trace_init();

        loom::model(|| {
            let t = Arc::new(TearableU64::new(U32_MAX - 1));

            let readers = spawn_readers(&t, VALS);

            let prev = t.fetch_add_u32(100);
            thread::yield_now();
            assert_eq!(prev, U32_MAX - 1);

            for thread in readers {
                thread.join().unwrap()
            }
        });
    }

    #[test]
    fn fetch_add_u32_wrap_high() {
        const VALS: &[u64] = &[u64::MAX, 100];

        let _trace = crate::test_util::trace_init();

        loom::model(|| {
            let t = Arc::new(TearableU64::new(u64::MAX));

            let readers = spawn_readers(&t, VALS);

            let prev = t.fetch_add_u32(100);
            thread::yield_now();
            assert_eq!(prev, u64::MAX);

            for thread in readers {
                thread.join().unwrap()
            }
        });
    }

    #[test]
    fn fetch_add_u32_high_maxed() {
        const HIGH_MAX: u64 = U32_MAX << 32;
        const VALS: &[u64] = &[HIGH_MAX, HIGH_MAX + 100];

        let _trace = crate::test_util::trace_init();

        loom::model(|| {
            let t = Arc::new(TearableU64::new(HIGH_MAX));

            let readers = spawn_readers(&t, VALS);

            let prev = t.fetch_add_u32(100);
            thread::yield_now();
            assert_eq!(prev, HIGH_MAX);

            for thread in readers {
                thread.join().unwrap()
            }
        });
    }
}
