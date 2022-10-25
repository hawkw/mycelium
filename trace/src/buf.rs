use alloc::{boxed::Box, string::String, vec::Vec};
use core::fmt::{self, Write};

/// A ring buffer of fixed-size lines.
#[derive(Debug)]
pub struct Buf {
    lines: Box<[Line]>,
    /// The maximum length of each line in the buffer
    line_len: usize,
    start: usize,
    end: usize,
}

#[derive(Copy, Clone, Debug)]
pub struct BufConfig {
    pub line_len: usize,
    pub lines: usize,
}

pub struct Writer<'buf> {
    buf: &'buf mut Buf,
    idx: usize,
}

pub struct Iter<'buf> {
    buf: &'buf Buf,
    idx: usize,
}

#[derive(Debug)]
struct Line {
    line: String,
    stamp: usize,
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

impl Buf {
    pub fn new(BufConfig { line_len, lines }: BufConfig) -> Self {
        Self {
            lines: (0..lines)
                .map(|stamp| Line {
                    stamp,
                    line: String::with_capacity(line_len),
                })
                .collect::<Vec<_>>()
                .into(),
            line_len,
            start: 0,
            end: 0,
        }
    }

    pub fn writer(&mut self) -> Writer<'_> {
        let idx = self.end;
        Writer { buf: self, idx }
    }

    pub fn iter(&self) -> Iter<'_> {
        Iter {
            idx: self.start,
            buf: self,
        }
    }

    fn advance(&mut self) {
        self.end += 1;
    }

    fn line_mut(&mut self) -> &mut String {
        let Line { stamp, line } = &mut self.lines[(self.end % self.lines.len())];
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
        while let Some(next) = test_dbg!(self.write_chunk(dbg!(line))) {
            line = next;
            test_dbg!(self.advance());
        }
    }
}

impl Write for &mut Buf {
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
        if idx == self.buf.end {
            return None;
        }
        self.buf
            .lines
            .get(idx % self.buf.lines.len())
            .map(|Line { line, .. }| line.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let mut buf = Buf::new(BufConfig {
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
}
