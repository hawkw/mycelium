use super::IsaInterrupt;
use crate::cpu;
use hal_core::interrupt::{Handlers, RegistrationError};

#[derive(Debug)]
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

    /// Mask the provided interrupt number.
    unsafe fn mask(&mut self, num: u8) {
        debug_assert!(num < 8);
        // read the current value of the Interrupt Mask Register (IMR).
        let imr = self.data.readb();
        // set the bit corresponding to the interrupt number to 1.
        self.data.writeb(imr | (1 << num));
    }

    /// Unmask the provided interrupt number.
    unsafe fn unmask(&mut self, num: u8) {
        debug_assert!(num < 8);
        // read the current value of the Interrupt Mask Register (IMR).
        let imr = self.data.readb();
        // clear the bit corresponding to the interrupt number to 1.
        self.data.writeb(imr & !(1 << num));
    }
}

#[derive(Debug)]
pub struct CascadedPic {
    sisters: PicSisters,
}

// two of them

#[derive(Debug)]
struct PicSisters {
    big: Pic,
    little: Pic,
}

impl PicSisters {
    pub(crate) const fn new() -> Self {
        Self {
            // primary and secondary PIC addresses (0 and 8) are Just How IBM Did It.
            // yes, they overlap with x86 interrupt numbers. it's not good.
            // port numbers are also magic IBM PC/AT architecture numbers.
            big: Pic::new(0, 0x20, 0x21),
            little: Pic::new(8, 0xa0, 0xa1),
        }
    }
}

impl CascadedPic {
    pub(crate) const fn new() -> Self {
        Self {
            sisters: PicSisters::new(),
        }
    }

    pub(crate) fn mask(&mut self, irq: IsaInterrupt) {
        let (pic, num) = self.pic_for_irq(irq);
        unsafe {
            pic.mask(num);
        }
    }

    pub(crate) fn unmask(&mut self, irq: IsaInterrupt) {
        let (pic, num) = self.pic_for_irq(irq);
        unsafe {
            pic.unmask(num);
        }
    }

    fn pic_for_irq(&mut self, irq: IsaInterrupt) -> (&mut Pic, u8) {
        let num = irq as u8;
        if num >= 8 {
            (&mut self.sisters.little, num - 8)
        } else {
            (&mut self.sisters.big, num)
        }
    }

    pub(crate) fn end_interrupt(&mut self, irq: IsaInterrupt) {
        const END_INTERRUPT: u8 = 0x20; // from osdev wiki
        let num = irq as u8;
        if num >= 8 {
            unsafe {
                self.sisters.little.command.writeb(END_INTERRUPT);
            }
        }

        unsafe {
            self.sisters.big.command.writeb(END_INTERRUPT);
        }
    }
}

impl hal_core::interrupt::Control for CascadedPic {
    type Registers = super::Registers;
    fn register_handlers<H>(&mut self) -> Result<(), hal_core::interrupt::RegistrationError>
    where
        H: Handlers<super::Registers>,
    {
        Err(RegistrationError::other(
            "x86_64 handlers must be registered via the IDT, not to the PIC interrupt component",
        ))
    }

    unsafe fn disable(&mut self) {
        self.sisters.big.data.writeb(0xff);
        self.sisters.little.data.writeb(0xff);
    }

    unsafe fn enable(&mut self) {
        // TODO(ixi): confirm this?? it looks like "disable" is "write a 1 to set the line masked"
        //            so maybe it stands to reason that writing a 0 unmasks an interrupt?
        self.sisters.big.data.writeb(0x00);
        self.sisters.little.data.writeb(0x00);
    }

    fn is_enabled(&self) -> bool {
        unimplemented!("ixi do this one!!!")
    }
}

impl CascadedPic {
    pub(crate) unsafe fn set_irq_address(&mut self, primary_start: u8, secondary_start: u8) {
        // iowait and its uses below are guidance from the osdev wiki for compatibility with "older
        // machines". it is not entirely clear what "older machines" exactly means, or where this
        // is or is not necessary precisely. this code happens to work in qemu without `iowait()`,
        // but is largely untested on real hardware where this may be a concern.
        let iowait = || cpu::Port::at(0x80).writeb(0);

        let primary_mask = self.sisters.big.data.readb();
        let secondary_mask = self.sisters.little.data.readb();

        const EXTENDED_CONFIG: u8 = 0x01; // if present, there are four initialization control words
        const PIC_INIT: u8 = 0x10; // reinitialize the 8259 PIC

        self.sisters.big.command.writeb(PIC_INIT | EXTENDED_CONFIG);
        iowait();
        self.sisters
            .little
            .command
            .writeb(PIC_INIT | EXTENDED_CONFIG);
        iowait();
        self.sisters.big.data.writeb(primary_start);
        iowait();
        self.sisters.little.data.writeb(secondary_start);
        iowait();
        // magic number: secondary pic is at cascade identity 2 (how does 4 say this ???)
        // !UPDATE! 4 says this because this word is a bitmask:
        //
        //   76543210
        // 0b00000100
        //        |
        //        --- bit 2 set means there is another 8259 cascaded at address 2
        //
        // bit 0 would be set if there was only one controller in the system, in `single` mode, and
        // is non-zero in all other modes. the exact read from the intel manual is not immediately
        // clear:
        // ```
        // in sisters mode, a `1` is set for each little sister in the system.
        // ```
        // which means bit 1 indicates a little sister at cascade identity 1, or, as in an IBM
        // PC/AT system, a little sister is present at cascade identity 2, indicated by bit 2 being
        // set for a bitmask of 0b0000_0100. there is a slightly faded diagram that describes ICW3
        // which has not been OCR'd, in the copy of the intel document you might find online.
        self.sisters.big.data.writeb(4);
        iowait();
        // magic number: secondary pic has cascade identity 2 (??)
        // !UPDATE! that's just how IBM PC/AT systems be
        self.sisters.little.data.writeb(2);
        iowait();
        self.sisters.big.data.writeb(1); // 8086/88 (MCS-80/85) mode
        iowait();
        self.sisters.little.data.writeb(1); // 8086/88 (MCS-80/85) mode
        iowait();

        self.sisters.big.data.writeb(primary_mask);
        iowait();
        self.sisters.little.data.writeb(secondary_mask);
        iowait();
        self.sisters.big.address = primary_start;
        self.sisters.little.address = secondary_start;
    }
}
