//! A simple 16550 UART serial port driver.
use crate::cpu;
use core::marker::PhantomData;
use mycelium_util::{io, sync::spin};

lazy_static::lazy_static! {
    static ref COM1: Option<Port> = Port::new(0x3F8).ok();
    static ref COM2: Option<Port> = Port::new(0x2F8).ok();
    static ref COM3: Option<Port> = Port::new(0x3E8).ok();
    static ref COM4: Option<Port> = Port::new(0x2E8).ok();
}

pub fn com1() -> Option<&'static Port> {
    COM1.as_ref()
}

pub fn com2() -> Option<&'static Port> {
    COM2.as_ref()
}

pub fn com3() -> Option<&'static Port> {
    COM3.as_ref()
}

pub fn com4() -> Option<&'static Port> {
    COM4.as_ref()
}

// #[derive(Debug)]
pub struct Port {
    inner: spin::Mutex<Registers>,
}

// #[derive(Debug)]
pub struct Lock<'a, B = Blocking> {
    // This is the non-moveable part.
    inner: LockInner<'a>,
    _is_blocking: PhantomData<B>,
}

struct LockInner<'a> {
    inner: spin::MutexGuard<'a, Registers>,
    prev_divisor: Option<u16>,
}

// #[derive(Debug)]
struct Registers {
    data: cpu::Port,
    irq_enable: cpu::Port,
    line_ctrl: cpu::Port,
    modem_ctrl: cpu::Port,
    status: cpu::Port,
    baud_rate_divisor: u16,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Blocking {
    _p: (),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Nonblocking {
    _p: (),
}

impl Port {
    pub const MAX_BAUD_RATE: usize = 115200;

    pub fn new(port: u16) -> io::Result<Self> {
        let scratch_test = unsafe {
            const TEST_BYTE: u8 = 69;
            let scratch_port = cpu::Port::at(port + 7);
            scratch_port.writeb(TEST_BYTE);
            scratch_port.readb() == TEST_BYTE
        };

        if !scratch_test {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "scrach port was not writeable, is there a serial port at this address?",
            ));
        }

        let mut registers = Registers {
            data: cpu::Port::at(port),
            irq_enable: cpu::Port::at(port + 1),
            line_ctrl: cpu::Port::at(port + 3),
            modem_ctrl: cpu::Port::at(port + 4),
            status: cpu::Port::at(port + 5),
            baud_rate_divisor: 3,
        };
        let fifo_ctrl = cpu::Port::at(port + 2);

        // Disable all interrupts
        registers.without_irqs(|registers| unsafe {
            // Set divisor to 38400 baud
            registers.set_baud_rate_divisor(3)?;

            // 8 bits, no parity, one stop bit
            registers.line_ctrl.writeb(0x03);

            // Enable FIFO with 14-byte threshold
            fifo_ctrl.writeb(0xC7);

            // RTS/DSR set
            registers.modem_ctrl.writeb(0x0B);

            Ok::<(), io::Error>(())
        })?;

        Ok(Self {
            inner: spin::Mutex::new(registers),
        })
    }

    pub fn lock(&self) -> Lock<'_> {
        Lock {
            inner: LockInner {
                inner: self.inner.lock(),
                prev_divisor: None,
            },
            _is_blocking: PhantomData,
        }
    }
}

impl Registers {
    const DLAB_BIT: u8 = 0b1000_0000;

    fn without_irqs<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        unsafe {
            self.irq_enable.writeb(0x00);
        }
        let res = f(self);
        unsafe {
            self.irq_enable.writeb(0x01);
        }
        res
    }

    fn set_baud_rate_divisor(&mut self, divisor: u16) -> io::Result<u16> {
        let prev = self.baud_rate_divisor;
        if divisor == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "baud rate divisor must be greater than 0",
            ));
        }

        let lcr_state = unsafe { self.line_ctrl.readb() };
        if lcr_state & Self::DLAB_BIT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "DLAB bit already set, what the heck!",
            ));
        }

        unsafe {
            // set the Divisor Latch Access Bit. now, the data port and irq enable
            // port can be used to set the least and most significant bytes of the
            // divisor, respectively.
            self.line_ctrl.writeb(lcr_state | Self::DLAB_BIT);

            // least significant byte
            self.data.writeb((divisor & 0x00FF) as u8);
            // most significant byte
            self.irq_enable.writeb((divisor >> 8) as u8);

            self.line_ctrl.writeb(lcr_state);
        }

        self.baud_rate_divisor = divisor;

        Ok(prev)
    }

    #[inline]
    fn line_status(&self) -> u8 {
        unsafe { self.status.readb() }
    }

    #[inline]
    fn is_write_ready(&self) -> bool {
        self.line_status() & 0x20 != 0
    }

    #[inline]
    fn is_read_ready(&self) -> bool {
        self.line_status() & 1 != 0
    }

    #[inline]
    fn read_blocking(&mut self) -> u8 {
        while !self.is_read_ready() {}
        unsafe { self.data.readb() }
    }

    #[inline]
    fn read_nonblocking(&mut self) -> io::Result<u8> {
        if self.is_read_ready() {
            Ok(unsafe { self.data.readb() })
        } else {
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        }
    }

    #[inline]
    fn write_blocking(&mut self, byte: u8) {
        while !self.is_write_ready() {}
        unsafe { self.data.writeb(byte) }
    }

    #[inline]
    fn write_nonblocking(&mut self, byte: u8) -> io::Result<()> {
        if self.is_write_ready() {
            unsafe {
                self.data.writeb(byte);
            }
            Ok(())
        } else {
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        }
    }
}

impl<'a> Lock<'a> {
    pub fn set_non_blocking(self) -> Lock<'a, Nonblocking> {
        Lock {
            inner: self.inner,
            _is_blocking: PhantomData,
        }
    }
}

impl<'a, B> Lock<'a, B> {
    /// Set the serial port's baud rate for this `Lock`.
    ///
    /// When the `Lock` is dropped, the baud rate will be set to the previous value.
    ///
    /// # Errors
    ///
    /// This returns an `InvalidInput` error if the target baud rate exceeds the
    /// maximum (115200), if the maximum baud rate is not divisible by the
    /// target, or if the target is so low that the resulting baud rate divisor
    /// is greater than `u16::MAX` (pretty unlikely!).
    pub fn set_baud_rate(&mut self, baud: usize) -> io::Result<()> {
        if baud > Port::MAX_BAUD_RATE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot exceed max baud rate (115200)",
            ));
        }

        if Port::MAX_BAUD_RATE % baud != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "max baud rate not divisible by target",
            ));
        }

        let divisor = Port::MAX_BAUD_RATE / baud;
        if divisor > (core::u16::MAX as usize) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "divisor for target baud rate is too high!",
            ));
        }

        self.inner.set_baud_rate_divisor(divisor as u16)
    }
}

impl<'a> io::Read for Lock<'a, Blocking> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        for byte in buf.iter_mut() {
            *byte = self.inner.read_blocking();
        }
        Ok(buf.len())
    }
}

impl<'a> io::Read for Lock<'a, Nonblocking> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        for byte in buf.iter_mut() {
            *byte = self.inner.read_nonblocking()?;
        }
        Ok(buf.len())
    }
}

impl<'a> io::Write for Lock<'a, Blocking> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for &byte in buf.iter() {
            self.inner.write_blocking(byte)
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        while !self.inner.is_write_ready() {}
        Ok(())
    }
}

impl<'a> io::Write for Lock<'a, Nonblocking> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for &byte in buf.iter() {
            self.inner.write_nonblocking(byte)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        while !self.inner.is_write_ready() {}
        Ok(())
    }
}

impl<'a> LockInner<'a> {
    #[inline(always)]
    fn is_write_ready(&self) -> bool {
        self.inner.is_write_ready()
    }

    #[inline(always)]
    fn is_read_ready(&self) -> bool {
        self.inner.is_read_ready()
    }

    #[inline(always)]
    fn write_nonblocking(&mut self, byte: u8) -> io::Result<()> {
        self.inner.write_nonblocking(byte)
    }

    #[inline(always)]
    fn read_nonblocking(&mut self) -> io::Result<u8> {
        self.inner.read_nonblocking()
    }

    #[inline(always)]
    fn write_blocking(&mut self, byte: u8) {
        self.inner.write_blocking(byte)
    }

    #[inline(always)]
    fn read_blocking(&mut self) -> u8 {
        self.inner.read_blocking()
    }

    #[inline(always)]
    fn set_baud_rate_divisor(&mut self, divisor: u16) -> io::Result<()> {
        let prev: u16 = self
            .inner
            .without_irqs(|inner| inner.set_baud_rate_divisor(divisor))?;
        self.prev_divisor = Some(prev);

        Ok(())
    }
}

impl<'a> Drop for LockInner<'a> {
    fn drop(&mut self) {
        if let Some(divisor) = self.prev_divisor {
            // Disable IRQs.
            self.inner.without_irqs(|inner| {
                // Reset the previous baud rate divisor.
                let _ = inner.set_baud_rate_divisor(divisor);
            });
        }
    }
}
