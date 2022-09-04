use super::*;
use crate::scheduler::Scheduler;
use core::sync::atomic::{AtomicUsize, Ordering};

fn spawn_sleep_group(
    scheduler: &Scheduler,
    timer: &'static Timer,
    count: &'static AtomicUsize,
    ticks: Ticks,
) {
    let tasks = count.load(Ordering::SeqCst);
    for i in 0..tasks {
        scheduler.spawn(async move {
            info!("sleep group task {i} sleeping for {ticks} ticks");
            timer.sleep(ticks).await;
            info!("sleep group task {i} slept for {ticks} ticks!");
            count.fetch_sub(1, Ordering::SeqCst);
        });
    }
    info!("spawned {tasks} tasks to sleep for {ticks} ticks");
}

#[test]
fn timer_basically_works() {
    crate::util::trace_init();

    static TIMER: Timer = Timer::new();
    static SLEEP_GROUP_1: AtomicUsize = AtomicUsize::new(3);
    static SLEEP_GROUP_2: AtomicUsize = AtomicUsize::new(4);
    static SLEEP_GROUP_3: AtomicUsize = AtomicUsize::new(5);
    let scheduler = Scheduler::new();

    spawn_sleep_group(&scheduler, &TIMER, &SLEEP_GROUP_1, 100);
    spawn_sleep_group(&scheduler, &TIMER, &SLEEP_GROUP_2, 65535);
    spawn_sleep_group(&scheduler, &TIMER, &SLEEP_GROUP_3, 6_000_000);

    assert_eq!(SLEEP_GROUP_1.load(Ordering::SeqCst), 3);
    assert_eq!(SLEEP_GROUP_2.load(Ordering::SeqCst), 4);
    assert_eq!(SLEEP_GROUP_3.load(Ordering::SeqCst), 5);

    // first tick --- timer is still at zero
    let tick = scheduler.tick();
    assert_eq!(tick.completed, 0);
    assert_eq!(SLEEP_GROUP_1.load(Ordering::SeqCst), 3);
    assert_eq!(SLEEP_GROUP_2.load(Ordering::SeqCst), 4);
    assert_eq!(SLEEP_GROUP_3.load(Ordering::SeqCst), 5);

    // advance the timer by 50 ticks.
    TIMER.advance(50);
    let tick = scheduler.tick();
    assert_eq!(tick.completed, 0);
    assert_eq!(SLEEP_GROUP_1.load(Ordering::SeqCst), 3);
    assert_eq!(SLEEP_GROUP_2.load(Ordering::SeqCst), 4);
    assert_eq!(SLEEP_GROUP_3.load(Ordering::SeqCst), 5);

    // advance the timer by 50 more ticks. the first sleep group should
    // complete.
    TIMER.advance(50);
    let tick = scheduler.tick();
    assert_eq!(tick.completed, 3);
    assert_eq!(SLEEP_GROUP_1.load(Ordering::SeqCst), 0);
    assert_eq!(SLEEP_GROUP_2.load(Ordering::SeqCst), 4);
    assert_eq!(SLEEP_GROUP_3.load(Ordering::SeqCst), 5);

    TIMER.advance(60000);
    let tick = scheduler.tick();
    assert_eq!(tick.completed, 0);
    assert_eq!(SLEEP_GROUP_1.load(Ordering::SeqCst), 0);
    assert_eq!(SLEEP_GROUP_2.load(Ordering::SeqCst), 4);
    assert_eq!(SLEEP_GROUP_3.load(Ordering::SeqCst), 5);

    // overshoot sleep group 2. the timers should still fire.
    TIMER.advance(56000);
    let tick = scheduler.tick();
    assert_eq!(tick.completed, 4);
    assert_eq!(SLEEP_GROUP_1.load(Ordering::SeqCst), 0);
    assert_eq!(SLEEP_GROUP_2.load(Ordering::SeqCst), 0);
    assert_eq!(SLEEP_GROUP_3.load(Ordering::SeqCst), 5);

    TIMER.advance(50000);
    let tick = scheduler.tick();
    assert_eq!(tick.completed, 0);
    assert_eq!(SLEEP_GROUP_1.load(Ordering::SeqCst), 0);
    assert_eq!(SLEEP_GROUP_2.load(Ordering::SeqCst), 0);
    assert_eq!(SLEEP_GROUP_3.load(Ordering::SeqCst), 5);

    TIMER.advance(6_000_000);
    let tick = scheduler.tick();
    assert_eq!(tick.completed, 5);
    assert_eq!(SLEEP_GROUP_1.load(Ordering::SeqCst), 0);
    assert_eq!(SLEEP_GROUP_2.load(Ordering::SeqCst), 0);
    assert_eq!(SLEEP_GROUP_3.load(Ordering::SeqCst), 0);
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
