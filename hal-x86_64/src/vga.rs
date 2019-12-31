use core::{cmp, fmt, ptr};
use mycelium_util::{io, sync::spin};

lazy_static::lazy_static! {
    static ref BUFFER: spin::Mutex<Buffer> = spin::Mutex::new(Buffer {
        col: 0,
        row: 0,
        indent: 0,
        chars_this_line: false,
        color: ColorSpec::new(Color::LightGray, Color::Black),
        buf: unsafe { &mut *(0xb8000 as *mut Buf) },
    });
}

pub fn writer() -> Writer {
    Writer(())
}

#[derive(Debug)]
pub struct Writer(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorSpec(u8);

pub struct Buffer {
    col: usize,
    row: usize,
    indent: usize,
    chars_this_line: bool,
    color: ColorSpec,
    buf: &'static mut Buf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct Character {
    ch: u8,
    color: ColorSpec,
}

const BUF_HEIGHT: usize = 25;
const BUF_WIDTH: usize = 80;

type Buf = [[Character; BUF_WIDTH]; BUF_HEIGHT];

impl ColorSpec {
    pub const fn new(fg: Color, bg: Color) -> Self {
        let bg = (bg as u8) << 4;
        Self(bg | (fg as u8))
    }
}

impl Character {
    fn write(&mut self, ch: Character) {
        unsafe {
            // safety: we have mutable access to this character.
            ptr::write_volatile(self as *mut Character, ch)
        }
    }
}

impl Buffer {
    pub fn set_color(&mut self, color: ColorSpec) {
        self.color = color;
    }

    fn blank(&self) -> Character {
        self.character(b' ')
    }

    fn character(&self, ch: u8) -> Character {
        Character {
            ch,
            color: self.color,
        }
    }

    fn write(&mut self, s: &str) {
        assert!(s.is_ascii(), "VGA buffer does not support Unicode");
        for ch in s.bytes() {
            self.write_char(ch);
        }
    }

    fn write_char(&mut self, ch: u8) {
        if ch == b'\n' {
            self.newline();
            return;
        }

        if self.col >= (BUF_WIDTH - 1) {
            if self.row >= (BUF_HEIGHT - 1) {
                self.newline();
            } else {
                self.row += 1;
                self.col = 0;
            }
        }

        self.buf[self.row][self.col].write(self.character(ch));
        self.col += 1;

        if ch != b' ' {
            self.chars_this_line = true;
        }
    }

    fn newline(&mut self) {
        if self.row >= (BUF_HEIGHT - 1) {
            let mut rows = self.buf.iter_mut();
            let mut prev_row = rows.next().expect("fixed size buf should have rows");
            for row in rows {
                for (i, ch) in row.iter().enumerate() {
                    prev_row[i].write(*ch);
                }
                prev_row = row;
            }
        } else {
            self.row += 1;
        }

        let blank = self.blank();
        for c in &mut self.buf[self.row][..] {
            c.write(blank)
        }

        self.col = self.indent;
        self.chars_this_line = false;
    }
}

impl fmt::Write for Buffer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write(s);
        Ok(())
    }
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Buffer")
            .field("row", &self.row)
            .field("col", &self.col)
            .field("color", &self.color)
            .field("buf", &format_args!("{:p}", self.buf))
            .finish()
    }
}

impl Writer {
    pub fn set_color(&mut self, color: ColorSpec) {
        BUFFER.lock().set_color(color);
    }

    pub fn indent(&mut self, indent: u32) {
        self.dent(indent as isize)
    }

    pub fn outdent(&mut self, outdent: u32) {
        self.dent(-(outdent as isize))
    }

    /// Indent or outdent the start of each line by the requested number of
    /// characters.
    pub fn dent(&mut self, dent: isize) {
        let mut lock = BUFFER.lock();
        if dent > 0 {
            lock.indent = lock.indent.saturating_add(dent as usize);
        } else {
            lock.indent = lock.indent.saturating_sub((-dent) as usize)
        }
        if !lock.chars_this_line {
            lock.col = lock.indent;
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        BUFFER.lock().write(s);
        Ok(())
    }
}

impl io::Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut lock = BUFFER.lock();
        for &byte in buf.iter() {
            lock.write_char(byte)
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
