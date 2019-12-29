use crate::cpu;
use core::marker::PhantomData;
use mycelium_util::{io, sync::spin};

lazy_static::lazy_static! {
    static ref COM1: Option<Port> = Port::new(0x3F8).ok();
    static ref COM2: Option<Port> = Port::new(0x2F8).ok();
    static ref COM3: Option<Port> = Port::new(0x3E8).ok();
    static ref COM4: Option<Port> = Port::new(0x2E8).ok();
}

pub fn com1() -> Option<&Port> {
    com1.as_ref()
}

pub fn com2() -> Option<&Port> {
    com2.as_ref()
}

pub fn com3() -> Option<&Port> {
    com3.as_ref()
}

pub fn com4() -> Option<&Port> {
    com4.as_ref()
}

// #[derive(Debug)]
pub struct Port {
    inner: spin::Mutex<Registers>,
}

// #[derive(Debug)]
pub struct Lock<'a, B = Blocking> {
    inner: spin::MutexGuard<'a, Registers>,
    _is_blocking: PhantomData<B>,
}

// #[derive(Debug)]
struct Registers {
    data: cpu::Port,
    irq_enable: cpu::Port,
    line_ctrl: cpu::Port,
    modem_ctrl: cpu::Port,
    status: cpu::Port,
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

        let data = cpu::Port::at(port);
        let irq_enable = cpu::Port::at(port + 1);
        let fifo_ctrl = cpu::Port::at(port + 2);
        let line_ctrl = cpu::Port::at(port + 3);
        let modem_ctrl = cpu::Port::at(port + 4);

        unsafe {
            // Disable all interrupts
            irq_enable.writeb(0x0);

            // Enable DLAB (set baud rate divisor)
            line_ctrl.writeb(0x80);

            // Set divisor to 38400 baud
            data.writeb(0x03); // divisor high byte
            irq_enable.writeb(0x00); // divisor low byte

            // 8 bits, no parity, one stop bit
            line_ctrl.writeb(0x03);

            // Enable FIFO with 14-byte threshold
            fifo_ctrl.writeb(0xC7);

            // RTS/DSR set
            modem_ctrl.writeb(0x0B);
            // IRQs enabled
            irq_enable.writeb(0x01);
        }

        let status = cpu::Port::at(port + 5);

        Ok(Self {
            inner: spin::Mutex::new(Registers {
                data,
                irq_enable,
                line_ctrl,
                modem_ctrl,
                status,
            }),
        })
    }

    pub fn lock(&self) -> Lock<'_> {
        Lock {
            inner: self.inner.lock(),
            _is_blocking: PhantomData,
        }
    }
}

impl Registers {
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
