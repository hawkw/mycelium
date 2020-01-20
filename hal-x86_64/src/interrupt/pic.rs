use crate::cpu;
use hal_core::interrupt::{Handlers, RegistrationError};

pub(crate) struct Pic {
    address: u8,
    command: cpu::Port,
    data: cpu::Port,
}

impl Pic {
    const fn new(address: u8, command: u16, data: u16) -> Self {
        Self {
            address,
            command: cpu::Port::at(command),
            data: cpu::Port::at(data),
        }
    }
}

pub struct CascadedPic {
    primary: Pic,
    secondary: Pic,
}

impl CascadedPic {
    pub(crate) const fn new() -> Self {
        Self {
            // primary and secondary PIC addresses (0 and 8) are Just How IBM Did It.
            // yes, they overlap with x86 interrupt numbers. it's not good.
            // port numbers are also magic IBM PC/AT architecture numbers.
            primary: Pic::new(0, 0x20, 0x21),
            secondary: Pic::new(8, 0xa0, 0xa1),
        }
    }

    pub(crate) fn end_interrupt(&mut self, num: u8) {
        const END_INTERRUPT: u8 = 0x20; // from osdev wiki
        if num >= self.secondary.address && num < self.secondary.address + 8 {
            unsafe {
                self.secondary.command.writeb(END_INTERRUPT);
            }
        }

        unsafe {
            self.primary.command.writeb(END_INTERRUPT);
        }
    }
}

impl hal_core::interrupt::Control for CascadedPic {
    fn register_handlers<H>(&mut self) -> Result<(), hal_core::interrupt::RegistrationError>
    where
        H: Handlers,
    {
        Err(RegistrationError::other(
            "x86_64 handlers must be registered via the IDT, not to the PIC interrupt component",
        ))
    }

    unsafe fn disable(&mut self) {
        self.primary.data.writeb(0xff);
        self.secondary.data.writeb(0xff);
    }

    unsafe fn enable(&mut self) {
        // TODO(ixi): confirm this?? it looks like "disable" is "write a 1 to set the line masked"
        //            so maybe it stands to reason that writing a 0 unmasks an interrupt?
        self.primary.data.writeb(0x00);
        self.secondary.data.writeb(0x00);
    }

    fn is_enabled(&self) -> bool {
        unimplemented!("ixi do this one!!!")
    }
}

impl CascadedPic {
    pub(crate) unsafe fn set_irq_addresses(&mut self, primary_start: u8, secondary_start: u8) {
        // iowait and its uses below are guidance from the osdev wiki for compatibility with "older
        // machines". it is not entirely clear what "older machines" exactly means, or where this
        // is or is not necessary precisely. this code happens to work in qemu without `iowait()`,
        // but is largely untested on real hardware where this may be a concern.
        let iowait = || cpu::Port::at(0x80).writeb(0);

        let primary_mask = self.primary.data.readb();
        let secondary_mask = self.secondary.data.readb();

        const EXTENDED_CONFIG: u8 = 0x01; // if present, there are four initialization control words
        const PIC_INIT: u8 = 0x10; // reinitialize the 8259 PIC

        self.primary.command.writeb(PIC_INIT | EXTENDED_CONFIG);
        iowait();
        self.secondary.command.writeb(PIC_INIT | EXTENDED_CONFIG);
        iowait();
        self.primary.data.writeb(primary_start);
        iowait();
        self.secondary.data.writeb(secondary_start);
        iowait();
        self.primary.data.writeb(4); // magic number: secondary pic is at IRQ2 (how does 4 say this ???)
        iowait();
        self.secondary.data.writeb(2); // magic number: secondary pic has cascade identity 2 (??)
        iowait();
        self.primary.data.writeb(1); // 8086/88 (MCS-80/85) mode
        iowait();
        self.secondary.data.writeb(1); // 8086/88 (MCS-80/85) mode
        iowait();

        self.primary.data.writeb(primary_mask);
        iowait();
        self.secondary.data.writeb(secondary_mask);
        iowait();
        self.primary.address = primary_start;
        self.secondary.address = secondary_start;
    }
}
