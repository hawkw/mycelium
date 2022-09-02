//! Timer utilities.

pub use self::sleep::Sleep;
mod sleep;

pub type Ticks = u64;

pub struct Timer {}

impl Timer {
    /// Returns a future that will complete in `ticks` timer ticks.
    pub fn sleep(&self, ticks: Ticks) -> Sleep {
        todo!("eliza")
    }

    /// Advance the timer by `ticks`, waking any `Sleep` futures that have
    /// completed.
    ///
    /// # Returns
    ///
    /// The number of `Sleep` futures that were woken.
    pub fn advance(&mut self, ticks: Ticks) -> usize {
        todo!("eliza")
    }
}
