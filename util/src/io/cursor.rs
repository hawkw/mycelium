use crate::io::prelude::*;
use crate::io::{self, Error, ErrorKind, Initializer, SeekFrom};

use core::cmp;

#[cfg(feature = "alloc")]
use core::convert::TryInto;

#[cfg(feature = "alloc")]
use alloc::{boxed::Box, vec::Vec};

/// A `Cursor` wraps an in-memory buffer and provides it with a
/// [`Seek`] implementation.
///
/// `Cursor`s are used with in-memory buffers, anything implementing
/// `AsRef<[u8]>`, to allow them to implement [`Read`] and/or [`Write`],
/// allowing these buffers to be used anywhere you might use a reader or writer
/// that does actual I/O.
///
/// The standard library implements some I/O traits on various types which
/// are commonly used as a buffer, like `Cursor<`[`Vec`]`<u8>>` and
/// `Cursor<`[`&[u8]`][bytes]`>`.
///
/// [`Seek`]: trait.Seek.html
/// [`Read`]: trait.Read.html
/// [`Write`]: trait.Write.html
/// [`Vec`]: ../../alloc/vec/struct.Vec.html
/// [bytes]: ../../core/primitive.slice.html
#[derive(Clone, Debug, Default)]
pub struct Cursor<T> {
    inner: T,
    pos: u64,
}

impl<T> Cursor<T> {
    /// Creates a new cursor wrapping the provided underlying in-memory buffer.
    ///
    /// Cursor initial position is `0` even if underlying buffer (e.g., `Vec`)
    /// is not empty. So writing to cursor starts with overwriting `Vec`
    /// content, not with appending to it.
    pub fn new(inner: T) -> Cursor<T> {
        Cursor { pos: 0, inner }
    }

    /// Consumes this cursor, returning the underlying value.
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Gets a reference to the underlying value in this cursor.
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Gets a mutable reference to the underlying value in this cursor.
    ///
    /// Care should be taken to avoid modifying the internal I/O state of the
    /// underlying value as it may corrupt this cursor's position.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Returns the current position of this cursor.
    pub fn position(&self) -> u64 {
        self.pos
    }

    /// Sets the position of this cursor.
    pub fn set_position(&mut self, pos: u64) {
        self.pos = pos;
    }
}

impl<T> io::Seek for Cursor<T>
where
    T: AsRef<[u8]>,
{
    fn seek(&mut self, style: SeekFrom) -> io::Result<u64> {
        let (base_pos, offset) = match style {
            SeekFrom::Start(n) => {
                self.pos = n;
                return Ok(n);
            }
            SeekFrom::End(n) => (self.inner.as_ref().len() as u64, n),
            SeekFrom::Current(n) => (self.pos, n),
        };
        let new_pos = if offset >= 0 {
            base_pos.checked_add(offset as u64)
        } else {
            base_pos.checked_sub((offset.wrapping_neg()) as u64)
        };
        match new_pos {
            Some(n) => {
                self.pos = n;
                Ok(self.pos)
            }
            None => Err(Error::new(
                ErrorKind::InvalidInput,
                "invalid seek to a negative or overflowing position",
            )),
        }
    }

    fn stream_len(&mut self) -> io::Result<u64> {
        Ok(self.inner.as_ref().len() as u64)
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        Ok(self.pos)
    }
}

impl<T> Read for Cursor<T>
where
    T: AsRef<[u8]>,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = Read::read(&mut self.fill_buf()?, buf)?;
        self.pos += n as u64;
        Ok(n)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let n = buf.len();
        Read::read_exact(&mut self.fill_buf()?, buf)?;
        self.pos += n as u64;
        Ok(())
    }

    #[inline]
    unsafe fn initializer(&self) -> Initializer {
        Initializer::nop()
    }
}

impl<T> BufRead for Cursor<T>
where
    T: AsRef<[u8]>,
{
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        let amt = cmp::min(self.pos, self.inner.as_ref().len() as u64);
        Ok(&self.inner.as_ref()[(amt as usize)..])
    }
    fn consume(&mut self, amt: usize) {
        self.pos += amt as u64;
    }
}

// Non-resizing write implementation
#[inline]
fn slice_write(pos_mut: &mut u64, slice: &mut [u8], buf: &[u8]) -> io::Result<usize> {
    let pos = cmp::min(*pos_mut, slice.len() as u64);
    let amt = (&mut slice[(pos as usize)..]).write(buf)?;
    *pos_mut += amt as u64;
    Ok(amt)
}

// Resizing write implementation
#[cfg(feature = "alloc")]
fn vec_write(pos_mut: &mut u64, vec: &mut Vec<u8>, buf: &[u8]) -> io::Result<usize> {
    let pos: usize = (*pos_mut).try_into().map_err(|_| {
        Error::new(
            ErrorKind::InvalidInput,
            "cursor position exceeds maximum possible vector length",
        )
    })?;
    // Make sure the internal buffer is as least as big as where we
    // currently are
    let len = vec.len();
    if len < pos {
        // use `resize` so that the zero filling is as efficient as possible
        vec.resize(pos, 0);
    }
    // Figure out what bytes will be used to overwrite what's currently
    // there (left), and what will be appended on the end (right)
    {
        let space = vec.len() - pos;
        let (left, right) = buf.split_at(cmp::min(space, buf.len()));
        vec[pos..pos + left.len()].copy_from_slice(left);
        vec.extend_from_slice(right);
    }

    // Bump us forward
    *pos_mut = (pos + buf.len()) as u64;
    Ok(buf.len())
}

impl Write for Cursor<&mut [u8]> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        slice_write(&mut self.pos, self.inner, buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl Write for Cursor<&mut Vec<u8>> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        vec_write(&mut self.pos, self.inner, buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl Write for Cursor<Vec<u8>> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        vec_write(&mut self.pos, &mut self.inner, buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl Write for Cursor<Box<[u8]>> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        slice_write(&mut self.pos, &mut self.inner, buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use crate::io::prelude::*;
    use crate::io::{Cursor, SeekFrom};
    #[cfg(feature = "alloc")]
    use alloc::{vec, vec::Vec};

    #[test]
    #[cfg(feature = "alloc")]
    fn test_vec_writer() {
        let mut writer = Vec::new();
        assert_eq!(writer.write(&[0]).unwrap(), 1);
        assert_eq!(writer.write(&[1, 2, 3]).unwrap(), 3);
        assert_eq!(writer.write(&[4, 5, 6, 7]).unwrap(), 4);
        let b: &[_] = &[0, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(writer, b);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_mem_writer() {
        let mut writer = Cursor::new(Vec::new());
        assert_eq!(writer.write(&[0]).unwrap(), 1);
        assert_eq!(writer.write(&[1, 2, 3]).unwrap(), 3);
        assert_eq!(writer.write(&[4, 5, 6, 7]).unwrap(), 4);
        let b: &[_] = &[0, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(&writer.get_ref()[..], b);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_mem_mut_writer() {
        let mut vec = Vec::new();
        let mut writer = Cursor::new(&mut vec);
        assert_eq!(writer.write(&[0]).unwrap(), 1);
        assert_eq!(writer.write(&[1, 2, 3]).unwrap(), 3);
        assert_eq!(writer.write(&[4, 5, 6, 7]).unwrap(), 4);
        let b: &[_] = &[0, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(&writer.get_ref()[..], b);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_box_slice_writer() {
        let mut writer = Cursor::new(vec![0u8; 9].into_boxed_slice());
        assert_eq!(writer.position(), 0);
        assert_eq!(writer.write(&[0]).unwrap(), 1);
        assert_eq!(writer.position(), 1);
        assert_eq!(writer.write(&[1, 2, 3]).unwrap(), 3);
        assert_eq!(writer.write(&[4, 5, 6, 7]).unwrap(), 4);
        assert_eq!(writer.position(), 8);
        assert_eq!(writer.write(&[]).unwrap(), 0);
        assert_eq!(writer.position(), 8);

        assert_eq!(writer.write(&[8, 9]).unwrap(), 1);
        assert_eq!(writer.write(&[10]).unwrap(), 0);
        let b: &[_] = &[0, 1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(&**writer.get_ref(), b);
    }

    #[test]
    fn test_buf_writer() {
        let mut buf = [0u8; 9];
        {
            let mut writer = Cursor::new(&mut buf[..]);
            assert_eq!(writer.position(), 0);
            assert_eq!(writer.write(&[0]).unwrap(), 1);
            assert_eq!(writer.position(), 1);
            assert_eq!(writer.write(&[1, 2, 3]).unwrap(), 3);
            assert_eq!(writer.write(&[4, 5, 6, 7]).unwrap(), 4);
            assert_eq!(writer.position(), 8);
            assert_eq!(writer.write(&[]).unwrap(), 0);
            assert_eq!(writer.position(), 8);

            assert_eq!(writer.write(&[8, 9]).unwrap(), 1);
            assert_eq!(writer.write(&[10]).unwrap(), 0);
        }
        let b: &[_] = &[0, 1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(buf, b);
    }

    #[test]
    fn test_buf_writer_seek() {
        let mut buf = [0u8; 8];
        {
            let mut writer = Cursor::new(&mut buf[..]);
            assert_eq!(writer.position(), 0);
            assert_eq!(writer.write(&[1]).unwrap(), 1);
            assert_eq!(writer.position(), 1);

            assert_eq!(writer.seek(SeekFrom::Start(2)).unwrap(), 2);
            assert_eq!(writer.position(), 2);
            assert_eq!(writer.write(&[2]).unwrap(), 1);
            assert_eq!(writer.position(), 3);

            assert_eq!(writer.seek(SeekFrom::Current(-2)).unwrap(), 1);
            assert_eq!(writer.position(), 1);
            assert_eq!(writer.write(&[3]).unwrap(), 1);
            assert_eq!(writer.position(), 2);

            assert_eq!(writer.seek(SeekFrom::End(-1)).unwrap(), 7);
            assert_eq!(writer.position(), 7);
            assert_eq!(writer.write(&[4]).unwrap(), 1);
            assert_eq!(writer.position(), 8);
        }
        let b: &[_] = &[1, 3, 2, 0, 0, 0, 0, 4];
        assert_eq!(buf, b);
    }

    #[test]
    fn test_buf_writer_error() {
        let mut buf = [0u8; 2];
        let mut writer = Cursor::new(&mut buf[..]);
        assert_eq!(writer.write(&[0]).unwrap(), 1);
        assert_eq!(writer.write(&[0, 0]).unwrap(), 1);
        assert_eq!(writer.write(&[0, 0]).unwrap(), 0);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_mem_reader() {
        let mut reader = Cursor::new(vec![0, 1, 2, 3, 4, 5, 6, 7]);
        let mut buf = [];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        assert_eq!(reader.position(), 0);
        let mut buf = [0];
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(reader.position(), 1);
        let b: &[_] = &[0];
        assert_eq!(buf, b);
        let mut buf = [0; 4];
        assert_eq!(reader.read(&mut buf).unwrap(), 4);
        assert_eq!(reader.position(), 5);
        let b: &[_] = &[1, 2, 3, 4];
        assert_eq!(buf, b);
        assert_eq!(reader.read(&mut buf).unwrap(), 3);
        let b: &[_] = &[5, 6, 7];
        assert_eq!(&buf[..3], b);
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_boxed_slice_reader() {
        let mut reader = Cursor::new(vec![0, 1, 2, 3, 4, 5, 6, 7].into_boxed_slice());
        let mut buf = [];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        assert_eq!(reader.position(), 0);
        let mut buf = [0];
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(reader.position(), 1);
        let b: &[_] = &[0];
        assert_eq!(buf, b);
        let mut buf = [0; 4];
        assert_eq!(reader.read(&mut buf).unwrap(), 4);
        assert_eq!(reader.position(), 5);
        let b: &[_] = &[1, 2, 3, 4];
        assert_eq!(buf, b);
        assert_eq!(reader.read(&mut buf).unwrap(), 3);
        let b: &[_] = &[5, 6, 7];
        assert_eq!(&buf[..3], b);
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn read_to_end() {
        let mut reader = Cursor::new(vec![0, 1, 2, 3, 4, 5, 6, 7]);
        let mut v = Vec::new();
        reader.read_to_end(&mut v).unwrap();
        assert_eq!(v, [0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_slice_reader() {
        let in_buf = [0, 1, 2, 3, 4, 5, 6, 7];
        let reader = &mut &in_buf[..];
        let mut buf = [];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        let mut buf = [0];
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(reader.len(), 7);
        let b: &[_] = &[0];
        assert_eq!(&buf[..], b);
        let mut buf = [0; 4];
        assert_eq!(reader.read(&mut buf).unwrap(), 4);
        assert_eq!(reader.len(), 3);
        let b: &[_] = &[1, 2, 3, 4];
        assert_eq!(&buf[..], b);
        assert_eq!(reader.read(&mut buf).unwrap(), 3);
        let b: &[_] = &[5, 6, 7];
        assert_eq!(&buf[..3], b);
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_read_exact() {
        let in_buf = [0, 1, 2, 3, 4, 5, 6, 7];
        let reader = &mut &in_buf[..];
        let mut buf = [];
        reader
            .read_exact(&mut buf)
            .expect("read_exact must succeed");
        let mut buf = [8];
        reader
            .read_exact(&mut buf)
            .expect("read_exact must succeed");
        assert_eq!(buf[0], 0);
        assert_eq!(reader.len(), 7);
        let mut buf = [0, 0, 0, 0, 0, 0, 0];
        reader
            .read_exact(&mut buf)
            .expect("read_exact must succeed");
        assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(reader.len(), 0);
        let mut buf = [0];
        reader
            .read_exact(&mut buf)
            .expect_err("read_exact on empty buffer must fail");
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_buf_reader() {
        let in_buf = [0, 1, 2, 3, 4, 5, 6, 7];
        let mut reader = Cursor::new(&in_buf[..]);
        let mut buf = [];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        assert_eq!(reader.position(), 0);
        let mut buf = [0];
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(reader.position(), 1);
        let b: &[_] = &[0];
        assert_eq!(buf, b);
        let mut buf = [0; 4];
        assert_eq!(reader.read(&mut buf).unwrap(), 4);
        assert_eq!(reader.position(), 5);
        let b: &[_] = &[1, 2, 3, 4];
        assert_eq!(buf, b);
        assert_eq!(reader.read(&mut buf).unwrap(), 3);
        let b: &[_] = &[5, 6, 7];
        assert_eq!(&buf[..3], b);
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn seek_past_end() {
        let buf = [0xff];
        let mut r = Cursor::new(&buf[..]);
        assert_eq!(r.seek(SeekFrom::Start(10)).unwrap(), 10);
        assert_eq!(r.read(&mut [0]).unwrap(), 0);

        let mut r = Cursor::new(vec![10]);
        assert_eq!(r.seek(SeekFrom::Start(10)).unwrap(), 10);
        assert_eq!(r.read(&mut [0]).unwrap(), 0);

        let mut buf = [0];
        let mut r = Cursor::new(&mut buf[..]);
        assert_eq!(r.seek(SeekFrom::Start(10)).unwrap(), 10);
        assert_eq!(r.write(&[3]).unwrap(), 0);

        #[cfg(feature = "alloc")]
        {
            let mut r = Cursor::new(vec![10].into_boxed_slice());
            assert_eq!(r.seek(SeekFrom::Start(10)).unwrap(), 10);
            assert_eq!(r.write(&[3]).unwrap(), 0);
        }
    }

    #[test]
    fn seek_past_i64() {
        let buf = [0xff];
        let mut r = Cursor::new(&buf[..]);
        assert_eq!(r.seek(SeekFrom::Start(6)).unwrap(), 6);
        assert_eq!(
            r.seek(SeekFrom::Current(0x7ffffffffffffff0)).unwrap(),
            0x7ffffffffffffff6
        );
        assert_eq!(r.seek(SeekFrom::Current(0x10)).unwrap(), 0x8000000000000006);
        assert_eq!(r.seek(SeekFrom::Current(0)).unwrap(), 0x8000000000000006);
        r.seek(SeekFrom::Current(0x7ffffffffffffffd))
            .expect_err("`seek` with big offset must fail");
        assert_eq!(r.seek(SeekFrom::Current(-0x8000000000000000)).unwrap(), 6);

        let mut r = Cursor::new(vec![10]);
        assert_eq!(r.seek(SeekFrom::Start(6)).unwrap(), 6);
        assert_eq!(
            r.seek(SeekFrom::Current(0x7ffffffffffffff0)).unwrap(),
            0x7ffffffffffffff6
        );
        assert_eq!(r.seek(SeekFrom::Current(0x10)).unwrap(), 0x8000000000000006);
        assert_eq!(r.seek(SeekFrom::Current(0)).unwrap(), 0x8000000000000006);
        r.seek(SeekFrom::Current(0x7ffffffffffffffd))
            .expect_err("`seek` with big offset must fail");
        assert_eq!(r.seek(SeekFrom::Current(-0x8000000000000000)).unwrap(), 6);

        let mut buf = [0];
        let mut r = Cursor::new(&mut buf[..]);
        assert_eq!(r.seek(SeekFrom::Start(6)).unwrap(), 6);
        assert_eq!(
            r.seek(SeekFrom::Current(0x7ffffffffffffff0)).unwrap(),
            0x7ffffffffffffff6
        );
        assert_eq!(r.seek(SeekFrom::Current(0x10)).unwrap(), 0x8000000000000006);
        assert_eq!(r.seek(SeekFrom::Current(0)).unwrap(), 0x8000000000000006);
        r.seek(SeekFrom::Current(0x7ffffffffffffffd))
            .expect_err("`seek` with big offset must fail");
        assert_eq!(r.seek(SeekFrom::Current(-0x8000000000000000)).unwrap(), 6);

        #[cfg(feature = "alloc")]
        {
            let mut r = Cursor::new(vec![10].into_boxed_slice());
            assert_eq!(r.seek(SeekFrom::Start(6)).unwrap(), 6);
            assert_eq!(
                r.seek(SeekFrom::Current(0x7ffffffffffffff0)).unwrap(),
                0x7ffffffffffffff6
            );
            assert_eq!(r.seek(SeekFrom::Current(0x10)).unwrap(), 0x8000000000000006);
            assert_eq!(r.seek(SeekFrom::Current(0)).unwrap(), 0x8000000000000006);
            r.seek(SeekFrom::Current(0x7ffffffffffffffd))
                .expect_err("`seek` with big offset must fail");
            assert_eq!(r.seek(SeekFrom::Current(-0x8000000000000000)).unwrap(), 6);
        }
    }

    #[test]
    fn seek_before_0() {
        let buf = [0xff];
        let mut r = Cursor::new(&buf[..]);
        r.seek(SeekFrom::End(-2))
            .expect_err("`seek` before 0 must fail");

        let mut r = Cursor::new(vec![10]);
        r.seek(SeekFrom::End(-2))
            .expect_err("`seek` before 0 must fail");

        let mut buf = [0];
        let mut r = Cursor::new(&mut buf[..]);
        r.seek(SeekFrom::End(-2))
            .expect_err("`seek` before 0 must fail");

        #[cfg(feature = "alloc")]
        {
            let mut r = Cursor::new(vec![10].into_boxed_slice());
            r.seek(SeekFrom::End(-2))
                .expect_err("`seek` before 0 must fail");
        }
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_seekable_mem_writer() {
        let mut writer = Cursor::new(Vec::<u8>::new());
        assert_eq!(writer.position(), 0);
        assert_eq!(writer.write(&[0]).unwrap(), 1);
        assert_eq!(writer.position(), 1);
        assert_eq!(writer.write(&[1, 2, 3]).unwrap(), 3);
        assert_eq!(writer.write(&[4, 5, 6, 7]).unwrap(), 4);
        assert_eq!(writer.position(), 8);
        let b: &[_] = &[0, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(&writer.get_ref()[..], b);

        assert_eq!(writer.seek(SeekFrom::Start(0)).unwrap(), 0);
        assert_eq!(writer.position(), 0);
        assert_eq!(writer.write(&[3, 4]).unwrap(), 2);
        let b: &[_] = &[3, 4, 2, 3, 4, 5, 6, 7];
        assert_eq!(&writer.get_ref()[..], b);

        assert_eq!(writer.seek(SeekFrom::Current(1)).unwrap(), 3);
        assert_eq!(writer.write(&[0, 1]).unwrap(), 2);
        let b: &[_] = &[3, 4, 2, 0, 1, 5, 6, 7];
        assert_eq!(&writer.get_ref()[..], b);

        assert_eq!(writer.seek(SeekFrom::End(-1)).unwrap(), 7);
        assert_eq!(writer.write(&[1, 2]).unwrap(), 2);
        let b: &[_] = &[3, 4, 2, 0, 1, 5, 6, 1, 2];
        assert_eq!(&writer.get_ref()[..], b);

        assert_eq!(writer.seek(SeekFrom::End(1)).unwrap(), 10);
        assert_eq!(writer.write(&[1]).unwrap(), 1);
        let b: &[_] = &[3, 4, 2, 0, 1, 5, 6, 1, 2, 0, 1];
        assert_eq!(&writer.get_ref()[..], b);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn vec_seek_past_end() {
        let mut r = Cursor::new(Vec::new());
        assert_eq!(r.seek(SeekFrom::Start(10)).unwrap(), 10);
        assert_eq!(r.write(&[3]).unwrap(), 1);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn vec_seek_before_0() {
        let mut r = Cursor::new(Vec::new());
        r.seek(SeekFrom::End(-2))
            .expect_err("`seek` before 0 must fail");
    }

    #[test]
    #[cfg(target_pointer_width = "32")]
    #[cfg(feature = "alloc")]
    fn vec_seek_and_write_past_usize_max() {
        let mut c = Cursor::new(Vec::new());
        c.set_position(<usize>::max_value() as u64 + 1);
        assert!(c.write_all(&[1, 2, 3]).is_err());
    }
}
