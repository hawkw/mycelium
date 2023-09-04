use super::CheckedMaybeUninit;
use core::{ptr, task::Waker};

/// A utility for waking multiple tasks in a batch, without reallocating.
///
/// This type is essentially an array of [`Waker`]s to which multiple tasks'
/// [`Waker`]s can be added, until the array fills up. Once the array is full,
/// all the tasks in the batch can be woken by calling [`WakeBatch::wake_all`],
/// and the array refilled with new tasks.
///
/// This is useful when a lock must be held to remove [`Waker`]s from a queue
/// (e.g. a [`cordyceps::List`]), but the lock can be released before the tasks
/// are actually woken. Doing this repeatedly, rather than holding the lock for
/// the entire wake process, may improve latency for other tasks that are
/// attempting to access the lock. Additionally, it may avoid deadlocks that
/// occur when a woken task will attempt to access the lock itself.
pub(crate) struct WakeBatch {
    init: usize,
    wakers: [CheckedMaybeUninit<Waker>; MAX_WAKERS],
}

// 16 seems like a decent size for a stack array, could make this 32 (or a const
// generic).
//
// when running loom tests, make the max much lower, so we can exercise behavior
// involving multiple lock acquisitions.
const MAX_WAKERS: usize = if cfg!(loom) { 2 } else { 16 };

impl WakeBatch {
    #[must_use]
    pub(crate) const fn new() -> Self {
        const INIT: CheckedMaybeUninit<Waker> = CheckedMaybeUninit::uninit();
        Self {
            init: 0,
            wakers: [INIT; MAX_WAKERS],
        }
    }

    /// Returns `true` if there is room for one or more additional [`Waker`] in
    /// this batch.
    ///
    /// When this method returns `false`, [`WakeBatch::wake_all`] should be
    /// called to wake all the wakers currently in the batch, and then this
    /// method will return `true` again.
    #[inline]
    pub(crate) fn can_add_waker(&self) -> bool {
        self.init < MAX_WAKERS
    }

    /// Adds a [`Waker`] to the batch, returning `true` if the batch still has
    /// capacity for an additional waker.
    ///
    /// When this method returns `false`, the most recently added waker used the
    /// last slot in the batch, and [`WakeBatch::wake_all`] must be called
    /// before continuing to add new [`Waker`]s.
    pub(crate) fn add_waker(&mut self, waker: Waker) -> bool {
        debug_assert!(self.can_add_waker());
        unsafe {
            self.wakers.get_unchecked_mut(self.init).write(waker);
        }
        self.init += 1;
        self.can_add_waker()
    }

    /// Wake all the tasks whose [`Waker`]s are currently in the batch.
    pub(crate) fn wake_all(&mut self) {
        let init = self.init;
        self.init = 0;
        for waker in self.wakers[..init].iter_mut() {
            unsafe {
                waker.as_mut_ptr().read().wake();
            }
        }
    }
}

impl Drop for WakeBatch {
    fn drop(&mut self) {
        let slice =
            ptr::slice_from_raw_parts_mut(self.wakers.as_mut_ptr() as *mut Waker, self.init);
        unsafe { ptr::drop_in_place(slice) };
    }
}
