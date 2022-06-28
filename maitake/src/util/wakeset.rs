use core::{ptr, task::Waker};
use mycelium_util::mem::CheckedMaybeUninit;

pub(crate) struct WakeSet {
    init: usize,
    wakers: [CheckedMaybeUninit<Waker>; MAX_WAKERS],
}

// 16 seems like a decent size for a stack array, could make this 32 (or a const
// generic).
const MAX_WAKERS: usize = 16;

impl WakeSet {
    #[must_use]
    pub(crate) const fn new() -> Self {
        const INIT: CheckedMaybeUninit<Waker> = CheckedMaybeUninit::uninit();
        Self {
            init: 0,
            wakers: [INIT; MAX_WAKERS],
        }
    }

    #[inline]
    pub(crate) fn can_add_waker(&self) -> bool {
        self.init < MAX_WAKERS
    }

    pub(crate) fn add_waker(&mut self, waker: Waker) -> bool {
        debug_assert!(self.can_add_waker());
        unsafe {
            self.wakers.get_unchecked_mut(self.init).write(waker);
        }
        self.init += 1;
        self.can_add_waker()
    }

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

impl Drop for WakeSet {
    fn drop(&mut self) {
        let slice =
            ptr::slice_from_raw_parts_mut(self.wakers.as_mut_ptr() as *mut Waker, self.init);
        unsafe { ptr::drop_in_place(slice) };
    }
}
