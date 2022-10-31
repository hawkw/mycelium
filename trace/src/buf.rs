use alloc::{boxed::Box, string::String, vec::Vec};
use core::{
    fmt::{self, Write},
    num::Wrapping,
    ops::{Bound, RangeBounds},
};

/// A ring buffer of fixed-size lines.
#[derive(Debug)]
pub struct LineBuf {
    lines: Box<[Line]>,
    /// The maximum length of each line in the buffer
    line_len: usize,
    start: Wrapping<usize>,
    end: Wrapping<usize>,
}

/// Configuration for a [`Buf`].
#[derive(Copy, Clone, Debug)]
#[non_exhaustive]
pub struct BufConfig {
    pub line_len: usize,
    pub lines: usize,
}

#[derive(Copy, Clone, Debug)]
pub struct Iter<'buf> {
    buf: &'buf LineBuf,
    idx: Wrapping<usize>,
    end: Wrapping<usize>,
}

#[derive(Debug)]
struct Line {
    line: String,
    stamp: Wrapping<usize>,
}

#[cfg(test)]
macro_rules! test_dbg {
    ($x:expr) => {
        dbg!($x)
    };
}

#[cfg(not(test))]
macro_rules! test_dbg {
    ($x:expr) => {
        $x
    };
}

impl LineBuf {
    pub fn new(config: BufConfig) -> Self {
        let line_len = config.line_len;
        Self {
            lines: (0..config.lines)
                .map(|stamp| Line {
                    stamp: Wrapping(stamp),
                    line: String::with_capacity(line_len),
                })
                .collect::<Vec<_>>()
                .into(),
            line_len,
            start: Wrapping(0),
            end: Wrapping(0),
        }
    }

    pub fn iter(&self) -> Iter<'_> {
        Iter {
            idx: self.start,
            end: self.end,
            buf: self,
        }
    }

    pub fn lines(&self, range: impl RangeBounds<usize>) -> Iter<'_> {
        let idx = self.bound_offset(range.start_bound()).unwrap_or(self.start);
        let end = self.bound_offset(range.end_bound()).unwrap_or(self.end);
        Iter {
            idx,
            end,
            buf: self,
        }
    }

    fn bound_offset(&self, bound: Bound<&usize>) -> Option<Wrapping<usize>> {
        let offset = match bound {
            Bound::Excluded(&offset) => offset + 1,
            Bound::Included(&offset) => offset,
            Bound::Unbounded => return None,
        };
        Some(self.start + Wrapping(offset) % Wrapping(self.lines.len()))
    }

    fn advance(&mut self) {
        self.end += 1;
    }

    fn line_mut(&mut self) -> &mut String {
        let idx = self.end.0 % self.lines.len();
        let Line { stamp, line } = &mut self.lines[idx];
        if *stamp != self.end {
            *stamp = self.end;
            line.clear();

            if self.end == self.start {
                // we are writing to the current start index, so scootch the
                // start forward by one.
                self.start += 1;
            }
        }
        line
    }

    fn write_chunk<'s>(&mut self, s: &'s str) -> Option<&'s str> {
        let rem = self.line_len - self.line_mut().len();
        let (line, next) = if s.len() > rem {
            let (this, next) = s.split_at(rem);
            (this, Some(next))
        } else {
            (s, None)
        };
        self.line_mut().push_str(line);
        next
    }

    fn write_line(&mut self, mut line: &str) {
        while let Some(next) = test_dbg!(self.write_chunk(test_dbg!(line))) {
            line = next;
            test_dbg!(self.advance());
        }
    }
}

impl Write for &mut LineBuf {
    fn write_str(&mut self, mut s: &str) -> fmt::Result {
        let ends_with_newline = if let Some(stripped) = s.strip_suffix('\n') {
            s = stripped;
            true
        } else {
            false
        };

        let mut lines = s.split('\n');
        if let Some(line) = lines.next() {
            self.write_line(line);
            for line in lines {
                self.advance();
                self.write_line(line);
            }
        }

        if ends_with_newline {
            self.advance();
        }

        Ok(())
    }
}

impl<'buf> Iterator for Iter<'buf> {
    type Item = &'buf str;

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.idx;
        self.idx += 1;
        if idx == self.end {
            return None;
        }
        self.buf
            .lines
            .get(idx.0 % self.buf.lines.len())
            .map(|Line { line, .. }| line.as_str())
    }
}

impl Default for BufConfig {
    fn default() -> Self {
        Self {
            line_len: 80,
            // CHOSEN BY FAIR DIE ROLL, GUARANTEED TO BE RANDOM
            lines: 120,
        }
    }
}

#[cfg(test)]
mod tests {
    use core::slice::SliceIndex;

    use super::*;

    #[test]
    fn basic() {
        let mut buf = LineBuf::new(BufConfig {
            line_len: 6,
            lines: 6,
        });
        writeln!(&mut buf, "hello").unwrap();
        writeln!(&mut buf, "world").unwrap();
        writeln!(&mut buf, "have\nlots").unwrap();
        writeln!(&mut buf, "of").unwrap();
        writeln!(&mut buf, "fun").unwrap();

        dbg!(&buf);
        let mut iter = buf.iter();
        assert_eq!(test_dbg!(iter.next()), Some("hello"));
        assert_eq!(test_dbg!(iter.next()), Some("world"));
        assert_eq!(test_dbg!(iter.next()), Some("have"));
        assert_eq!(test_dbg!(iter.next()), Some("lots"));
        assert_eq!(test_dbg!(iter.next()), Some("of"));
        assert_eq!(test_dbg!(iter.next()), Some("fun"));
        assert_eq!(test_dbg!(iter.next()), None);
    }

    #[test]
    fn buffer_wraparound() {
        let mut buf = LineBuf::new(BufConfig {
            line_len: 7,
            lines: 6,
        });
        writeln!(&mut buf, "hello").unwrap();
        writeln!(&mut buf, "world").unwrap();
        writeln!(&mut buf, "have\nlots").unwrap();
        writeln!(&mut buf, "of").unwrap();
        writeln!(&mut buf, "fun").unwrap();
        writeln!(&mut buf, "goodbye").unwrap();

        dbg!(&buf);
        let mut iter = buf.iter();
        assert_eq!(test_dbg!(iter.next()), Some("goodbye"));
        assert_eq!(test_dbg!(iter.next()), Some("world"));
        assert_eq!(test_dbg!(iter.next()), Some("have"));
        assert_eq!(test_dbg!(iter.next()), Some("lots"));
        assert_eq!(test_dbg!(iter.next()), Some("of"));
        assert_eq!(test_dbg!(iter.next()), Some("fun"));
        assert_eq!(test_dbg!(iter.next()), None);
    }

    #[test]
    fn line_wrapping() {
        let mut buf = LineBuf::new(BufConfig {
            line_len: 4,
            lines: 6,
        });
        writeln!(&mut buf, "this is a very long line").unwrap();

        dbg!(&buf);
        let mut iter = buf.iter();
        assert_eq!(test_dbg!(iter.next()), Some("this"));
        assert_eq!(test_dbg!(iter.next()), Some(" is "));
        assert_eq!(test_dbg!(iter.next()), Some("a ve"));
        assert_eq!(test_dbg!(iter.next()), Some("ry l"));
        assert_eq!(test_dbg!(iter.next()), Some("ong "));
        assert_eq!(test_dbg!(iter.next()), Some("line"));
        assert_eq!(test_dbg!(iter.next()), None);
    }

    #[test]
    fn range_iter_unbounded() {
        let mut buf = LineBuf::new(BufConfig {
            line_len: 6,
            lines: 6,
        });
        let expected = ["hello", "world", "have", "lots", "of", "fun"];
        fill(&mut buf, &expected);
        assert_slicelike(&buf, &expected, ..)
    }

    #[test]
    fn range_iter_basic() {
        let mut buf = LineBuf::new(BufConfig {
            line_len: 6,
            lines: 6,
        });

        let expected = ["hello", "world", "have", "lots", "of", "fun"];
        fill(&mut buf, &expected);
        test_range_iters(&buf, &expected)
    }

    #[test]
    fn range_iter_buf_wrapped() {
        let mut buf = LineBuf::new(BufConfig {
            line_len: 7,
            lines: 6,
        });
        writeln!(&mut buf, "hello").unwrap();
        writeln!(&mut buf, "world").unwrap();
        writeln!(&mut buf, "have\nlots").unwrap();
        writeln!(&mut buf, "of").unwrap();
        writeln!(&mut buf, "fun").unwrap();
        writeln!(&mut buf, "goodbye").unwrap();

        let expected = ["goodbye", "world", "have", "lots", "of", "fun"];
        test_range_iters(&buf, &expected);
    }

    fn test_range_iters(buf: &LineBuf, expected: &[&str]) {
        test_dbg!(buf);
        test_dbg!(expected);

        println!("\n=== range unbounded ===\n");
        assert_slicelike(&buf, &expected, ..);

        println!("\n=== range start inclusive ===\n");
        assert_slicelike(&buf, &expected, 2..);
        assert_slicelike(&buf, &expected, 3..);
        assert_slicelike(&buf, &expected, 4..);
        assert_slicelike(&buf, &expected, 5..);

        println!("\n=== range end inclusive ===\n");
        assert_slicelike(&buf, &expected, ..=2);
        assert_slicelike(&buf, &expected, ..=5);

        println!("\n=== range end exclusive ===\n");
        assert_slicelike(&buf, &expected, ..2);
        assert_slicelike(&buf, &expected, ..5);
        assert_slicelike(&buf, &expected, ..6)
    }

    fn fill(mut buf: &mut LineBuf, strs: &[&str]) {
        for item in strs {
            writeln!(buf, "{item}").unwrap();
        }
        dbg!(buf);
    }

    fn assert_slicelike<'ex>(
        buf: &LineBuf,
        expected: &'ex [&'ex str],
        range: impl RangeBounds<usize> + SliceIndex<[&'ex str], Output = [&'ex str]> + Clone,
    ) {
        let slice = &expected[range.clone()];
        let mut iter = test_dbg!(buf.lines(range));
        for &expected in slice {
            assert_eq!(Some(test_dbg!(expected)), test_dbg!(iter.next()))
        }
    }
}
