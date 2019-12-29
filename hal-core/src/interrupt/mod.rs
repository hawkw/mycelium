use crate::Architecture;
use core::fmt;
use core::marker::PhantomData;

pub mod ctx;
pub use self::ctx::Context;
// pub mod vectors;

/// An interrupt controller for a platform.
pub trait Control {
    type Arch: Architecture;
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

    // /// Register an interrupt handler to service the provided interrupt.
    // ///
    // /// The interrupt controller is assumed to generate an actual ISR that calls
    // /// the handler; that ISR will construct an interrupt context for the
    // /// handler. The ISR is responsible for entering a critical section while
    // /// the handler is active.
    // fn register_handler<I, C>(
    //     &mut self,
    //     handler: fn(C) -> I::Out,
    // ) -> Result<(), RegistrationError<I>>
    // where
    //     Self: RegisterHandler<I, C>,
    //     I: Interrupt<C>,
    //     // C: Context,
    // {
    //     self.register(handler)
    // }
    fn register_handlers<H>(&mut self) -> Result<(), RegistrationError>
    where
        H: Handlers<Self::Arch>;

    /// Enter a critical section, returning a guard.
    fn enter_critical(&mut self) -> CriticalGuard<'_, Self> {
        unsafe {
            self.disable();
        }
        CriticalGuard { ctrl: self }
    }
}

pub trait Handlers<A: Architecture> {
    fn page_fault<C>(cx: C)
    where
        C: ctx::Context<Arch = A> + ctx::PageFault;

    fn code_fault<C>(cx: C)
    where
        C: ctx::Context<Arch = A> + ctx::CodeFault;

    #[inline(always)]
    fn double_fault<C>(cx: C)
    where
        C: ctx::Context<Arch = A> + ctx::CodeFault,
    {
        Self::code_fault(cx)
    }

    fn timer_tick();

    fn keyboard_controller();

    fn test_interrupt<C>(cx: C)
    where
        C: ctx::Context<Arch = A>,
    {
        // nop
    }
}

// pub trait RegisterHandler<I, C>
// where
//     I: Interrupt<C>,
//     // C: Context,
//     Self: Sized,
// {
//     fn register(&mut self, handler: fn(C) -> I::Out) -> Result<(), RegistrationError<I>>;
// }

// /// An interrupt.
// pub trait Interrupt<Ctx> {
//     type Out;
//     // // type Ctrl: Control + ?Sized;

//     const NAME: &'static str = "unknown interrupt";
// }

/// Errors that may occur while registering an interrupt handler.
#[derive(Clone, Eq, PartialEq)]
pub struct RegistrationError {
    kind: RegistrationErrorKind,
    // _irq: PhantomData<fn(I)>,
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
            // _irq: PhantomData,
        }
    }

    /// Returns a new error indicating that the registered interrupt vector has
    /// already been registered and cannot be registered again.
    pub fn already_registered() -> Self {
        Self {
            kind: RegistrationErrorKind::AlreadyRegistered,
            // _irq: PhantomData,
        }
    }

    /// Returns a new platform-specific error with the provided message.
    pub fn other(message: &'static str) -> Self {
        Self {
            kind: RegistrationErrorKind::Other(message),
            // _irq: PhantomData,
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

// impl<I: Interrupt<C>, C> fmt::Display for RegistrationError<I> {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         write!("failed to register {} handler", I::NAME)
//     }
// }

impl fmt::Debug for RegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistrationError")
            .field("kind", &self.kind)
            // .field(
            //     "interrupt",
            //     &format_args!("{}", core::any::type_name::<I>()),
            // )
            .finish()
    }
}
