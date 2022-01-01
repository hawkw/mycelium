pub trait Log2 {
    /// Returns `ceiling(log2(self))`.
    #[must_use]
    fn log2_ceil(self) -> Self;
}

impl Log2 for usize {
    #[inline(always)]
    fn log2_ceil(self) -> usize {
        usize_const_log2_ceil(self)
    }
}

pub const fn usize_const_log2_ceil(u: usize) -> usize {
    u.next_power_of_two().trailing_zeros() as usize
}

#[test]
fn test_log2_ceil() {
    assert_eq!(0, 0.log2_ceil());
    assert_eq!(0, 1.log2_ceil());
    assert_eq!(1, 2.log2_ceil());
    assert_eq!(5, 32.log2_ceil());
    assert_eq!(10, 1024.log2_ceil());
}
