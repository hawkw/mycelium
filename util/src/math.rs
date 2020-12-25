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
    u.next_power_of_two().trailing_zeros() as usize
}

#[test]
fn test_log2() {
    assert_eq!(0, 0.log2(), "");
    assert_eq!(0, 1.log2());
    assert_eq!(1, 2.log2());
    assert_eq!(5, 32.log2());
    assert_eq!(10, 1024.log2());
}
