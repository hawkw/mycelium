use crate::loom::sync::atomic::{
    self, AtomicUsize,
    Ordering::{self, *},
};
use core::fmt;

mycelium_bitfield::bitfield! {
    /// A bitfield that represents a task's current state.
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

        /// The number of currently live references to this task.
        ///
        /// When this is 0, the task may be deallocated.
        const REFS = ..;
    }

}

#[repr(transparent)]
pub(super) struct StateVar(AtomicUsize);

impl State {
    #[inline]
    pub(crate) fn ref_count(self) -> usize {
        self.get(Self::REFS)
    }
}

const REF_ONE: usize = State::REFS.first_bit();
const REF_MAX: usize = State::REFS.raw_mask();

// === impl StateVar ===

impl StateVar {
    pub fn new() -> Self {
        Self(AtomicUsize::new(REF_ONE))
    }

    pub(super) fn start_poll(&self) -> Result<(), State> {
        self.transition(|state| {
            // Cannot start polling a task which is being polled on another
            // thread.
            if test_dbg!(state.get(State::POLLING)) {
                return Err(state);
            }

            // Cannot start polling a completed task.
            if test_dbg!(state.get(State::COMPLETED)) {
                return Err(state);
            }

            let new_state = state
                // The task is now being polled.
                .with(State::POLLING, true)
                // If the task was woken, consume the wakeup.
                .with(State::WOKEN, false);
            Ok(test_dbg!(new_state))
        })
    }

    pub(super) fn end_poll(&self, completed: bool) -> Result<(), State> {
        self.transition(|state| {
            // Cannot end a poll if a task is not being polled!
            debug_assert!(state.get(State::POLLING));
            debug_assert!(!state.get(State::COMPLETED));

            let new_state = state
                .with(State::POLLING, false)
                .with(State::COMPLETED, completed);
            Ok(test_dbg!(new_state))
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
        let old_refs = test_dbg!(self.0.fetch_add(REF_ONE, Relaxed));

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
        // Because `cores` is already atomic, we do not need to synchronize
        // with other threads unless we are going to delete the task.
        let old_refs = test_dbg!(self.0.fetch_sub(REF_ONE, Release));

        // Did we drop the last ref?
        if test_dbg!(old_refs != REF_ONE) {
            return false;
        }

        atomic::fence(Acquire);
        true
    }

    pub(super) fn load(&self, order: Ordering) -> State {
        State(self.0.load(order))
    }

    /// Attempts to advance this task's state by running the provided fallible
    /// `transition` function on the current [`State`].
    ///
    /// The `transition` function should return an error if the desired state
    /// transition is not possible from the task's current state, or return `Ok`
    /// with a new [`State`] if the transition is possible.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the task was successfully transitioned.
    /// - `Err(E)` with the error returned by the transition function if the
    ///   state transition is not possible.
    fn transition<E>(
        &self,
        mut transition: impl FnMut(State) -> Result<State, E>,
    ) -> Result<(), E> {
        let mut current = self.load(Acquire);
        loop {
            // Try to run the transition function to transition from `current`
            // to the next state. If the transition functiion fails (indicating
            // that the requested transition is no longer reachable from the
            // current state), bail.
            let State(next) = transition(current)?;

            match self
                .0
                .compare_exchange_weak(current.0, next, AcqRel, Acquire)
            {
                Ok(_) => return Ok(()),
                Err(actual) => current = State(actual),
            }
        }
    }
}

impl fmt::Debug for StateVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.load(Relaxed).fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_specs_valid() {
        State::assert_valid()
    }
}
