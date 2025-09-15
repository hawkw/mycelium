//! A simple driver for 16550-like UARTs.
//!
//! This driver is primarily tested against the QEMU emulated 16550, which itself is intended to
//! emulate a National Semiconductor PC16550D. From QEMU's
//! [HardwareManuals](https://wiki.qemu.org/Documentation/HardwareManuals) wiki page, the emulated
//! device is described by this datasheet: <https://wiki.qemu.org/images/1/18/PC16550D.pdf>.
//!
//! The COM1-4 defined here are, again, primarily in service of QEMU's PC layout, but should be
//! relatively general.
//!
//! ## Implementation
//!
//! Ports as implemented in this driver are split into a "read" part and a "write" part, which
//! provide access to not-quite-disjoint sets of registers for a port. Ports can be written to by
//! any task ay any time, requiring the write lock to do so, but reading a port happens in an
//! interrupt context which can take the read lock at any time.
//!
//! The behavior implemented by the read half of a port must be able to tolerate execution
//! concurrent with any behavior implemented on the write half of a port, and vice versa with the
//! write half of a port tolerating any behavior implemented on the read half of a port. As a
//! trivial example, reading and writing a port both operate on the data register, but one is
//! performed with `inb` while the other is performed with `outb`; no interleaving of `inb`/`outb`
//! on the data register will cause misbehavior, so reads and writes can be split in this way.
//!
//! When a 16550 raises an interrupt, the pending interrupts cannot change as a result of a write,
//! so the read lock may read IIR. Conceivably, modifying `modem_ctrl` may change the port's
//! pending interrupts, so the write lock must not modify this register while interrupts are
//! unmasked.
//!
//! If we must operate with exclusive access to all port registers, we have to:
//!
//! * acquire the port write lock
//! * disable interrupts for the port
//! * acquire the port read lock
//! * perform the operation
//! * release the read lock
//! * enable interrupts for the port
//! * release the write lock
//!
//! This ordering is critical and depends on current serial implementation! The write lock excludes
//! other processors without blocking interrupts from completing, disabling interrupts prevents new
//! interrupts from firing, and acquiring the read lock only completes when any potentially
//! in-flight interrupt is done with the port state. If two processors race to perform management
//! operations, the write lock prevents one processor from enabling interrupts between the other
//! disabling interrupts and acquiring the read lock.
//!
//! If serial interrupts must ever take a write lock, or interrupts intermittently drop the read
//! lock, we'll have to add additional synchronization to guarantee no combination of read/write
//! operations can deadlock.

use crate::cpu;
use core::{fmt, marker::PhantomData};
use mycelium_util::{
    io,
    sync::{
        blocking::{Mutex, MutexGuard},
        spin::Spinlock,
        Lazy,
    },
};

static COM1: Lazy<Option<Port>> = Lazy::new(|| Port::new(0x3F8).ok());
static COM2: Lazy<Option<Port>> = Lazy::new(|| Port::new(0x2F8).ok());
static COM3: Lazy<Option<Port>> = Lazy::new(|| Port::new(0x3E8).ok());
static COM4: Lazy<Option<Port>> = Lazy::new(|| Port::new(0x2E8).ok());

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

// For representation simplicity, the variants here have values corresponding to the IIR bits that
// produce these variants.
#[derive(Debug)]
pub enum Pc16550dInterrupt {
    ModemStatus = 0b0000,
    TransmitterHoldingRegEmpty = 0b0010,
    ReceivedDataAvailable = 0b0100,
    ReceiverLineStatus = 0b110,
    CharacterTimeout = 0b1100,
}

// #[derive(Debug)]
pub struct Port {
    read_inner: Mutex<ReadRegisters, Spinlock>,
    write_inner: Mutex<WriteRegisters, Spinlock>,
}

/// A lock for the read parts of a serial port. The read and write parts of this port state are
/// described in the documentation for this module.
// #[derive(Debug)]
pub struct ReadLock<'a, B = Blocking> {
    // This is the non-moveable part.
    inner: ReadLockInner<'a>,
    _is_blocking: PhantomData<B>,
}

/// A lock for the write parts of a serial port. The read and write parts of this port state are
/// described in the documentation for this module.
pub struct WriteLock<'a, B = Blocking> {
    // This is the non-moveable part.
    inner: WriteLockInner<'a>,
    _is_blocking: PhantomData<B>,
}

struct ReadLockInner<'a> {
    inner: MutexGuard<'a, ReadRegisters, Spinlock>,
}

struct WriteLockInner<'a> {
    inner: MutexGuard<'a, WriteRegisters, Spinlock>,
    prev_divisor: Option<u16>,
}

/// The registers involved in handling a read of the UART.
///
/// This is closely related to the registers and implementation around `WriteRegisters`; we will
/// concurrently access the port for reading and writing. While `data` is used in both reading and
/// writing, it is only read while reading, and only written while writing, which means readers and
/// writers do not interfere with one another.
// #[derive(Debug)]
struct ReadRegisters {
    data: cpu::Port,
    // This register is called the Interrupt Identification Register ("IIR") in the PC16550D
    // datasheet from which QEMU's implementation is derived, but other datasheets for similar
    // parts also call this the "Interrupt Status Register" or "ISR".
    iir: cpu::Port,
    status: cpu::Port,
}

// #[derive(Debug)]
struct WriteRegisters {
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
    pub const MAX_BAUD_RATE: usize = 115_200;

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

        let read_registers = ReadRegisters {
            data: cpu::Port::at(port),
            iir: cpu::Port::at(port + 2),
            status: cpu::Port::at(port + 5),
        };
        let mut write_registers = WriteRegisters {
            data: cpu::Port::at(port),
            irq_enable: cpu::Port::at(port + 1),
            line_ctrl: cpu::Port::at(port + 3),
            modem_ctrl: cpu::Port::at(port + 4),
            status: cpu::Port::at(port + 5),
            baud_rate_divisor: 3,
        };
        let fifo_ctrl = cpu::Port::at(port + 2);

        // Disable all interrupts
        write_registers.without_irqs(|registers| unsafe {
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
            read_inner: Mutex::new_with_raw_mutex(read_registers, Spinlock::new()),
            write_inner: Mutex::new_with_raw_mutex(write_registers, Spinlock::new()),
        })
    }

    pub fn read_lock(&self) -> ReadLock<'_> {
        ReadLock {
            inner: ReadLockInner {
                inner: self.read_inner.lock(),
            },
            _is_blocking: PhantomData,
        }
    }

    pub fn write_lock(&self) -> WriteLock<'_> {
        WriteLock {
            inner: WriteLockInner {
                inner: self.write_inner.lock(),
                prev_divisor: None,
            },
            _is_blocking: PhantomData,
        }
    }

    /// Forcibly unlock the serial port, releasing any locks held by other cores
    /// or in other functions.
    ///
    /// # Safety
    ///
    ///  /!\ only call this when oopsing!!! /!\
    pub unsafe fn force_unlock(&self) {
        self.read_inner.force_unlock();
        self.write_inner.force_unlock();
    }
}

impl ReadRegisters {
    #[inline]
    fn iir(&self) -> u8 {
        unsafe { self.iir.readb() }
    }

    #[inline]
    fn line_status(&self) -> u8 {
        unsafe { self.status.readb() }
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
}

impl WriteRegisters {
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

impl<'a> ReadLock<'a> {
    pub fn set_non_blocking(self) -> ReadLock<'a, Nonblocking> {
        ReadLock {
            inner: self.inner,
            _is_blocking: PhantomData,
        }
    }
}

impl<'a> WriteLock<'a> {
    pub fn set_non_blocking(self) -> WriteLock<'a, Nonblocking> {
        WriteLock {
            inner: self.inner,
            _is_blocking: PhantomData,
        }
    }
}

impl<B> ReadLock<'_, B> {
    pub fn check_interrupt_type(&mut self) -> io::Result<Option<Pc16550dInterrupt>> {
        // IIR bits 0 through 3 describe what happened to produce an interrupt, with bits 4 and 5
        // always 0, and bits 6 and 7 set to 1 if `FCR0=1`.
        let iir = self.inner.inner.iir();

        if iir & 0b0011_0000 != 0b0000_0000 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "IIR indicates bogus interrupt bits"
            ));
        }

        // TODO(ixi): probably should check IIR bits 6 and 7? punting on everything related to FIFO
        // though.

        let interrupt = match self.inner.inner.iir() & 0b1111 {
            0b0000 => Pc16550dInterrupt::ModemStatus,
            0b0001 => { return Ok(None); }
            0b0010 => Pc16550dInterrupt::TransmitterHoldingRegEmpty,
            0b0100 => Pc16550dInterrupt::ReceivedDataAvailable,
            0b0110 => Pc16550dInterrupt::ReceiverLineStatus,
            0b1100 => Pc16550dInterrupt::CharacterTimeout,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "IIR indicates unrecognized status"
                ));
            }
        };

        Ok(Some(interrupt))
    }
}

impl<B> WriteLock<'_, B> {
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
        if divisor > (u16::MAX as usize) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "divisor for target baud rate is too high!",
            ));
        }

        self.inner.set_baud_rate_divisor(divisor as u16)
    }
}

impl io::Read for ReadLock<'_, Blocking> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        for byte in buf.iter_mut() {
            *byte = self.inner.read_blocking();
        }
        Ok(buf.len())
    }
}

impl io::Read for ReadLock<'_, Nonblocking> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        for byte in buf.iter_mut() {
            // not ideal that this loses how many bytes were read if any read would block
            *byte = self.inner.read_nonblocking()?;
        }
        Ok(buf.len())
    }
}

impl io::Write for WriteLock<'_, Blocking> {
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

impl fmt::Write for WriteLock<'_, Blocking> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.inner.write_blocking(byte)
        }
        Ok(())
    }
}

impl io::Write for WriteLock<'_, Nonblocking> {
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

impl ReadLockInner<'_> {
    #[inline(always)]
    fn read_nonblocking(&mut self) -> io::Result<u8> {
        self.inner.read_nonblocking()
    }

    #[inline(always)]
    fn read_blocking(&mut self) -> u8 {
        self.inner.read_blocking()
    }
}

impl WriteLockInner<'_> {
    #[inline(always)]
    fn is_write_ready(&self) -> bool {
        self.inner.is_write_ready()
    }

    #[inline(always)]
    fn write_nonblocking(&mut self, byte: u8) -> io::Result<()> {
        self.inner.write_nonblocking(byte)
    }

    #[inline(always)]
    fn write_blocking(&mut self, byte: u8) {
        self.inner.write_blocking(byte)
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

impl Drop for WriteLockInner<'_> {
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

impl<'a> mycelium_trace::writer::MakeWriter<'a> for &Port {
    type Writer = WriteLock<'a, Blocking>;
    fn make_writer(&'a self) -> Self::Writer {
        self.write_lock()
    }

    fn line_len(&self) -> usize {
        120
    }
}
