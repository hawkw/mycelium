use core::ptr;

/// A type used to conditionally initialize buffers passed to `Read` methods.
#[derive(Debug)]
pub struct Initializer(bool);

impl Initializer {
    /// Returns a new `Initializer` which will zero out buffers.
    #[inline]
    pub fn zeroing() -> Initializer {
        Initializer(true)
    }

    /// Returns a new `Initializer` which will not zero out buffers.
    ///
    /// # Safety
    ///
    /// This may only be called by `Read`ers which guarantee that they will not
    /// read from buffers passed to `Read` methods, and that the return value of
    /// the method accurately reflects the number of bytes that have been
    /// written to the head of the buffer.
    #[inline]
    pub unsafe fn nop() -> Initializer {
        Initializer(false)
    }

    /// Indicates if a buffer should be initialized.
    #[inline]
    pub fn should_initialize(&self) -> bool {
        self.0
    }

    /// Initializes a buffer if necessary.
    #[inline]
    pub fn initialize(&self, buf: &mut [u8]) {
        if self.should_initialize() {
            unsafe { ptr::write_bytes(buf.as_mut_ptr(), 0, buf.len()) }
        }
    }
}
