use crate::arch;
use core::{
    cell::Cell,
    cmp,
    future::Future,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::*},
};
use maitake::{
    scheduler::{self, StaticScheduler, Stealer},
    time,
};
use mycelium_util::{fmt, sync::InitOnce};
use rand::Rng;

pub use maitake::task::JoinHandle;

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

    /// Used to select the index of the next core to steal work from.
    ///
    /// Selecting a random core index when work-stealing helps ensure we don't
    /// have a situation where all idle steal from the first available worker,
    /// resulting in other cores ending up with huge queues of idle tasks while
    /// the first core's queue is always empty.
    ///
    /// This is *not* a cryptographically secure random number generator, since
    /// randomness of this value is not required for security. Instead, it just
    /// helps ensure a good distribution of load. Therefore, we use a fast,
    /// non-cryptographic RNG.
    rng: rand_xoshiro::Xoroshiro128PlusPlus,
}

struct Runtime {
    cores: [InitOnce<StaticScheduler>; MAX_CORES],

    /// Global injector queue for spawning tasks on any `Core` instance.
    injector: scheduler::Injector<&'static StaticScheduler>,
    initialized: AtomicUsize,
}

/// 512 CPU cores ought to be enough for anybody...
pub const MAX_CORES: usize = 512;

pub static TIMER: time::Timer = time::Timer::new(arch::interrupt::TIMER_INTERVAL);

static RUNTIME: Runtime = {
    // This constant is used as an array initializer; the clippy warning that it
    // contains interior mutability is not actually a problem here, since we
    // *want* a new instance of the value for each array element created using
    // the `const`.
    #[allow(clippy::declare_interior_mutable_const)]
    const UNINIT_SCHEDULER: InitOnce<StaticScheduler> = InitOnce::uninitialized();

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
    SCHEDULER.with(|scheduler| {
        if let Some(scheduler) = scheduler.get() {
            scheduler.spawn(future)
        } else {
            // no scheduler is running on this core.
            RUNTIME.injector.spawn(future)
        }
    })
}

/// Initialize the kernel runtime.
pub fn init() {
    time::set_global_timer(&TIMER).expect("`rt::init` should only be called once!");

    tracing::info!("kernel runtime initialized");
}

pub const DUMP_RT: crate::shell::Command = crate::shell::Command::new("rt")
    .with_help("print the kernel's async runtime")
    .with_fn(|_| {
        tracing::info!(runtime = ?RUNTIME);
        Ok(())
    });

static SCHEDULER: arch::LocalKey<Cell<Option<&'static StaticScheduler>>> =
    arch::LocalKey::new(|| Cell::new(None));

impl Core {
    #[must_use]
    pub fn new() -> Self {
        let (id, scheduler) = RUNTIME.new_scheduler();
        tracing::info!(core = id, "initialized task scheduler");
        Self {
            scheduler,
            id,
            rng: arch::seed_rng(),
            running: AtomicBool::new(false),
        }
    }

    /// Runs one tick of the kernel main loop on this core.
    ///
    /// Returns `true` if this core has more work to do, or `false` if it does not.
    pub fn tick(&mut self) -> bool {
        // drive the task scheduler
        let tick = self.scheduler.tick();

        // turn the timer wheel if it wasn't turned recently and no one else is
        // holding a lock, ensuring any pending timer ticks are consumed.
        TIMER.advance_ticks(0);

        // if there are remaining tasks to poll, continue without stealing.
        if tick.has_remaining {
            return true;
        }

        // if there are no tasks remaining in this core's run queue, try to
        // steal new tasks from the distributor queue.
        let stolen = self.try_steal();
        if stolen > 0 {
            tracing::debug!(tick.stolen = stolen);
            // if we stole tasks, we need to keep ticking
            return true;
        }

        // if we have no remaining woken tasks, and we didn't steal any new
        // tasks, this core can sleep until an interrupt occurs.
        false
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
    pub fn run(&mut self) {
        struct CoreGuard;
        impl Drop for CoreGuard {
            fn drop(&mut self) {
                SCHEDULER.with(|scheduler| scheduler.set(None));
            }
        }

        let _span = tracing::info_span!("core", id = self.id).entered();
        if self
            .running
            .compare_exchange(false, true, AcqRel, Acquire)
            .is_err()
        {
            tracing::error!("this core is already running!");
            return;
        }

        SCHEDULER.with(|scheduler| scheduler.set(Some(self.scheduler)));
        let _unset = CoreGuard;

        tracing::info!("started kernel main loop");

        loop {
            // tick the scheduler until it indicates that it's out of tasks to run.
            if self.tick() {
                continue;
            }

            // check if this core should shut down.
            if !self.is_running() {
                tracing::info!(core = self.id, "stop signal received, shutting down");
                return;
            }

            // if we have no tasks to run, we can sleep until an interrupt
            // occurs.
            arch::wait_for_interrupt();
        }
    }

    fn try_steal(&mut self) -> usize {
        // don't try stealing work infinitely many times if all potential
        // victims' queues are empty or busy.
        const MAX_STEAL_ATTEMPTS: usize = 16;
        // chosen arbitrarily!
        const MAX_STOLEN_PER_TICK: usize = 256;

        // first, try to steal from the injector queue.
        if let Ok(injector) = RUNTIME.injector.try_steal() {
            return injector.spawn_n(&self.scheduler, MAX_STOLEN_PER_TICK);
        }

        // if the injector queue is empty or someone else is stealing from it,
        // try to find another worker to steal from.
        let mut attempts = 0;
        while attempts < MAX_STEAL_ATTEMPTS {
            let active_cores = RUNTIME.active_cores();

            // if the stealing core is the only active core, there's no one else
            // to steal from, so bail.
            if active_cores <= 1 {
                break;
            }

            // randomly pick a potential victim core to steal from.
            let victim_idx = self.rng.gen_range(0..active_cores);

            // we can't steal tasks from ourself.
            if victim_idx == self.id {
                continue;
            }

            // found a core to steal from
            if let Some(victim) = RUNTIME.try_steal_from(victim_idx) {
                let num_steal = cmp::min(victim.initial_task_count() / 2, MAX_STOLEN_PER_TICK);
                return victim.spawn_n(&self.scheduler, num_steal);
            } else {
                attempts += 1;
            }
        }

        // try the injector queue again if we couldn't find anything else
        if let Ok(injector) = RUNTIME.injector.try_steal() {
            injector.spawn_n(&self.scheduler, MAX_STOLEN_PER_TICK)
        } else {
            0
        }
    }
}

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

// === impl Runtime ===

impl Runtime {
    fn active_cores(&self) -> usize {
        self.initialized.load(Acquire)
    }

    fn new_scheduler(&self) -> (usize, &StaticScheduler) {
        let next = self.initialized.fetch_add(1, AcqRel);
        assert!(next < MAX_CORES);
        let scheduler = self.cores[next].init(StaticScheduler::new());
        (next, scheduler)
    }

    fn try_steal_from(
        &'static self,
        idx: usize,
    ) -> Option<Stealer<'static, &'static StaticScheduler>> {
        self.cores[idx].try_get()?.try_steal().ok()
    }
}

impl fmt::Debug for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cores = self.active_cores();
        f.debug_struct("Runtime")
            .field("active_cores", &cores)
            .field("cores", &&self.cores[..cores])
            .field("injector", &self.injector)
            .finish()
    }
}
