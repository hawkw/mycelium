use super::{Context, Interrupt};
use crate::Architecture;

use core::marker::PhantomData;

pub trait PageFaultContext {
    type Arch: Architecture;
    fn address(&self) -> <Self::Arch as Architecture>::VAddr;
    fn was_not_present(&self) -> bool;
    fn was_user_mode(&self) -> bool;
    fn was_protection_violation(&self) -> bool {
        !self.was_not_present()
    }
}

pub struct PageFault<C> {
    _p: PhantomData<C>,
}

impl<C> Interrupt<C> for PageFault<C>
where
// C: PageFaultContext,
{
    type Out = ();
    const NAME: &'static str = "page fault";
}

pub struct Test<C> {
    _p: PhantomData<C>,
}

impl<C> Interrupt<C> for Test<C>
where
// C: PageFaultContext,
{
    type Out = ();
    const NAME: &'static str = "test";
}
