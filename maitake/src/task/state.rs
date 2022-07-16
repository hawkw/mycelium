use crate::loom::sync::atomic::{
    self, AtomicUsize,
    Ordering::{self, *},
};
use core::fmt;

mycelium_bitfield::bitfield! {
    /// A snapshot of a task's current state.
    #[derive(PartialEq, Eq)]
    pub(crate) struct State<usize> {
        /// If set, this task is currently being polled.
        pub(crate) const POLLING: bool;

        /// If set, this task's [`Waker`] has been woken.
        ///
        /// [`Waker`]: core::task::Waker
        pub(crate) const WOKEN: bool;

        /// If set, this task's [`Future`] has completed (i.e., it has returned
        /// [`Poll::Ready`]).
        ///
        /// [`Future`]: core::future::Future
        /// [`Poll::Ready`]: core::task::Poll::Ready
        pub(crate) const COMPLETED: bool;

        /// If set, this task has a [`JoinHandle`] awaiting its completion.
        ///
        /// If the `JoinHandle` is dropped, this flag is unset.
        pub(crate) const HAS_JOIN_HANDLE: bool;

        /// The number of currently live references to this task.
        ///
        /// When this is 0, the task may be deallocated.
        const REFS = ..;
    }

}

/// An atomic cell that stores a task's current [`State`].
#[repr(transparent)]
pub(super) struct StateCell(AtomicUsize);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum ScheduleAction {
    /// The task should be enqueued.
    Enqueue,

    /// The task does not need to be enqueued.
    None,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum PollAction {
    /// The task should be enqueued.
    Enqueue,

    /// The task's join waker should be woken.
    WakeJoinWaiter,

    /// The task does not need to be enqueued, and the join waker does not need
    /// to be woken.
    None,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum OrDrop<T> {
    /// Another action should be performed.
    Action(T),

    /// The task should be deallocated.
    Drop,
}

pub(super) type WakeAction = OrDrop<ScheduleAction>;

impl State {
    #[inline]
    pub(crate) fn ref_count(self) -> usize {
        self.get(Self::REFS)
    }

    fn drop_ref(self) -> Self {
        Self(self.0 - REF_ONE)
    }

    fn clone_ref(self) -> Self {
        Self(self.0 + REF_ONE)
    }
}

const REF_ONE: usize = State::REFS.first_bit();
const REF_MAX: usize = State::REFS.raw_mask();

// === impl StateCell ===

impl StateCell {
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Self(AtomicUsize::new(REF_ONE))
    }

    #[cfg(loom)]
    pub fn new() -> Self {
        Self(AtomicUsize::new(REF_ONE))
    }

    pub(super) fn start_poll(&self) -> Result<State, State> {
        self.transition(|state| {
            // Cannot start polling a task which is being polled on another
            // thread.
            if test_dbg!(state.get(State::POLLING)) {
                return Err(*state);
            }

            // Cannot start polling a completed task.
            if test_dbg!(state.get(State::COMPLETED)) {
                return Err(*state);
            }

            let new_state = state
                // The task is now being polled.
                .with(State::POLLING, true)
                // If the task was woken, consume the wakeup.
                .with(State::WOKEN, false);
            *state = new_state;
            Ok(new_state)
        })
    }

    pub(super) fn end_poll(&self, completed: bool) -> OrDrop<PollAction> {
        self.transition(|state| {
            // Cannot end a poll if a task is not being polled!
            debug_assert!(state.get(State::POLLING));
            debug_assert!(!state.get(State::COMPLETED));
            let mut next_state = state
                .with(State::POLLING, false)
                .with(State::COMPLETED, completed);

            // Was the task woken during the poll?
            if !test_dbg!(completed) && test_dbg!(state.get(State::WOKEN)) {
                *state = test_dbg!(next_state);
                return OrDrop::Action(PollAction::Enqueue);
            }

            let had_join_waiter = if test_dbg!(completed) {
                // unset the join handle flag, as we are waking the join handle
                // now.
                next_state = next_state.with(State::HAS_JOIN_HANDLE, false);
                test_dbg!(state.get(State::HAS_JOIN_HANDLE))
            } else {
                false
            };

            *state = next_state;

            if next_state.ref_count() == 0 {
                debug_assert!(!had_join_waiter, "a task's ref count went to zero, but the `HAS_JOIN_HANDLE` bit was set! state: {state:?}");
                OrDrop::Drop
            } else if had_join_waiter {

                OrDrop::Action(PollAction::WakeJoinWaiter)
            } else {
                OrDrop::Action(PollAction::None)
            }
        })
    }

    /// Transition to the woken state by value, returning `true` if the task
    /// should be enqueued.
    pub(super) fn wake_by_val(&self) -> WakeAction {
        self.transition(|state| {
            // If the task was woken *during* a poll, it will be re-queued by the
            // scheduler at the end of the poll if needed, so don't enqueue it now.
            if test_dbg!(state.get(State::POLLING)) {
                *state = state.with(State::WOKEN, true).drop_ref();
                assert!(state.ref_count() > 0);

                return OrDrop::Action(ScheduleAction::None);
            }

            // If the task is already completed or woken, we don't need to
            // requeue it, but decrement the ref count for the waker that was
            // used for this wakeup.
            if test_dbg!(state.get(State::COMPLETED)) || test_dbg!(state.get(State::WOKEN)) {
                let new_state = state.drop_ref();
                *state = new_state;
                return if new_state.ref_count() == 0 {
                    OrDrop::Drop
                } else {
                    OrDrop::Action(ScheduleAction::None)
                };
            }

            // Otherwise, transition to the notified state and enqueue the task.
            *state = state.with(State::WOKEN, true).clone_ref();
            OrDrop::Action(ScheduleAction::Enqueue)
        })
    }

    /// Transition to the woken state by ref, returning `true` if the task
    /// should be enqueued.
    pub(super) fn wake_by_ref(&self) -> ScheduleAction {
        self.transition(|state| {
            if test_dbg!(state.get(State::COMPLETED)) || test_dbg!(state.get(State::WOKEN)) {
                return ScheduleAction::None;
            }

            if test_dbg!(state.get(State::POLLING)) {
                state.set(State::WOKEN, true);
                return ScheduleAction::None;
            }

            *state = state.with(State::WOKEN, true).clone_ref();
            ScheduleAction::Enqueue
        })
    }

    #[inline]
    pub(super) fn clone_ref(&self) {
        // Using a relaxed ordering is alright here, as knowledge of the
        // original reference prevents other threads from erroneously deleting
        // the object.
        //
        // As explained in the [Boost documentation][1], Increasing the
        // reference counter can always be done with memory_order_relaxed: New
        // references to an object can only be formed from an existing
        // reference, and passing an existing reference from one thread to
        // another must already provide any required synchronization.
        //
        // [1]: (www.boost.org/doc/libs/1_55_0/doc/html/atomic/usage_examples.html)
        let old_refs = self.0.fetch_add(REF_ONE, Relaxed);
        test_dbg!(State::REFS.unpack(old_refs));

        // However we need to guard against massive refcounts in case someone
        // is `mem::forget`ing tasks. If we don't do this the count can overflow
        // and users will use-after free. We racily saturate to `isize::MAX` on
        // the assumption that there aren't ~2 billion threads incrementing
        // the reference count at once. This branch will never be taken in
        // any realistic program.
        //
        // We abort because such a program is incredibly degenerate, and we
        // don't care to support it.
        if test_dbg!(old_refs > REF_MAX) {
            panic!("task reference count overflow");
        }
    }

    #[inline]
    pub(super) fn drop_ref(&self) -> bool {
        // We do not need to synchronize with other cores unless we are going to
        // delete the task.
        let old_refs = self.0.fetch_sub(REF_ONE, Release);

        // Manually shift over the refcount to clear the state bits. We don't
        // use the packing spec here, because it would also mask out any high
        // bits, and we can avoid doing the bitwise-and (since there are no
        // higher bits that are not part of the ref count). This is probably a
        // premature optimization lol.
        let old_refs = old_refs >> State::REFS.least_significant_index();
        test_dbg!(State::REFS.unpack(old_refs));

        // Did we drop the last ref?
        if test_dbg!(old_refs) > 1 {
            return false;
        }

        atomic::fence(Acquire);
        true
    }

    #[inline]
    pub(super) fn drop_join_handle(&self) {
        const MASK: usize = !State::HAS_JOIN_HANDLE.raw_mask();
        let _prev = self.0.fetch_and(MASK, Release);
        debug_assert!(
            State(_prev).get(State::HAS_JOIN_HANDLE),
            "tried to drop a join handle when the task did not have a join handle!\nstate: {:#?}",
            State(_prev),
        )
    }

    pub(super) fn load(&self, order: Ordering) -> State {
        State(self.0.load(order))
    }

    /// Advance this task's state by running the provided
    /// `transition` function on the current [`State`].
    fn transition<T>(&self, mut transition: impl FnMut(&mut State) -> T) -> T {
        let mut current = self.load(Acquire);
        loop {
            let mut next = test_dbg!(current);
            // Run the transition function.
            let res = transition(&mut next);

            if current.0 == next.0 {
                return res;
            }

            match self
                .0
                .compare_exchange_weak(current.0, next.0, AcqRel, Acquire)
            {
                Ok(_) => return res,
                Err(actual) => current = State(actual),
            }
        }
    }
}

impl fmt::Debug for StateCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.load(Relaxed).fmt(f)
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;

    #[test]
    fn packing_specs_valid() {
        State::assert_valid()
    }

    #[test]
    fn debug_alt() {
        let state = StateCell::new();
        println!("{:#?}", state);
    }
}
