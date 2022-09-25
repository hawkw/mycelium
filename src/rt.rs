use core::{
    cmp,
    future::Future,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::*},
};
use maitake::scheduler::{self, StaticScheduler};
pub use maitake::task::JoinHandle;
use mycelium_util::sync::InitOnce;

/// A kernel runtime for a single core.
pub struct Core {
    /// The task scheduler for this core.
    scheduler: &'static StaticScheduler,

    /// This core's ID.
    ///
    /// ID 0 is the first CPU core started when the system boots.
    id: usize,

    /// Set to `false` if this core should shut down.
    running: AtomicBool,
}

struct Runtime {
    cores: [InitOnce<StaticScheduler>; MAX_CORES],

    /// Global injector queue for spawning tasks on any `Core` instance.
    injector: Injector<&'static StaticScheduler>,
    initialized: AtomicUsize,
}

/// 512 CPU cores ought to be enough for anybody...
pub const MAX_CORES: usize = 512;

static RUNTIME: Runtime = {
    const UNINIT_SCHEDULER: InitOnce = InitOnce::uninitialized();
    Runtime {
        cores: [UNINIT_SCHEDULER; MAX_CORES],
        initialized: AtomicUsize::new(0),
        injector: {
            static STUB_TASK: scheduler::TaskStub = scheduler::TaskStub::new();
            unsafe { scheduler::Injector::new_with_static_stub(&STUB_TASK) }
        },
    }
};

/// Spawn a task on Mycelium's global runtime.
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    RUNTIME.injector.spawn(future)
}

impl Core {
    pub fn new() -> Self {
        let (id, scheduler) = RUNTIME.new_scheduler();
        tracing::info!(core = id, "initialized task scheduler");
        Self {
            scheduler,
            id,
            running: AtomicBool::new(false),
        }
    }

    /// Runs one tick of the kernel main loop on this core.
    pub fn tick(&self) {
        // drive the task scheduler
        let tick = self.scheduler.tick();

        // turn the timer wheel if it wasn't turned recently and no one else is
        // holding a lock, ensuring any pending timer ticks are consumed.
        crate::arch::tick_timer();

        // if there are no tasks remaining in this core's run queue, try to
        // steal new tasks from the distributor queue.
        // TODO(eliza): add a "check distributor first interval" of some kind to
        // ensure new tasks are eventually spawned on a core...
        let stolen = if !tick.has_remaining {
            RUNTIME.try_steal(self)
        } else {
            0
        };

        if tick.polled > 0 || stolen > 0 {
            tracing::trace!(
                core = self.id,
                tick.polled,
                tick.completed,
                tick.spawned,
                tick.woken_external,
                tick.woken_internal,
                tick.has_remaining,
                tick.stolen = stolen,
            );
        }
    }

    /// Returns `true` if this core is currently running.
    #[inline]
    pub fn is_running(&self) -> bool {
        self.running.load(Acquire)
    }

    /// Stops this core if it is currently running.
    ///
    /// # Returns
    ///
    /// - `true` if this core was running and is now stopping
    /// - `false` if this core was not running.
    pub fn stop(&self) -> bool {
        let was_running = self
            .running
            .compare_exchange(true, false, AcqRel, Acquire)
            .is_ok();
        tracing::info!(core = self.id, core.was_running = was_running, "stopping");
        was_running
    }

    /// Run this core until [`Core::stop`] is called.
    pub fn run(&self) {
        let _span = tracing::info_span!("core", id = self.id).entered();
        if self
            .running
            .compare_exchange(false, true, AcqRel, Acquire)
            .is_err()
        {
            tracing::error!("this core is already running!");
            return;
        }

        tracing::info!("started kernel main loop");

        while self.is_running() {
            self.tick();
        }

        tracing::info!("stop signal received, shutting down");
    }
}

// === impl Schedulers ===

impl Runtime {
    fn try_steal(&'static self, core: &Core) -> usize {
        // chosen arbitrarily!
        const MAX_STOLEN_PER_TICK: usize = 256;

        // first, try to steal from the injector queue.
        if let Some(stealer) = self.injector.try_steal() {
            return stealer.spawn_n(&core.scheduler, MAX_STOLEN_PER_TICK);
        }

        // if the injector queue is empty or someone else is stealing from it,
        // try to find another worker to steal from.
        self.schedulers()
            .find_map(|(id, worker)| {
                // we can't steal tasks from ourself!
                if id == core.id {
                    return None;
                }
                // is this worker's run queue free?
                worker.try_steal().ok()
            })
            .map(|stealer| {
                // steal up to half of the tasks in the target worker's run queue,
                // or `MAX_STOLEN_PER_TICK` if the target run queue has more than
                // that many tasks in it.
                let max_stolen = cmp::min(MAX_STOLEN_PER_TICK, worker.initial_task_count() / 2);
                return stealer.spawn_n(max_stolen, self.scheduler);
            })
            .unwrap_or(0)
    }

    fn new_scheduler<'a>(&'a self) -> (usize, &'a StaticScheduler) {
        let next = self.initialized.fetch_add(1, AcqRel);
        assert!(next < MAX_CORES);
        let scheduler = self.cores[next].init(StaticScheduler::new);
        (next, scheduler)
    }

    fn schedulers<'a>(&'a self) -> impl Iterator<Item = (usize, &'a StaticScheduler)> + 'a {
        let initialized = self.initialized.load(Acquire);
        self.cores[..initialized]
            .iter()
            .enumerate()
            .filter_map(InitOnce::try_get)
    }
}
