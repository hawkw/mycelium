use hal_core::mem::page;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Size4Kb {}

impl page::Size for Size4Kb {
    const SIZE: usize = 1024 * 4;
}
