use alloc::{boxed::Box, string::String, vec::Vec};
use core::fmt::{self, Write};

/// A ring buffer of fixed-size lines.
#[derive(Debug)]
pub struct Buf {
    lines: Box<[String]>,
    /// The maximum length of each line in the buffer
    line_len: usize,
    start: usize,
    end: usize,
    started: bool,
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

impl Buf {
    pub fn new(BufConfig { line_len, lines }: BufConfig) -> Self {
        Self {
            lines: (0..lines)
                .map(|_| String::with_capacity(line_len))
                .collect::<Vec<_>>()
                .into(),
            line_len,
            start: 0,
            end: 0,
            started: false,
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

    fn advance(&mut self) -> usize {
        let end = self.end;

        self.end = self.end + 1;
        if self.end == self.start {
            self.start += 1
        }
        let idx = end % self.lines.len();
        self.started = false;
        idx
    }

    fn line(&mut self) -> &mut String {
        let line = &mut self.lines[(self.end % self.lines.len())];
        line
    }

    fn write_chunk<'s>(&mut self, s: &'s str) -> Option<&'s str> {
        let rem = self.line_len - self.line().len();
        let (line, next) = if s.len() > rem {
            let (this, next) = s.split_at(rem);
            (this, Some(next))
        } else {
            (s, None)
        };
        self.line().push_str(line);
        next
    }

    fn write_line(&mut self, mut line: &str) {
        while let Some(next) = self.write_chunk(line) {
            line = next;
            self.advance();
        }
    }
}

impl Write for &mut Buf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let ends_with_newline = s.chars().any(|ch| ch == '\n');
        let mut lines = s.split('\n');
        if let Some(line) = lines.next() {
            self.write_line(line);
            for line in lines {
                self.write_line(line);
                self.advance();
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
        if idx == self.buf.end {
            return None;
        }
        self.buf
            .lines
            .get(idx % self.buf.lines.len())
            .map(String::as_str)
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
        assert_eq!(iter.next(), Some("hello"));
        assert_eq!(iter.next(), Some("world"));
        assert_eq!(iter.next(), Some("have"));
        assert_eq!(iter.next(), Some("lots"));
        assert_eq!(iter.next(), Some("of"));
        assert_eq!(iter.next(), Some("fun"));
        assert_eq!(iter.next(), None);
    }
}
