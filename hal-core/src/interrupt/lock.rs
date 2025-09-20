use super::MaskInterrupt;
use maitake_sync::{blocking::RawMutex, spin::Spinlock};
use portable_atomic::{AtomicBool, Ordering};

/// A spinlock that also provides mutual exclusion against the interrupt handler
/// for a particular interrupt vector.
///
/// While this spinlock is locked, the interrupt vector is masked, preventing
/// the interrupt handler from running. The interrupt is unmasked when the
/// spinlock is unlocked.
///
/// This type requires exclusive control over the masking and unmasking of that
/// interrupt vector.
#[derive(Debug)]
pub struct IrqSpinlock<I, V> {
    lock: Spinlock,
    ctrl: I,
    vector: V,
}

pub struct IrqSpinlockTable<V, const VECTORS: usize> {
    locks: [AtomicBool; VECTORS],
    _v: core::marker::PhantomData<fn(V)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimLockError {
    AlreadyClaimed,
    NoSuchIrq,
}

impl<I, V> IrqSpinlock<I, V>
where
    I: MaskInterrupt<V>,
    V: Copy,
{
    /// # Safety
    ///
    /// Constructing an `IrqSpinlock` for a given interrupt vector requires
    /// exclusive control over the masking and unmasking of that interrupt
    /// vector.
    pub unsafe fn new(ctrl: I, vector: V) -> Self {
        Self {
            lock: Spinlock::new(),
            ctrl,
            vector,
        }
    }
}

unsafe impl<I, V> RawMutex for IrqSpinlock<I, V>
where
    I: MaskInterrupt<V>,
    V: Copy,
{
    type GuardMarker = ();

    fn lock(&self) {
        self.lock.lock();
        unsafe {
            // Safety: having locked the spinlock means we have exclusive access
            // to that vector.
            self.ctrl.mask_irq(self.vector)
        };
    }

    fn try_lock(&self) -> bool {
        if !self.lock.try_lock() {
            return false;
        }

        unsafe {
            // Safety: having locked the spinlock means we have exclusive access
            // to that vector.
            self.ctrl.mask_irq(self.vector)
        };
        true
    }

    unsafe fn unlock(&self) {
        // Safety: the contract of `RawMutex::unlock` requires that the caller
        // ensure that the mutex is locked, so this implies that we have
        // exclusive access to that vector in the interrupt controller.
        self.ctrl.unmask_irq(self.vector);
        self.lock.unlock();
    }

    /// Returns `true` if the mutex is currently locked.
    fn is_locked(&self) -> bool {
        self.lock.is_locked()
    }
}

impl<V, const VECTORS: usize> IrqSpinlockTable<V, VECTORS>
where
    V: Into<usize> + Copy,
{
    pub fn claim_lock<I>(&self, vector: V, ctrl: I) -> Result<IrqSpinlock<I, V>, ClaimLockError>
    where
        I: MaskInterrupt<V>,
    {
        let claimed = self
            .locks
            .get(vector.into())
            .ok_or(ClaimLockError::NoSuchIrq)?;
        claimed
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ClaimLockError::AlreadyClaimed)?;
        Ok(unsafe {
            // Safety: we have just claimed the vector, so unless the interrupt
            // controller hands out access to it through other mechanisms, we
            // have exclusive access to it.
            IrqSpinlock::new(ctrl, vector)
        })
    }
}
