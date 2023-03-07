use super::{
    rwlock::{RwLock, RwLockReadGuard},
    WaitQueue,
};
use crate::loom::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering::*},
};
use alloc::sync::Arc;
use core::ops::Deref;
use mycelium_util::{fmt, sync::CachePadded};

#[cfg(test)]
mod tests;

pub struct Publisher<T> {
    shared: Arc<Shared<T>>,
}

pub struct Subscriber<T> {
    shared: Arc<Shared<T>>,
    pos: usize,
}

pub struct RecvRef<'a, T> {
    slot: RwLockReadGuard<'a, Slot<T>>,
}

#[derive(Debug)]
pub enum TryRecvError {
    Empty,
    Closed,
    Lagged(usize),
}

pub fn channel<T>(mut capacity: usize) -> (Publisher<T>, Subscriber<T>) {
    capacity = capacity.next_power_of_two();

    let shared = Arc::new(Shared {
        mask: capacity - 1,
        pubs: CachePadded::new(AtomicUsize::new(1)),
        subs: CachePadded::new(AtomicUsize::new(1)),
        tail: CachePadded::new(AtomicUsize::new(0)),
        sub_wait: WaitQueue::new(),
        slots: (0..capacity)
            .map(|i| {
                RwLock::new(Slot {
                    rem: AtomicUsize::new(0),
                    gen: i.wrapping_sub(capacity),
                    val: None,
                })
            })
            .collect::<_>(),
    });

    let pub_ = Publisher {
        shared: shared.clone(),
    };
    let sub = Subscriber { shared, pos: 0 };

    (pub_, sub)
}

struct Shared<T> {
    tail: CachePadded<AtomicUsize>,
    /// Number of subscribers.
    subs: CachePadded<AtomicUsize>,
    /// Number of publishers.
    pubs: CachePadded<AtomicUsize>,
    /// Capapcity - 1 of the channel.
    mask: usize,
    sub_wait: WaitQueue,
    slots: alloc::boxed::Box<[RwLock<Slot<T>>]>,
}

struct Slot<T> {
    /// Count of readers remaining to view this slot.
    rem: AtomicUsize,

    gen: usize,

    /// The value broadcast at this position.
    ///
    /// The value is set when sending a value to this slot.
    val: Option<T>,
}

// === impl Publisher ===

impl<T> Publisher<T> {
    pub async fn send(&self, value: T) -> Result<(), T> {
        test_debug!("Publisher::send");
        let tail = test_dbg!(self.shared.tail.fetch_add(1, AcqRel));
        let idx = tail & self.shared.mask;
        {
            let mut slot = self.shared.slots[test_dbg!(idx)].write().await;

            // load subscriber count
            let subs = self.shared.subs.load(Acquire);
            if test_dbg!(subs) == 0 {
                return Err(value);
            }

            // write to the slot
            slot.gen = tail;
            slot.rem.store(subs, Release);
            slot.val = Some(value);
        }

        // wake any waiting subscribers
        self.shared.sub_wait.wake_all();
        test_debug!("wrote value to slot {idx}");

        Ok(())
    }

    pub fn subscribe(&self) -> Subscriber<T> {
        self.shared.subs.fetch_add(1, Relaxed);
        Subscriber {
            shared: self.shared.clone(),
            pos: self.shared.tail.load(Acquire),
        }
    }
}

impl<T> Clone for Publisher<T> {
    fn clone(&self) -> Self {
        self.shared.pubs.fetch_add(1, Relaxed);
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<T> Drop for Publisher<T> {
    fn drop(&mut self) {
        if self.shared.pubs.fetch_sub(1, AcqRel) == 1 {
            self.shared.sub_wait.close();
        }
    }
}

impl<T> fmt::Debug for Publisher<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { shared } = self;
        f.debug_struct("Publisher").field("shared", shared).finish()
    }
}

// === impl Subscriber ===

impl<T: Clone> Subscriber<T> {
    pub async fn recv(&mut self) -> Result<T, TryRecvError> {
        test_debug!("Subscriber::recv(pos: {})", self.pos);
        self.recv_ref().await.map(|r| r.clone())
    }

    pub async fn try_recv(&mut self) -> Result<T, TryRecvError> {
        test_debug!("Subscriber::try_recv(pos: {})", self.pos);
        self.try_recv_ref().await.map(|r| r.clone())
    }
}

impl<T> Subscriber<T> {
    pub async fn recv_ref(&mut self) -> Result<RecvRef<'_, T>, TryRecvError> {
        test_debug!("Subscriber::recv_ref(pos: {})", self.pos);
        loop {
            match Self::try_recv_ref2(&self.shared, &mut self.pos).await {
                Ok(val) => {
                    test_debug!("Subscriber::recv_ref -> received!");
                    return Ok(val);
                }
                Err(TryRecvError::Empty) => {
                    test_debug!("Subscriber::recv_ref -> empty; waiting...");
                    // ignore errors here; the WaitQueue may close while there
                    // are still slots we have left to read, and the subsequent
                    // `try_recv_ref2` call will handle this.
                    let _ = test_dbg!(self.shared.sub_wait.wait().await);
                }
                Err(e) => {
                    test_debug!("Subscriber::recv_ref -> error {e:?}");
                    return Err(e);
                }
            }
        }
    }

    pub async fn try_recv_ref(&mut self) -> Result<RecvRef<'_, T>, TryRecvError> {
        test_debug!("Subscriber::recv(pos: {})", self.pos);
        Self::try_recv_ref2(&self.shared, &mut self.pos).await
    }

    async fn try_recv_ref2<'shared>(
        shared: &'shared Shared<T>,
        pos: &mut usize,
    ) -> Result<RecvRef<'shared, T>, TryRecvError> {
        let idx = test_dbg!(*pos) & shared.mask;

        let slot = shared.slots[test_dbg!(idx)].read().await;

        if test_dbg!(slot.gen != *pos) {
            // we lagged behind, try to read the next slot.
            let lap = slot.gen.wrapping_add(shared.slots.len());

            // the channel is empty relative to this receiver.
            if test_dbg!(lap == *pos) {
                return if shared.sub_wait.is_closed() {
                    Err(TryRecvError::Closed)
                } else {
                    Err(TryRecvError::Empty)
                };
            }

            let tail = shared.tail.load(Acquire);
            let next = tail.wrapping_sub(shared.slots.len());

            let missed = next.wrapping_sub(*pos);

            // The receiver is slow but no values have been missed
            if test_dbg!(missed) == 0 {
                *pos = pos.wrapping_add(1);

                return Ok(RecvRef { slot });
            }

            *pos = next;

            return Err(TryRecvError::Lagged(missed));
        }

        *pos = pos.wrapping_add(1);
        Ok(RecvRef { slot })
    }
}

impl<T> fmt::Debug for Subscriber<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { pos, shared } = self;
        f.debug_struct("Subscriber")
            .field("shared", shared)
            .field("pos", pos)
            .finish()
    }
}

// === impl RecvRef ===

impl<T: fmt::Debug> fmt::Debug for RecvRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.slot.val.as_ref().unwrap().fmt(f)
    }
}

impl<T> Deref for RecvRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.slot
            .val
            .as_ref()
            .expect("a RecvRef is only returned if the slot is `Some`")
    }
}

// === impl Shared ===

impl<T> fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            tail,
            subs,
            pubs,
            mask,
            slots,
            sub_wait,
        } = self;
        f.debug_struct("Shared")
            .field("tail", tail)
            .field("subs", subs)
            .field("pubs", pubs)
            .field("mask", &fmt::hex(mask))
            .field("slots", slots)
            .field("sub_wait", sub_wait)
            .finish()
    }
}

impl<T> fmt::Debug for Slot<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { gen, rem, val } = self;
        f.debug_struct("Slot")
            .field("gen", gen)
            .field("rem", rem)
            .field(
                "val",
                &if val.is_some() {
                    format_args!("Some(...)")
                } else {
                    format_args!("None")
                },
            )
            .finish()
    }
}
