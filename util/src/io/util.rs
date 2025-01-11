use crate::io::{self, ErrorKind, Initializer, Read, Write};
use core::{fmt, mem::MaybeUninit};

/// Copies the entire contents of a reader into a writer.
///
/// This function will continuously read data from `reader` and then
/// write it into `writer` in a streaming fashion until `reader`
/// returns EOF.
///
/// On success, the total number of bytes that were copied from
/// `reader` to `writer` is returned.
///
/// # Errors
///
/// This function will return an error immediately if any call to `read` or
/// `write` returns an error. All instances of `ErrorKind::Interrupted` are
/// handled by this function and the underlying operation is retried.
pub fn copy<R, W>(reader: &mut R, writer: &mut W) -> io::Result<u64>
where
    R: ?Sized + Read,
    W: ?Sized + Write,
{
    let mut buf = MaybeUninit::<[u8; super::DEFAULT_BUF_SIZE]>::uninit();
    // FIXME(eliza): the stdlib has a scary comment that says we haven't decided
    // if this is okay or not...
    unsafe {
        reader.initializer().initialize(&mut *buf.as_mut_ptr());
    }

    let mut written = 0;
    loop {
        let len = match reader.read(unsafe { &mut *buf.as_mut_ptr() }) {
            Ok(0) => return Ok(written),
            Ok(len) => len,
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(unsafe { &(&*buf.as_ptr())[..len] })?;
        written += len as u64;
    }
}

/// A reader which is always at EOF.
///
/// This struct is generally created by calling [`empty`]. Please see
/// the documentation of [`empty()`][`empty`] for more details.
///
/// [`empty`]: fn.empty.html
pub struct Empty {
    _priv: (),
}

/// Constructs a new handle to an empty reader.
///
/// All reads from the returned reader will return `Ok(0)`.
///
pub fn empty() -> Empty {
    Empty { _priv: () }
}

impl Read for Empty {
    #[inline]
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }

    #[inline]
    unsafe fn initializer(&self) -> Initializer {
        Initializer::nop()
    }
}

// impl BufRead for Empty {
//     #[inline]
//     fn fill_buf(&mut self) -> io::Result<&[u8]> {
//         Ok(&[])
//     }
//     #[inline]
//     fn consume(&mut self, _n: usize) {}
// }

impl fmt::Debug for Empty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("Empty { .. }")
    }
}

/// A reader which yields one byte over and over and over and over and over and...
///
/// This struct is generally created by calling [`repeat`][repeat]. Please
/// see the documentation of `repeat()` for more details.
///
/// [repeat]: fn.repeat.html
pub struct Repeat {
    byte: u8,
}

/// Creates an instance of a reader that infinitely repeats one byte.
///
/// All reads from this reader will succeed by filling the specified buffer with
/// the given byte.
pub fn repeat(byte: u8) -> Repeat {
    Repeat { byte }
}

impl Read for Repeat {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        for slot in &mut *buf {
            *slot = self.byte;
        }
        Ok(buf.len())
    }

    #[inline]
    unsafe fn initializer(&self) -> Initializer {
        Initializer::nop()
    }
}

impl fmt::Debug for Repeat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("Repeat { .. }")
    }
}

/// A writer which will move data into the void.
///
/// This struct is generally created by calling [`sink`][sink]. Please
/// see the documentation of `sink()` for more details.
///
/// [sink]: fn.sink.html
pub struct Sink {
    _priv: (),
}

/// Creates an instance of a writer which will successfully consume all data.
///
/// All calls to `write` on the returned instance will return `Ok(buf.len())`
/// and the contents of the buffer will not be inspected.
pub fn sink() -> Sink {
    Sink { _priv: () }
}

impl Write for Sink {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl fmt::Debug for Sink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("Sink { .. }")
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use crate::io::prelude::*;
    use crate::io::{copy, empty, repeat, sink};

    #[test]
    fn copy_copies() {
        let mut r = repeat(0).take(4);
        let mut w = sink();
        assert_eq!(copy(&mut r, &mut w).unwrap(), 4);

        let mut r = repeat(0).take(1 << 17);
        assert_eq!(
            copy(&mut r as &mut dyn Read, &mut w as &mut dyn Write).unwrap(),
            1 << 17
        );
    }

    #[test]
    fn sink_sinks() {
        let mut s = sink();
        assert_eq!(s.write(&[]).unwrap(), 0);
        assert_eq!(s.write(&[0]).unwrap(), 1);
        assert_eq!(s.write(&[0; 1024]).unwrap(), 1024);
        assert_eq!(s.by_ref().write(&[0; 1024]).unwrap(), 1024);
    }

    #[test]
    fn empty_reads() {
        let mut e = empty();
        assert_eq!(e.read(&mut []).unwrap(), 0);
        assert_eq!(e.read(&mut [0]).unwrap(), 0);
        assert_eq!(e.read(&mut [0; 1024]).unwrap(), 0);
        assert_eq!(e.by_ref().read(&mut [0; 1024]).unwrap(), 0);
    }

    #[test]
    fn repeat_repeats() {
        let mut r = repeat(4);
        let mut b = [0; 1024];
        assert_eq!(r.read(&mut b).unwrap(), 1024);
        assert!(b.iter().all(|b| *b == 4));
    }

    #[test]
    fn take_some_bytes() {
        assert_eq!(repeat(4).take(100).bytes().count(), 100);
        assert_eq!(repeat(4).take(100).bytes().next().unwrap().unwrap(), 4);
        assert_eq!(
            repeat(1).take(10).chain(repeat(2).take(10)).bytes().count(),
            20
        );
    }
}
