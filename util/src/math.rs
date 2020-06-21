pub trait Log2 {
    /// Returns the log base 2 of `self`.
    fn log2(self) -> Self;
}

impl Log2 for usize {
    /// Fast log base 2 implementation for integers.
    ///
    /// Based on the C code at
    /// http://graphics.stanford.edu/~seander/bithacks.html#IntegerLog
    fn log2(mut self) -> usize {
        const B: [usize; 6] = [
            0x2,
            0xC,
            0xF0,
            0xFF00,
            0xFFFF0000,
            #[cfg(target_pointer_width = "64")]
            // only include this if a `usize` is 64-bits.
            0xFFFFFFFF00000000,
        ];

        const S: [usize; 6] = [
            1,
            2,
            4,
            8,
            16,
            #[cfg(target_pointer_width = "64")]
            // only include this if a `usize` is 64-bits.
            32,
        ];

        let mut result: usize = 0;

        for i in (0..S.len()).rev() {
            if self & B[i] != 0 {
                self >>= S[i];
                result |= S[i];
            }
        }
        result
    }
}
