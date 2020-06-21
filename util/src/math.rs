pub trait Log2 {
    /// Returns the log base 2 of `self`.
    fn log2(self) -> Self;
}

impl Log2 for usize {
    /// Fast log base 2 implementation for integers.
    #[inline(always)]
    fn log2(self) -> usize {
        const WORD_SIZE: usize = 0usize.leading_zeros() as usize;
        WORD_SIZE - self.leading_zeros() as usize
    }
}
