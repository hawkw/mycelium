use core::{
    future::Future,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::*},
};
use maitake::scheduler::{self, StaticScheduler};
pub use maitake::task::JoinHandle;

/// A kernel runtime for a single core.
pub struct Core {
    /// The task scheduler for this core.
    scheduler: &'static StaticScheduler,
    /*
    /// A reference to the global system timer.
    timer: &'static Timer,
    */
    /// This core's ID.
    ///
    /// ID 0 is the first CPU core started when the system boots.
    id: usize,

    /// Set to `false` if this core should shut down.
    running: AtomicBool,
}

/// Work-stealing distributor queue.
static DISTRIBUTOR: scheduler::Distributor<&'static StaticScheduler> = {
    static STUB_TASK: scheduler::TaskStub = scheduler::TaskStub::new();
    unsafe { scheduler::Distributor::new_with_static_stub(&STUB_TASK) }
};

/// Spawn a task on Mycelium's global runtime.
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    DISTRIBUTOR.spawn(future)
}

impl Core {
    pub fn new(scheduler: &'static StaticScheduler /* timer: &'static Timer */) -> Self {
        static CORE_IDS: AtomicUsize = AtomicUsize::new(0);
        let id = CORE_IDS.fetch_add(1, Relaxed);
        Self {
            scheduler,
            // timer,
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
            DISTRIBUTOR.try_steal(&self.scheduler).unwrap_or(0)
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
