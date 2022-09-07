use super::*;
use crate::scheduler::Scheduler;
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use proptest::{collection::vec, proptest};

struct SleepGroupTest {
    scheduler: Scheduler,
    timer: &'static Timer,
    now: Ticks,
    groups: BTreeMap<Ticks, SleepGroup>,
    next_id: usize,
}

struct SleepGroup {
    duration: Ticks,
    t_start: Ticks,
    tasks: usize,
    count: Arc<AtomicUsize>,
    id: usize,
}

impl SleepGroupTest {
    fn new(timer: &'static Timer) -> Self {
        crate::util::test::trace_init_with_default("info,maitake::timer=trace");
        Self {
            scheduler: Scheduler::new(),
            now: 0,
            groups: BTreeMap::new(),
            timer,
            next_id: 0,
        }
    }

    fn spawn_group(&mut self, duration: Ticks, tasks: usize) {
        self.next_id += 1;
        let count = Arc::new(AtomicUsize::new(tasks));
        let id = self.next_id;
        for i in 0..tasks {
            let count = count.clone();
            let timer = self.timer;
            self.scheduler.spawn(async move {
                info!(task.group = id, task = i, "sleeping for {duration} ticks");
                timer.sleep_ticks(duration).await;
                info!(task.group = id, task = i, "slept for {duration} ticks!");
                count.fetch_sub(1, Ordering::SeqCst);
            });
        }
        info!(
            group = id,
            group.tasks = tasks,
            "spawned sleep group to sleep for {duration} ticks"
        );
        self.groups.insert(
            self.now + duration,
            SleepGroup {
                duration,
                t_start: self.now,
                tasks,
                count,
                id,
            },
        );
        // eagerly poll the spawned group to ensure they are added to the wheel.
        // XXX(eliza): is this correct behavior? or should the time start
        // when the sleep is _created_ rather than first polled? this would mean
        // a lock-free way to get the "current time" from the timer...
        let tick = self.scheduler.tick();
        assert_eq!(
            tick.completed, 0,
            "no tasks should complete if the timer has not advanced"
        );
    }

    #[track_caller]
    fn assert_all_complete(&self) {
        let t_1 = self.now;
        for (
            &t_done,
            SleepGroup {
                ref count,
                duration,
                tasks,
                t_start,
                id,
            },
        ) in self.groups.iter()
        {
            let active = count.load(Ordering::SeqCst);
            let elapsed = t_1 - t_start;
            assert!(t_done <= t_1);
            assert_eq!(
                *tasks, 0,
                "test expected sleep group {id} to not have completed by {t_1}, \
                but `assert_all_complete` was called"
            );
            assert_eq!(
                active, 0,
                "sleep group {id} with {tasks} tasks sleeping for {duration} \
                starting at tick {t_start} should have completed by tick {t_1} \
                ({elapsed} ticks have elapsed)",
            );
        }
    }

    #[track_caller]
    fn assert(&self) {
        let t_1 = self.now;
        for (
            &t_done,
            SleepGroup {
                ref count,
                duration,
                tasks,
                t_start,
                id,
            },
        ) in self.groups.iter()
        {
            let active = count.load(Ordering::SeqCst);
            let elapsed = t_1 - t_start;
            if t_done <= t_1 {
                assert_eq!(
                    active, 0,
                    "sleep group {id} with {tasks} tasks sleeping for {duration} \
                    starting at tick {t_start} should have completed by tick {t_1} \
                    ({elapsed} ticks have elapsed)",
                );
            } else {
                assert_eq!(
                    active, *tasks,
                    "sleep group {id} with {tasks} tasks sleeping for {duration} \
                    starting at tick {t_start} should *not* have completed by \
                    tick {t_1} ({elapsed} ticks have elapsed)"
                );
            }
        }
    }

    #[track_caller]
    fn advance(&mut self, ticks: Ticks) {
        let t_0 = self.now;
        self.now += ticks;
        info!("");
        let _span = span!(Level::INFO, "advance", ticks, from = t_0, to = self.now).entered();
        info!("advancing test timer to {}", self.now);
        // how many tasks are expected to complete?
        let expected_complete: usize = self
            .groups
            .iter_mut()
            .take_while(|(&t, _)| t <= self.now)
            .map(|(_, g)| std::mem::replace(&mut g.tasks, 0))
            .sum();

        // advance the timer.
        self.timer.advance_ticks(ticks);

        let completed = self.scheduler.tick().completed;

        info!(completed, "advanced test timer");
        info!("");

        self.assert();

        assert_eq!(
            completed,
            expected_complete,
            "expected {expected_complete} tasks to complete when advancing \
             the timer from {t_0} to {t_1}",
            t_1 = self.now,
        );
    }
}

#[test]
fn timer_basically_works() {
    static TIMER: Timer = Timer::new(Duration::from_secs(1));
    let mut test = SleepGroupTest::new(&TIMER);

    test.spawn_group(100, 2);
    test.spawn_group(65535, 3);
    test.spawn_group(6_000_000, 4);

    // first tick --- timer is still at zero
    let tick = test.scheduler.tick();
    assert_eq!(tick.completed, 0);
    test.assert();

    // advance the timer by 50 ticks.
    test.advance(50);

    // advance the timer by 50 more ticks. the first sleep group should
    // complete.
    test.advance(50);

    // no tasks should complete.
    test.advance(60000);

    // overshoot sleep group 2. the timers should still fire.
    test.advance(56000);

    // no tasks should complete
    test.advance(50000);

    // the last sleep group should complete
    test.advance(6_000_000);

    test.assert_all_complete();
}

#[test]
fn schedule_after_start() {
    static TIMER: Timer = Timer::new(Duration::from_secs(1));
    let mut test = SleepGroupTest::new(&TIMER);

    test.spawn_group(100, 2);
    test.spawn_group(70_000, 3);

    // first tick --- timer is still at zero
    let tick = test.scheduler.tick();
    assert_eq!(tick.completed, 0);
    test.assert();

    // advance the timer by 50 ticks.
    test.advance(50);

    test.spawn_group(100, 3);

    // advance the timer by 50 more ticks. the first sleep group should
    // complete, but the second 100-tick group should not.
    test.advance(50);

    // the second 100-tick group should complete.
    test.advance(10_000);

    test.spawn_group(70_100, 4);

    // the first 70,000-tick group should complete.
    test.advance(60_000);

    // no tasks should complete.
    test.advance(30_000);

    test.spawn_group(10_000, 2);

    // every group should complete.
    test.advance(40_000);

    test.assert_all_complete();
}

#[test]
fn max_sleep() {
    static TIMER: Timer = Timer::new(Duration::from_secs(1));
    let mut test = SleepGroupTest::new(&TIMER);

    test.spawn_group(wheel::Core::MAX_SLEEP_TICKS, 2);
    test.spawn_group(100, 3);

    // first tick --- timer is still at zero
    let tick = test.scheduler.tick();
    assert_eq!(tick.completed, 0);
    test.assert();

    // advance the timer by 100 ticks.
    test.advance(100);

    test.advance(wheel::Core::MAX_SLEEP_TICKS / 2);

    test.spawn_group(wheel::Core::MAX_SLEEP_TICKS, 1);

    test.advance(wheel::Core::MAX_SLEEP_TICKS / 2);

    test.advance(wheel::Core::MAX_SLEEP_TICKS);

    test.assert_all_complete();
}

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
        (MIN_SLEEP_TICKS..wheel::Core::MAX_SLEEP_TICKS).prop_map(FuzzAction::Spawn),
        (0..MAX_ADVANCE).prop_map(FuzzAction::Advance),
    ]
}

proptest! {
    #[test]
    fn fuzz_timer(actions in vec(fuzz_action_strategy(), 0..MAX_FUZZ_ACTIONS)) {
        static TIMER: Timer = Timer::new(Duration::from_secs(1));
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
        info!("iteration done\n\n");
    }
}
