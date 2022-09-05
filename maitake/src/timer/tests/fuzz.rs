use super::*;
use proptest::collection::vec;
use proptest::{prop_oneof, strategy::Strategy};

#[derive(Debug)]
enum FuzzAction {
    Spawn(Ticks),
    Advance(Ticks),
    // TODO(eliza): add an action that cancels a sleep group...
}
/// Miri uses a significant amount of time and memory, meaning that
/// running 256 property tests (the default test-pass count) * (0..100)
/// vec elements (the default proptest vec length strategy) causes the
/// CI running to OOM (I think). In local testing, this required up
/// to 11GiB of resident memory with the default strategy, at the
/// time of this change.
///
/// In the future, it may be desirable to have an "override" feature
/// to use a larger test case set for more exhaustive local miri testing,
/// where the time and memory limitations are less restrictive than in CI.
#[cfg(miri)]
const MAX_FUZZ_ACTIONS: usize = 10;

/// The default range for proptest's vec strategy is 0..100.
#[cfg(not(miri))]
const MAX_FUZZ_ACTIONS: usize = 100;

fn fuzz_action_strategy() -> impl Strategy<Value = FuzzAction> {
    // don't spawn tasks that sleep for 0 ticks
    const MIN_SLEEP_TICKS: u64 = 1;

    // don't overflow the timer's elapsed counter
    const MAX_ADVANCE: u64 = u64::MAX / MAX_FUZZ_ACTIONS as u64;

    prop_oneof![
        (MIN_SLEEP_TICKS..Timer::MAX_SLEEP_TICKS).prop_map(FuzzAction::Spawn),
        (0..MAX_ADVANCE).prop_map(FuzzAction::Advance),
    ]
}

proptest::proptest! {
    #[test]
    fn fuzz_timer(actions in vec(fuzz_action_strategy(), 0..MAX_FUZZ_ACTIONS)) {
        static TIMER: Timer = Timer::new();
        static FUZZ_RUNS: AtomicUsize = AtomicUsize::new(1);

        TIMER.reset();
        let mut test = SleepGroupTest::new(&TIMER);
        let _span = span!(Level::INFO, "fuzz_timer", iteration = FUZZ_RUNS.fetch_add(1, Ordering::Relaxed)).entered();
        info!(?actions);

        for action in actions {
            match action {
                FuzzAction::Spawn(ticks) => test.spawn_group(ticks, 1),
                FuzzAction::Advance(ticks) => test.advance(ticks),
            }
        }

        test.assert();
    }
}
