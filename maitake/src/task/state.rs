use crate::loom::sync::atomic::{
    self, AtomicUsize,
    Ordering::{self, *},
};
use core::fmt;
use mycelium_util::{sync::spin::Backoff, unreachable_unchecked};

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
        ///
        /// This flag does *not* indicate the presence of a [`Waker`] in the
        /// `join_waker` slot; it only indicates that a [`JoinHandle`] for this
        /// task *exists*. The join waker may not currently be registered if
        /// this flag is set.
        pub(crate) const HAS_JOIN_HANDLE: bool;

        /// The state of the task's [`JoinHandle`] waker.
        const JOIN_WAKER: JoinWakerState;

        /// If set, this task has output ready to be taken by a [`JoinHandle`].
        pub(crate) const HAS_OUTPUT: bool;

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
pub(super) enum JoinAction {
    /// It's safe to take the task's output!
    TakeOutput,

    /// Register the *first* join waker; there is no previous join waker and the
    /// slot is not initialized.
    Register,

    /// The task is not ready to read the output, but a previous join waker is
    /// registered.
    Reregister,
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
enum JoinWakerState {
    /// There is no join waker; the slot is uninitialized.
    Empty = 0b00,
    /// A join waker is *being* registered.
    Registering = 0b01,
    /// A join waker is registered, the slot is initialized.
    Waiting = 0b10,
    /// The join waker has been woken.
    Woken = 0b11,
}

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
        let mut should_wait_for_join_waker = false;
        let action = self.transition(|state| {
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

            let had_join_waker = if test_dbg!(completed) {
                // set the output flag so that the joinhandle knows it is now
                // safe to read the task's output.
                next_state.set(State::HAS_OUTPUT, true);
                match state.get(State::JOIN_WAKER) {
                    JoinWakerState::Empty => false,
                    JoinWakerState::Registering => {
                        should_wait_for_join_waker = true;
                        true
                    },
                    JoinWakerState::Waiting => {
                        should_wait_for_join_waker = false;
                        next_state.set(State::JOIN_WAKER, JoinWakerState::Empty);
                        true
                    },
                    JoinWakerState::Woken => {
                        debug_assert!(false, "join waker should not be woken until task has completed, wtf");
                        false
                    }
                }
            } else {
                false
            };


            let action = if next_state.ref_count() == 0 {
                debug_assert!(
                    !had_join_waker,
                    "a task's ref count went to zero, but the `HAS_JOIN_WAKER` bit was set! state: {state:?}",
                );
                debug_assert!(
                    !state.get(State::HAS_JOIN_HANDLE),
                    "a task's ref count went to zero, but the `HAS_JOIN_HANDLE` bit was set! state: {state:?}",
                );
                OrDrop::Drop
            } else if had_join_waker {
                debug_assert!(
                    state.get(State::HAS_JOIN_HANDLE),
                    "a task cannot have a join waker if it does not have a join handle!",
                );

                OrDrop::Action(PollAction::WakeJoinWaiter)
            } else {
                OrDrop::Action(PollAction::None)
            };

            *state = next_state;

            action
        });

        if should_wait_for_join_waker {
            debug_assert_eq!(action, OrDrop::Action(PollAction::WakeJoinWaiter));
            self.wait_for_join_waker();
        }

        action
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
        test_debug!("StateCell::drop_ref");
        // We do not need to synchronize with other cores unless we are going to
        // delete the task.
        let old_refs = self.0.fetch_sub(REF_ONE, Release);

        // Manually shift over the refcount to clear the state bits. We don't
        // use the packing spec here, because it would also mask out any high
        // bits, and we can avoid doing the bitwise-and (since there are no
        // higher bits that are not part of the ref count). This is probably a
        // premature optimization lol.
        test_dbg!(State::REFS.unpack(old_refs));
        let old_refs = old_refs >> State::REFS.least_significant_index();

        // Did we drop the last ref?
        if test_dbg!(old_refs) > 1 {
            return false;
        }

        atomic::fence(Acquire);
        true
    }

    #[inline]
    pub(super) fn create_join_handle(&self) {
        test_debug!("StateCell::create_join_handle");
        self.transition(|state| {
            debug_assert!(
                !state.get(State::HAS_JOIN_HANDLE),
                "task already has a join handle, cannot create a new one! state={state:?}"
            );

            *state = state.with(State::HAS_JOIN_HANDLE, true);
        })
    }

    #[inline]
    pub(super) fn drop_join_handle(&self) {
        test_debug!("StateCell::drop_join_handle");
        const MASK: usize = !State::HAS_JOIN_HANDLE.raw_mask();
        let _prev = self.0.fetch_and(MASK, Release);
        test_trace!(
            "drop_join_handle; prev_state:\n{}\nstate:\n{}",
            State::from_bits(_prev),
            self.load(Acquire),
        );
        debug_assert!(
            State(_prev).get(State::HAS_JOIN_HANDLE),
            "tried to drop a join handle when the task did not have a join handle!\nstate: {:#?}",
            State(_prev),
        )
    }

    /// Returns whether if it's okay to take the task's output.
    pub(super) fn try_join(&self) -> JoinAction {
        fn should_register(state: &mut State) -> JoinAction {
            let action = match state.get(State::JOIN_WAKER) {
                JoinWakerState::Empty => JoinAction::Register,
                x => {
                    debug_assert_eq!(x, JoinWakerState::Waiting);
                    JoinAction::Reregister
                }
            };
            state.set(State::JOIN_WAKER, JoinWakerState::Registering);

            action
        }

        self.transition(|state| {
            // If the task has not completed, we can't take its join output.
            if test_dbg!(!state.get(State::COMPLETED)) {
                return should_register(state);
            }

            // If the task does not have output, we cannot take it.
            if test_dbg!(!state.get(State::HAS_OUTPUT)) {
                return should_register(state);
            }

            *state = state.with(State::HAS_OUTPUT, false);
            JoinAction::TakeOutput
        })
    }

    pub(super) fn join_waker_registered(&self) {
        self.transition(|state| {
            debug_assert_eq!(state.get(State::JOIN_WAKER), JoinWakerState::Registering);
            state
                .set(State::HAS_JOIN_HANDLE, true)
                .set(State::JOIN_WAKER, JoinWakerState::Waiting);
        })
    }

    pub(super) fn load(&self, order: Ordering) -> State {
        State(self.0.load(order))
    }

    /// Advance this task's state by running the provided
    /// `transition` function on the current [`State`].
    #[cfg_attr(test, track_caller)]
    fn transition<T>(&self, mut transition: impl FnMut(&mut State) -> T) -> T {
        let mut current = self.load(Acquire);
        loop {
            test_trace!("StateCell::transition; current:\n{}", current);
            let mut next = current;
            // Run the transition function.
            let res = transition(&mut next);

            if test_dbg!(current.0 == next.0) {
                return res;
            }

            test_trace!("StateCell::transition; next:\n{}", next);
            match self
                .0
                .compare_exchange_weak(current.0, next.0, AcqRel, Acquire)
            {
                Ok(_) => return res,
                Err(actual) => current = State(actual),
            }
        }
    }

    fn wait_for_join_waker(&self) {
        test_trace!("StateCell::wait_for_join_waker");
        let mut state = self.load(Acquire);
        let mut boff = Backoff::new();
        loop {
            state.set(State::JOIN_WAKER, JoinWakerState::Waiting);
            let next = state.with(State::JOIN_WAKER, JoinWakerState::Woken);
            match self
                .0
                .compare_exchange_weak(state.0, next.0, AcqRel, Acquire)
            {
                Ok(_) => return,
                Err(actual) => state = State(actual),
            }
            boff.spin();
        }
    }
}

impl fmt::Debug for StateCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.load(Relaxed).fmt(f)
    }
}

impl mycelium_bitfield::FromBits<usize> for JoinWakerState {
    type Error = core::convert::Infallible;

    /// The number of bits required to represent a value of this type.
    const BITS: u32 = 2;

    #[inline]
    fn try_from_bits(bits: usize) -> Result<Self, Self::Error> {
        match bits {
            b if b == Self::Registering as usize => Ok(Self::Registering),
            b if b == Self::Waiting as usize => Ok(Self::Waiting),
            b if b == Self::Empty as usize => Ok(Self::Empty),
            b if b == Self::Woken as usize => Ok(Self::Woken),
            _ => unsafe {
                // this should never happen unless the bitpacking code is broken
                unreachable_unchecked!("invalid join waker state {bits:#b}")
            },
        }
    }

    #[inline]
    fn into_bits(self) -> usize {
        self as u8 as usize
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
