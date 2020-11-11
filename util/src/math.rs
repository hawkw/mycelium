pub trait Log2 {
    /// Returns the log base 2 of `self`.
    fn log2(self) -> Self;
}

impl Log2 for usize {
    /// Fast log base 2 implementation for integers.
    #[inline(always)]
    fn log2(self) -> usize {
        usize_const_log2(self)
    }
}

pub const fn usize_const_log2(u: usize) -> usize {
    const WORD_SIZE: usize = 0usize.leading_zeros() as usize;
    WORD_SIZE - u.leading_zeros() as usize
}
