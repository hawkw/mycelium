use super::Msr;
use alloc::boxed::Box;
use core::{
    arch::asm,
    marker::PhantomPinned,
    pin::Pin,
    ptr,
    sync::atomic::{AtomicPtr, AtomicUsize, Ordering},
};
use mycelium_util::{fmt, sync::Lazy};

#[repr(C)]
#[derive(Debug)]
pub struct GsLocalData {
    /// This *must* be the first field of the local data struct, because we read
    /// from `gs:0x0` to get the local data's address.
    _self: *const Self,
    magic: usize,
    /// Because this struct is self-referential, it may not be `Unpin`.
    _must_pin: PhantomPinned,
    /// Arbitrary user data.
    ///
    // TODO(eliza): consider storing this in some kind of heap allocated tree
    // so that it's growable?
    userdata: [AtomicPtr<()>; MAX_LOCAL_KEYS],
}

pub struct LocalKey<T> {
    idx: Lazy<usize>,
    initializer: fn() -> T,
}

pub const MAX_LOCAL_KEYS: usize = 64;

impl GsLocalData {
    // coffee is magic
    const MAGIC: usize = 0xC0FFEE;
    const fn new() -> Self {
        #[allow(clippy::declare_interior_mutable_const)] // array initializer
        const LOCAL_SLOT_INIT: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
        Self {
            _self: ptr::null(),
            _must_pin: PhantomPinned,
            magic: Self::MAGIC,
            userdata: [LOCAL_SLOT_INIT; MAX_LOCAL_KEYS],
        }
    }

    #[must_use]
    pub fn try_current() -> Option<Pin<&'static Self>> {
        if !Self::has_magic() {
            return None;
        }
        unsafe {
            let ptr: *const Self;
            asm!("mov {}, gs:0x0", out(reg) ptr);
            debug_assert_eq!(
                (*ptr).magic,
                Self::MAGIC,
                "weird magic mismatch, this should never happen??"
            );
            Some(Pin::new_unchecked(&*ptr))
        }
    }

    #[track_caller]
    #[must_use]
    pub fn current() -> Pin<&'static Self> {
        Self::try_current()
            .expect("GsLocalData::current() called before local data was initialized on this core!")
    }

    pub fn with<T, U>(&self, key: &LocalKey<T>, f: impl FnOnce(&T) -> U) -> U {
        let idx = *key.idx.get();
        let slot = match self.userdata.get(idx) {
            Some(slot) => slot,
            None => panic!(
                "local key had an index greater than MAX_LOCAL_KEYS: index = {idx}, max = {}",
                MAX_LOCAL_KEYS
            ),
        };

        // XXX(eliza): would be nicer if these could be `dyn Any`s and the cast
        // could be checked...
        let mut ptr = slot.load(Ordering::Acquire);
        if ptr.is_null() {
            let data = Box::new((key.initializer)());
            let data_ptr = Box::into_raw(data) as *mut ();
            slot.compare_exchange(ptr, data_ptr, Ordering::AcqRel, Ordering::Acquire)
                .expect("CAS should be uncontended!");
            ptr = data_ptr;
        }

        let data = unsafe { &*(ptr as *const T) };
        f(data)
    }

    /// # Safety
    ///
    /// This should only be called a single time per CPU core.
    pub unsafe fn init() {
        tracing::trace!("initializing local data");
        let ptr = Box::into_raw(Box::new(Self::new()));
        unsafe {
            // set up self reference
            (*ptr)._self = ptr as *const _;
            Msr::ia32_gs_base().write(ptr as u64);
        }
    }

    fn has_magic() -> bool {
        let word: usize;
        unsafe {
            asm!("mov {}, gs:8", out(reg) word);
        }
        word == Self::MAGIC
    }
}

impl<T: 'static> LocalKey<T> {
    #[must_use]
    pub const fn new(initializer: fn() -> T) -> Self {
        Self {
            idx: Lazy::new(|| {
                static NEXT_INDEX: AtomicUsize = AtomicUsize::new(0);
                NEXT_INDEX.fetch_add(1, Ordering::Relaxed)
            }),
            initializer,
        }
    }

    pub fn with<U>(&self, f: impl FnOnce(&T) -> U) -> U {
        GsLocalData::current().with(self, f)
    }
}

impl<T> fmt::Debug for LocalKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalKey")
            .field("type", &core::any::type_name::<T>())
            .field("initializer", &fmt::ptr(self.initializer))
            .field("idx", &self.idx)
            .finish()
    }
}
