use core::marker::PhantomData;

/// An interrupt controller for a platform.
pub trait Control {
    type Vector;

    // type PageFault: Interrupt<Ctrl = Self>;
    // const PAGE_FAULT: Self::PageFault;

    /// Disable all interrupts.
    ///
    /// # Safety
    ///
    /// This may cause a fault if called when interrupts are already disabled
    /// (depending on the platform). It does not guarantee that interrupts will
    /// ever be unmasked.
    unsafe fn disable(&mut self);

    /// Enable all interrupts.
    ///
    /// # Safety
    ///
    /// This may cause a fault if called when interrupts are already enabled
    /// (depending on the platform).
    unsafe fn enable(&mut self);

    /// Returns `true` if interrupts are enabled.
    fn is_enabled(&self) -> bool;

    /// Register an interrupt handler to service the provided interrupt.
    ///
    /// The interrupt controller is assumed to generate an actual ISR that calls
    /// the handler; that ISR will construct an interrupt context for the
    /// handler. The ISR is responsible for entering a critical section while
    /// the handler is active.
    unsafe fn register_handler_raw<I>(
        &mut self,
        irq: &I,
        handler: *const (),
    ) -> Result<(), RegistrationError>
    where
        I: Interrupt<Ctrl = Self>;

    /// Enter a critical section, returning a guard.
    fn enter_critical(&mut self) -> CriticalGuard<'_, Self> {
        unsafe {
            self.disable();
        }
        CriticalGuard { ctrl: self }
    }
}

/// An interrupt.
pub trait Interrupt {
    /// The type of handler for this interrupt.
    ///
    /// This should always be a function pointer; but we cannot constrain this
    /// associated type since we don't know the _arguments_ to the handler
    /// function.
    type Handler;

    type Ctrl: Control + ?Sized;

    fn vector(&self) -> <Self::Ctrl as Control>::Vector;

    /// Returns the name of this interrupt, for diagnostics etc.
    fn name(&self) -> &'static str {
        "unknown interrupt"
    }
}

/// Errors that may occur while registering an interrupt handler.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RegistrationError {
    kind: RegistrationErrorKind,
}

#[derive(Debug)]
pub struct CriticalGuard<'a, C: Control + ?Sized> {
    ctrl: &'a mut C,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum RegistrationErrorKind {
    Nonexistant,
    AlreadyRegistered,
    Other(&'static str),
}

// === impl CriticalGuard ===

impl<'a, C: Control + ?Sized> Drop for CriticalGuard<'a, C> {
    fn drop(&mut self) {
        unsafe {
            self.ctrl.enable();
        }
    }
}

// === impl RegistrationError ===
impl RegistrationError {
    /// Returns a new error indicating that the registered interrupt vector does
    /// not exist.
    pub fn nonexistant() -> Self {
        Self {
            kind: RegistrationErrorKind::Nonexistant,
        }
    }

    /// Returns a new error indicating that the registered interrupt vector has
    /// already been registered and cannot be registered again.
    pub fn already_registered() -> Self {
        Self {
            kind: RegistrationErrorKind::AlreadyRegistered,
        }
    }

    /// Returns a new platform-specific error with the provided message.
    pub fn other(message: &'static str) -> Self {
        Self {
            kind: RegistrationErrorKind::Other(message),
        }
    }

    pub fn is_nonexistant(&self) -> bool {
        match self.kind {
            RegistrationErrorKind::Nonexistant => true,
            _ => false,
        }
    }

    pub fn is_already_registered(&self) -> bool {
        match self.kind {
            RegistrationErrorKind::AlreadyRegistered => true,
            _ => false,
        }
    }
}
