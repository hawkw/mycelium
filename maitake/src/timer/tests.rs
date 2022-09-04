use super::*;
use crate::scheduler::Scheduler;
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct SleepGroupTest {
    scheduler: Scheduler,
    timer: &'static Timer,
    now: Ticks,
    groups: BTreeMap<Ticks, SleepGroup>,
}

struct SleepGroup {
    duration: Ticks,
    t_start: Ticks,
    tasks: usize,
    count: Arc<AtomicUsize>,
}

impl SleepGroupTest {
    fn new(timer: &'static Timer) -> Self {
        crate::util::test::trace_init_with_default("info,maitake::timer=trace");
        Self {
            scheduler: Scheduler::new(),
            now: 0,
            groups: BTreeMap::new(),
            timer,
        }
    }

    fn spawn_group(&mut self, duration: Ticks, tasks: usize) {
        let count = Arc::new(AtomicUsize::new(tasks));
        for i in 0..tasks {
            let count = count.clone();
            let timer = self.timer;
            self.scheduler.spawn(async move {
                info!("sleep group task {i} sleeping for {duration} ticks");
                timer.sleep(duration).await;
                info!("sleep group task {i} slept for {duration} ticks!");
                count.fetch_sub(1, Ordering::SeqCst);
            });
        }
        info!("spawned {tasks} tasks to sleep for {duration} ticks");
        self.groups.insert(
            self.now + duration,
            SleepGroup {
                duration,
                t_start: self.now,
                tasks,
                count,
            },
        );
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
            },
        ) in self.groups.iter()
        {
            let active = count.load(Ordering::SeqCst);
            let elapsed = t_1 - t_start;
            if t_done <= t_1 {
                assert_eq!(
                    active, 0,
                    "{tasks} tasks sleeping for {duration} starting at tick \
                    {t_start} should have completed by tick {t_1} \
                    ({elapsed} ticks have elapsed)",
                );
            } else {
                assert_eq!(
                    active, *tasks,
                    "{tasks} tasks sleeping for {duration} starting at tick \
                    {t_start} should not have completed by tick {t_1} \
                    ({elapsed} ticks have elapsed)"
                );
            }
        }
    }

    #[track_caller]
    fn advance(&mut self, ticks: Ticks) {
        let t_0 = self.now;
        self.now += ticks;
        // how many tasks are expected to complete?
        let expected_complete: usize = self
            .groups
            .iter_mut()
            .take_while(|(&t, _)| t <= self.now)
            .map(|(_, g)| std::mem::replace(&mut g.tasks, 0))
            .sum();

        // advance the timer.
        self.timer.advance(ticks);

        let completed = self.scheduler.tick().completed;

        info!(ticks, t_0, t_1 = self.now, completed, "advancing timer");

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
    static TIMER: Timer = Timer::new();
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
}

#[test]
fn wheel_indices() {
    let core = Core::new();
    for ticks in 0..64 {
        assert_eq!(
            core.wheel_index(ticks),
            0,
            "Core::wheel_index({ticks}) == 0"
        )
    }

    for wheel in 1..Core::WHEELS as usize {
        for slot in wheel..wheel::SLOTS {
            let ticks = (slot * usize::pow(wheel::SLOTS, wheel as u32)) as u64;
            assert_eq!(
                core.wheel_index(ticks),
                wheel,
                "Core::wheel_index({ticks}) == {wheel}"
            );

            if slot > wheel {
                let ticks = ticks - 1;
                assert_eq!(
                    core.wheel_index(ticks),
                    wheel,
                    "Core::wheel_index({ticks}) == {wheel}"
                );
            }

            if slot < 64 {
                let ticks = ticks + 1;
                assert_eq!(
                    core.wheel_index(ticks),
                    wheel,
                    "Core::wheel_index({ticks}) == {wheel}"
                );
            }
        }
    }
}
