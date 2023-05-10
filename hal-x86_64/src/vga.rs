use core::fmt;
use mycelium_util::{
    io,
    sync::{spin, Lazy},
};
use volatile::Volatile;
static BUFFER: Lazy<spin::Mutex<Buffer>> = Lazy::new(|| {
    spin::Mutex::new(Buffer {
        col: 0,
        row: 0,
        color: ColorSpec::new(Color::LightGray, Color::Black),
        buf: Volatile::new(unsafe { &mut *(0xb8000u64 as *mut Buf) }),
    })
});

pub fn writer() -> Writer {
    Writer(())
}

/// # Safety
/// fuck off
pub unsafe fn init_with_offset(offset: u64) {
    // lmao
    BUFFER.lock().buf = Volatile::new(&mut *((0xb8000u64 + offset) as *mut Buf));
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
    color: ColorSpec,
    buf: Volatile<&'static mut Buf>,
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

        let ch = self.character(ch);
        self.buf
            .as_mut_slice()
            .index_mut(self.row)
            .as_mut_slice()
            .index_mut(self.col)
            .write(ch);
        self.col += 1;
    }

    fn newline(&mut self) {
        let blank = self.blank();
        let mut buf = self.buf.as_mut_slice();
        if self.row >= (BUF_HEIGHT - 1) {
            for row in 1..(BUF_HEIGHT - 1) {
                buf.copy_within(row..=row, row);
            }
        } else {
            self.row += 1;
        }

        let mut row = buf.index_mut(self.row);
        let mut row = row.as_mut_slice();
        // XXX(eliza): it would be cool if this could be a memset...
        for i in 0..BUF_WIDTH {
            row.index_mut(i).write(blank);
        }

        self.col = 0;
    }

    fn clear(&mut self) {
        let ch = self.character(b' ');
        let mut buf = self.buf.as_mut_slice();
        // XXX(eliza): it would be cool if this could also be a ``smemset`.
        for row in 0..BUF_HEIGHT {
            let mut row = buf.index_mut(row);
            let mut row = row.as_mut_slice();
            for col in 0..BUF_WIDTH {
                row.index_mut(col).write(ch);
            }
        }
        self.row = 0;
        self.col = 0;
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
        let Self {
            row,
            col,
            color,
            buf: _,
        } = self;
        f.debug_struct("vga::Buffer")
            .field("row", row)
            .field("col", col)
            .field("color", color)
            .field("buf", &format_args!("[..]"))
            .finish()
    }
}

impl Writer {
    pub fn set_color(&mut self, color: ColorSpec) {
        BUFFER.lock().set_color(color);
    }

    pub fn clear(&mut self) {
        BUFFER.lock().clear();
    }

    /// Forcibly unlock the VGA buffer's mutex.
    ///
    /// If a lock is currently held, it will be released, regardless of who's
    /// holding it. Of course, this is **outrageously, disgustingly unsafe** and
    /// you should never do it.
    ///
    /// # Safety
    ///
    /// This deliberately violates mutual exclusion.
    ///
    /// Only call this method when it is _guaranteed_ that no stack frame that
    /// has previously locked the mutex will ever continue executing.
    /// Essentially, this is only okay to call when the kernel is oopsing and
    /// all code running on other cores has already been killed.
    #[doc(hidden)]
    pub unsafe fn force_unlock(&mut self) {
        BUFFER.force_unlock();
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
