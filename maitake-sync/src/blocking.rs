//! Synchronous (blocking) synchronization primitives.
//!
//! The core synchronization primitives in `maitake-sync`, such as
//! [`Mutex`](crate::Mutex),  [`RwLock`](crate::RwLock), and
//! [`WaitQueue`](crate::WaitQueue) are _asynchronous_. They are designed to be
//! used with [`core::task`] and [`core::future`], and when it is necessary to
//! wait for another task to complete some work for the current task to proceed,
//! `maitake`'s synchronization primitives wait by *yielding* to the
//! asynchronous task scheduler to allow another task to proceed.
//!
//! This module, on the other hand, provides _synchronous_ (or _blocking_)
//! synchronization primitives. Rather than yielding to the runtime, these
//! synchronization primitives will block the current CPU core (or thread, if
//! running in an environment with threads) until they are woken by other cores.
//! These synchronization primitives are, in some cases, necessary to implement
//! the async synchronization primitives that form `maitake-sync`'s core APIs.
//! They are also exposed publicly in this module so that they can be used in
//! other projects when a blocking-based synchronization primitive is needed.
//!
//! This module provides the following synchronization primitive types:
//!
//! - [`Mutex`]: a synchronous [mutual exclusion lock].
//! - [`RwLock`]: a synchronous [reader-writer lock].
//!
//! # overriding mutex implementations
//!
//! By default, the [`Mutex`] type uses a [`DefaultMutex`] as the underlying
//! blocking strategy. This type attempts to choose a suitable implementation
//! for the blocking mutex based on the currently available [feature flags]. When
//! the `std` feature is not enabled, this is typically a _[spinlock]_, which
//! waits for the lock to become available by _spinning_: repeatedly checking an
//! atomic value in a loop, executing [spin-loop hint instructions] until the
//! lock value changes. These spinlock implementations are represented by the
//! [`Spinlock`] and [`RwSpinlock`] types in the [`spin`](crate::spin) module.
//!
//! Spinlocks are simple to implement and, thanks to the Rust standard library
//! abstracting over atomic operations, portable. The default spinlock will work
//! on any platform with support for atomic compare-and-swap operations.[^1]
//! However, there are a number of reasons why a generic spinlock may not be
//! desirable for all use-cases. For example:
//!
//! - On single-core platforms, there *is* no concurrent thread of execution
//!   which can acquire and release a lock. If code running on a single-core
//!   system attempts to acquire a lock and finds that it is already locked,
//!   waiting for the lock to be released will never work, since the *same*
//!   thread of execution is holding the lock already. On such systems, any
//!   attempt to lock a mutex that is locked is guaranteed to be a deadlock.
//!   Therefore, code which can guarantee that it will only run on single-core
//!   CPUs may prefer to avoid the complexity of a "real" lock implementation
//!   altogether, and panic (or otherwise alert the programmer) rather than
//!   deadlocking when attempting to acquire a mutex that is already locked.
//! - In bare-metal code, the data protected by a mutex may be shared not only
//!   with concurrent threads of execution, but with [interrupt handlers] as
//!   well. When this is the case, it may be necessary for acquiring a lock to
//!   also disable interrupts while the lock is held (and re-enable then when
//!   the lock is released) to avoid racing with code that runs in an interrupt
//!   handler.
//! - While spinlocks are simple and portable, they are [not the most
//!   *efficient*][busy-wait]. A CPU core waiting on a spinlock draws power
//!   while in a spin loop, and is not executing any other tasks. Systems with a
//!   notion of a scheduler, whether provided by an operating system or
//!   implemented directly within a bare-metal program, may prefer to yield to
//!   the scheduler when waiting for a lock.
//! - Some hardware platforms provide mechanisms to optimize the performance of
//!   spinlocks, such as [Hardware Lock Elision] on x86 CPUs. When such features
//!   are available, using them can improve performance and efficiency.
//!
//! However, all of these alternative waiting strategies knowledge of either the
//! underlying hardware platform, the specific details of the system, or both. A
//! single-core "fake" spinlock naturally requires the knowledge that the
//! hardware platform is single-core, and hardware lock elision similarly
//! requires knowledge about platform-specific features. Similarly, disabling
//! interrupts may require application-specific as well as hardware-specific
//! code, especially if only certain interrupts need to be disabled. And, of
//! course, yielding to a scheduler requires scheduler-specific code. Since
//! `maitake-sync` is a platform-agnostic, generic library, it does not provide
//! its own implementations of such behaviors. Instead, users which need a
//! blocking strategy other than the default spinlock may *override* the default
//! behavior using the traits provided by the [`mutex-traits`] crate.
//!
//! The [`Mutex`] type in this module is generic over an additional `Lock` type
//! parameter, which represents the actual raw mutex implementation. This type
//! parameter defaults to the [`Spinlock`] type, but it can be overridden to an
//! arbitrary user-provided type. The [`mutex-traits`] crate provides two
//! traits representing a raw mutex, [`mutex_traits::ScopedRawMutex`] and
//! [`mutex_traits::RawMutex`]. These can be implemented to provide a custom
//! lock implementation. [`mutex_traits::RawMutex`] represents a generic
//! mutual-exclusion lock that can be freely locked and unlocked at any time,
//! while [`mutex_traits::ScopedRawMutex`] is a subset of [`RawMutex`] for which
//! locks can only be acquired and released for the duration of a closure. As
//! the functionality of [`RawMutex`] is a superset of [`ScopedRawMutex`], all
//! types which implement [`RawMutex`] also implement [`ScopedRawMutex`]. In
//! general, it is recommended for user lock types implement the more flexible
//! [`RawMutex`] trait rather than [`ScopedRawMutex`], if possible:
//! [`ScopedRawMutex`] exists to support more restricted lock types which
//! require scoped lock-and-unlock operations. Finally, the [`ConstInit`] trait
//! abstracts over `const fn` initialization of a raw mutex type, and is
//! required for `const fn` constructors with a custom raw mutex type.
//!
//! When the `Lock` type parameter implements [`ScopedRawMutex`], the [`Mutex`]
//! type provides the [`Mutex::with_lock`] and [`Mutex::try_with_lock`] methods,
//! which execute a closure with the mutex locked, and release the lock when the
//! closure returns. When the `Lock` type parameter also implements
//! [`RawMutex`], the [`Mutex`] type provides [`Mutex::lock`] and
//! [`Mutex::try_lock`] methods, which return a RAII [`MutexGuard`], similar to
//! the interface provided by [`std::sync::Mutex`]. The [`Mutex::new`] function
//! returns a [`Mutex`] using the default spinlock. To instead construct a
//! [`Mutex`] with a custom [`RawMutex`] implementation, use the
//! [`Mutex::new_with_raw_mutex`] function.
//!
//! Furthermore, many *async* synchronization primitives provided by this crate,
//! such as the [async `Mutex`](crate::Mutex), [async `RwLock`], [`WaitQueue`],
//! [`WaitMap`], and [`Semaphore`], internally depend on the blocking `Mutex`
//! for wait list synchronization. These types are *also* generic over a `Lock`
//! type parameter, and also provide `new_with_raw_mutex` constructors, such as
//! [`WaitQueue::new_with_raw_mutex`](crate::WaitQueue::new_with_raw_mutex). This allows
//! the blocking mutex used by these types to be overridden. The majority
//! `maitake-sync`'s async synchronization types only require the `Lock` type to
//! implement [`ScopedRawMutex`]. However, the [`Semaphore`] and [async
//! `RwLock`] require the more permissive [`RawMutex`] trait.
//!
//! The [`mutex` crate] provides a number of types implementing [`RawMutex`] and
//! [`ScopedRawMutex`], including adapters for compatibility with the
//! [`lock_api`] and [`critical-section`] crates.
//!
//! Similarly to the [`RawMutex`] trait, the blocking [`RwLock`] type in this
//! module is generic over a `Lock` type parameter, which must implement the
//! [`RawRwLock`] trait. This allows the `RwLock`'s blocking behavior to be
//! overridden similarly to [`Mutex`].
//!
//! [^1]: Including those where "atomics" are implemented by the
//!     `portable-atomic` crate, as described
//!     [here](crate#support-for-atomic-operations).
//!
//! [mutual exclusion lock]: https://en.wikipedia.org/wiki/Mutual_exclusion
//! [reader-writer lock]:
//!     https://en.wikipedia.org/wiki/Readers%E2%80%93writer_lock
//! [spinlock]: https://en.wikipedia.org/wiki/Spinlock
//! [spin-loop hint instructions]: core::hint::spin_loop
//! [`Spinlock`]: crate::spin::Spinlock
//! [`RwSpinlock`]: crate::spin::RwSpinlock
//! [interrupt handler]: https://en.wikipedia.org/wiki/Interrupt_handler
//! [busy-wait]: https://en.wikipedia.org/wiki/Spinlock#Alternatives
//! [Hardware Lock Elision]:
//!     https://en.wikipedia.org/wiki/Transactional_Synchronization_Extensions#HLE
//! [`WaitQueue`]: crate::WaitQueue
//! [`WaitMap`]: crate::WaitMap
//! [`Semaphore`]: crate::Semaphore
//! [async `RwLock`]: crate::RwLock
//! [`mutex-traits`]: https://docs.rs/mutex-traits
//! [`mutex` crate]: https://crates.io/crates/mutex
//! [`lock_api`]: https://crates.io/crates/lock_api
//! [`critical-section`]: https://crates.io/crates/critical-section
//! [`std::sync::Mutex`]:
//!     https://doc.rust-lang.org/stable/std/sync/struct.Mutex.html
//! [feature flags]: crate#features
mod default_mutex;
pub(crate) mod mutex;
pub(crate) mod rwlock;

pub use self::{mutex::*, rwlock::*};

pub use default_mutex::DefaultMutex;

pub use mutex_traits::ConstInit;
