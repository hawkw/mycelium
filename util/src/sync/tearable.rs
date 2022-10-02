use crate::sync::{
    atomic::{AtomicU32, Ordering::*},
    hint,
    spin::Backoff,
};

/// An efficient
/// [sequence lock]: https://en.wikipedia.org/wiki/Seqlock
pub struct TearableU64 {
    seq: AtomicU32,
    high: AtomicU32,
    low: AtomicU32,
}

pub struct TryWriteError<T>(T);

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

    /// Reads the data stored inside the sequence lock.
    ///
    /// This method may spin if the lock is currently being written to.
    pub fn load(&self) -> u64 {
        loop {
            let mut preread = self.seq.load(Acquire);

            let mut backoff = Backoff::new();
            while is_writing(test_dbg!(preread)) {
                backoff.spin();
                preread = self.seq.load(Acquire);
            }

            let low = self.low.load(Acquire);
            let high = self.high.load(Acquire);

            let postread = self.seq.load(Acquire);

            // if the sequence numbers match, we didn't observe a torn write.
            // return it!
            if test_dbg!(preread) == test_dbg!(postread) {
                return unsplit(high, low);
            }

            // in the case we *did* observe a torn write, we only issue one spin
            // loop hint, rather than backing off, because if we try another
            // read, we should get something reasonable (unless it's being
            // written to again).
            hint::spin_loop()
        }
    }

    /// Writes a new value to the data stored inside the lock.
    ///
    /// This method may spin if the lock is currently being written to.
    pub fn store(&self, value: u64) {
        let mut seq = self.seq.load(Relaxed);
        let mut backoff = Backoff::new();

        // wait for a current write to complete
        while test_dbg!(is_writing(seq)) {
            backoff.spin();
            seq = self.seq.load(Relaxed);
        }

        // increment the sequence number by one to indicate that we're starting
        // a write.
        while let Err(actual) =
            test_dbg!(self
                .seq
                .compare_exchange_weak(seq, seq.wrapping_add(1), Acquire, Relaxed))
        {
            seq = actual;
            hint::spin_loop();
        }

        let (high, low) = split(value);
        self.low.store(low, Release);
        self.high.store(high, Release);

        // increment the sequence number again to indicate that we have finished
        // a write.
        self.seq.store(seq.wrapping_add(2), Release);
    }

    // pub fn try_store(&self, value: u64) -> Result<(), TryWriteError<u64>> {
    //     // increment the sequence number by one to indicate that we're starting
    //     // a write.
    //     let seq = self.seq.fetch_add(1, Relaxed);
    //     if is_writing(seq) {
    //         return Err(TryWriteError(value));
    //     }

    //     let next = seq.wrapping_add(1);
    //     self.seq
    //         .compare_exchange(seq, next, Acquire, Relaxed)
    //         .map_err(|_| TryWriteError(value))?;

    //     let (high, low) = split(value);
    //     self.low.store(low, Release);
    //     self.high.store(high, Release);

    //     // increment the sequence number again to indicate that we have finished
    //     // a write.
    //     self.seq.store(seq.wrapping_add(2), Release);
    //     Ok(())
    // }
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

    #[test]
    fn spmc() {
        const VALS: &[u64] = &[0, u64::MAX, u32::MAX as u64 + 1];
        const READERS: usize = 2;

        loom::model(|| {
            let t = Arc::new(TearableU64::new(0));

            let threads = (0..READERS)
                .map(|_| {
                    let t = t.clone();
                    thread::spawn(move || {
                        for _ in 0..READERS {
                            let value = test_dbg!(t.load());
                            assert!(VALS.contains(&value));
                        }
                    })
                })
                .collect::<Vec<_>>();

            for &value in &VALS[1..] {
                t.store(test_dbg!(value));
                thread::yield_now();
            }

            for thread in threads {
                thread.join().unwrap()
            }
        });
    }
}
