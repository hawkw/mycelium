use crate::sync::{
    atomic::{AtomicU32, Ordering::*},
    hint,
};

use core::convert::TryFrom;

/// An efficient shareable 64-bit integer value, based on a [sequence lock].
///
/// This type is intended for cases where an atomic 64-bit value is needed on
/// platforms that do not support atomic operations on 64-bit memory locations.
///
/// [sequence lock]: https://en.wikipedia.org/wiki/Seqlock
pub struct TearableU64 {
    seq: AtomicU32,
    high: AtomicU32,
    low: AtomicU32,
}

impl TearableU64 {
    loom_const_fn! {
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

            hint::spin_loop();
        }
    }

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

            hint::spin_loop();
        }
    }

    pub fn fetch_add(&self, value: u64) -> u64 {
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

            let (high, low) = split(value);
            let prev_low = self.low.fetch_add(low, AcqRel);
            let overflow = (prev_low as u64 + low as u64).saturating_sub(u32::MAX as u64);
            let prev_high = match u32::try_from(high as u64 + overflow) {
                Ok(high) => self.high.fetch_add(high, Release),
                Err(_) => todo!("wrap around"),
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
    fn try_start_write(&self, curr: u32) -> Result<u32, u32> {
        test_dbg!(self
            .seq
            .compare_exchange_weak(curr, curr.wrapping_add(1), Acquire, Relaxed))
    }

    // Increment the sequence number again to indicate that we have finished
    // a write.
    #[inline(always)]
    fn finish_write(&self, curr: u32) {
        test_dbg!(self.seq.store(curr.wrapping_add(2), Release));
    }
}

/// Returns `true` if a sequence number indicates that a write is in progress.

#[inline(always)]
const fn is_writing(seq: u32) -> bool {
    seq & 1 == 1
}

const fn split(val: u64) -> (u32, u32) {
    ((val >> 32) as u32, val as u32)
}

const fn unsplit(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | (low as u64)
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
                        assert!(vals.contains(&value));
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
    fn fetch_add_would_tear() {
        const VALS: &[u64] = &[U32_MAX, U32_MAX + 1];

        let _trace = crate::test_util::trace_init();

        loom::model(|| {
            let t = Arc::new(TearableU64::new(U32_MAX));
            let readers = spawn_readers(&t, VALS);

            let prev = t.fetch_add(1);
            thread::yield_now();
            assert_eq!(prev, U32_MAX);

            for thread in readers {
                thread.join().unwrap()
            }
        });
    }

    #[test]
    fn fetch_add_no_tear() {
        const VALS: &[u64] = &[0, U32_MAX - 1, U32_MAX];

        let _trace = crate::test_util::trace_init();

        loom::model(|| {
            let t = Arc::new(TearableU64::new(0));

            let readers = spawn_readers(&t, VALS);

            let prev = t.fetch_add(U32_MAX - 1);
            thread::yield_now();
            assert_eq!(prev, 0);

            let prev = t.fetch_add(1);
            thread::yield_now();
            assert_eq!(prev, U32_MAX - 1);

            for thread in readers {
                thread.join().unwrap()
            }
        });
    }
}
