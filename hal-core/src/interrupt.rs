/// An interrupt controller for a platform.
pub trait Control {
    type Error;
    /// The type of this platform's interrupt vector.
    ///
    /// TODO(eliza): This is *probably* always a `u8`, do we really need to have
    /// this?
    type Vector;

    /// Disable interrupts.
    unsafe fn disable_irq(&mut self);

    /// Enable interrupts.
    unsafe fn enable_irq(&mut self);

    /// Returns `true` if interrupts are enabled.
    fn is_enabled(&self) -> bool;

    /// Register an interrupt handler to service the provided interrupt.
    ///
    /// The interrupt controller is assumed to generate an actual ISR that calls
    /// the handler; that ISR will construct an interrupt context for the
    /// handler. The ISR is responsible for entering a critical section while
    /// the handler is active.
    fn register_handler<I>(
        &mut self,
        irq: &I,
        handler: I::Handler,
    ) -> Result<(), RegistrationError>
    where
        I: Interrupt<Ctrl = Self>;

    /// Enter a critical section, returning a guard.
    fn enter_critical(&mut self) -> CriticalGuard<'_, Self> {
        unsafe {
            self.disable_irq();
        }
        CriticalGuard { ctrl: self }
    }
}

/// An interrupt.
pub trait Interrupt {
    type Ctrl: Control + ?Sized;
    /// The type of handler for this interrupt.
    ///
    /// This should always be a function pointer; but we cannot constrain this
    /// associated type since we don't know the _arguments_ to the handler
    /// function.
    type Handler;

    /// Returns the interrupt vector associated with this interrupt.
    fn vector(&self) -> <Self::Ctrl as Control>::Vector;

    /// Returns the name of this interrupt, for diagnostics etc.
    fn name(&self) -> &'static str;
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
            self.ctrl.enable_irq();
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
