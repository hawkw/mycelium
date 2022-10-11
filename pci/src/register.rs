//! A PCI device's [`Status`] and [`Command`] registers.
use mycelium_bitfield::{bitfield, FromBits};

bitfield! {
    /// A PCI device's `Status` register.
    ///
    /// See <https://wiki.osdev.org/Pci#Status_Register>
    #[derive(Eq, PartialEq)]
    pub struct Status<u16> {
        const _RES0 = 2;
        /// Interrupt Status.
        ///
        /// Represents the state of the device's `INTx#` signal. If set to 1 and
        /// bit 10 of the [`Command`] register ([`INTERRUPT_DISABLE`]) is set to
        /// 0, the signal will be asserted; otherwise, the signal will be ignored.
        ///
        /// [`INTERUPT_DISABLE`]: Command::INTERRUPT_DISABLE
        pub const INTERRUPT_STATUS: bool;

        /// Capabilities List
        ///
        /// If set to 1, the device implements the pointer for a New Capabilities
        /// linked list at offset `0x34`; otherwise, the linked list is not available.
        pub const CAPABILITIES_LIST: bool;

        /// 66 MHz Capable
        ///
        /// If set to 1 the device is capable of running at 66 MHz; otherwise,
        /// the device runs at 33 MHz.
        pub const IS_66MHZ_CAPABLE: bool;

        /// As of revision 3.0 of the PCI Local Bus specification this bit is
        /// reserved. In revision 2.1 of the specification this bit was used to
        /// indicate whether or not a device supported User Definable Features
        const _RES1 = 1;

        /// Fast Back-To-Back Capable
        ///
        /// If set to 1 the device can accept fast back-to-back transactions
        /// that are not from the same agent; otherwise, transactions can only
        /// be accepted from the same agent.
        pub const FAST_BACK_TO_BACK_CAPABLE: bool;

        /// Master Data Parity Error
        ///
        /// This bit is only set when the following conditions are met. The bus
        /// agent asserted `PERR#` on a read or observed an assertion of `PERR#`
        /// on a write, the agent setting the bit  acted as the bus master for
        /// the operation in which the error occurred, and bit 6 of the
        /// [`Command`] register ([`PARITY_ERROR_RESPONSE`] bit) is set to 1.
        ///
        /// [`PARITY_ERROR_RESPONSE`]: Command::PARITY_ERROR_RESPONSE
        pub const MASTER_DATA_PARITY_ERROR: bool;

        /// `DEVSEL#` Timing
        ///
        /// Read-only bits that represent the slowest time that a device will
        /// assert `DEVSEL#` for any bus command except Configuration Space read
        /// and writes. A value of 0x0 represents fast timing, a value of
        /// 0x1 represents medium timing, and a value of 0x2 represents slow
        /// timing.
        pub const DEVSEL_TIMING: DevselTiming;

        /// Signalled Target Abort
        ///
        /// This bit will be set to 1 whenever a target device terminates a
        /// transaction with Target-Abort.
        pub const SIGNALLED_TARGET_ABORT: bool;

        /// Received Target Abort
        ///
        /// This bit will be set to 1, by a master device, whenever its
        /// transaction is terminated with Target-Abort.
        pub const RECEIVED_TARGET_ABORT: bool;

        /// Received Master Abort
        ///
        /// This bit will be set to 1, by a master device, whenever its
        /// transaction (except for Special Cycle transactions) is terminated
        /// with Master-Abort.
        pub const RECEIVED_MASTER_ABORT: bool;

        /// Signalled System Error
        ///
        /// This bit will be set to 1 whenever the device asserts `SERR#`.
        pub const SIGNALLED_SYSTEM_ERROR: bool;

        /// Detected Parity Error
        ///
        /// This bit will be set to 1 whenever the device detects a parity
        /// error, even if parity error handling is disabled.
        pub const DETECTED_PARITY_ERROR: bool;
    }
}

bitfield! {

    /// A PCI device's `Command` register.
    ///
    /// See <https://wiki.osdev.org/Pci#Command_Register>
    #[derive(Eq, PartialEq)]
    pub struct Command<u16> {
        /// I/O Space Enabled
        ///
        /// If set to 1 the device can respond to I/O Space accesses; otherwise,
        /// the device's response is disabled.
        pub const IO_SPACE_ENABLED: bool;

        /// Memory Space Enabled
        ///
        /// If set to 1 the device can respond to Memory Space accesses;
        /// otherwise, the device's response is disabled.
        pub const MEMORY_SPACE_ENABLED: bool;

        /// Bus Master
        ///
        /// If set to 1, the device can behave as a bus master; otherwise, the
        /// device can not generate PCI accesses.
        pub const BUS_MASTER: bool;

        /// Special Cycle Enabled
        ///
        /// If set to 1 the device can monitor Special Cycle operations;
        /// otherwise, the device will ignore them.
        pub const SPECIAL_CYCLE_ENABLE: bool;

        /// Memory Write and Invalidate Enabled
        ///
        /// If set to 1 the device can generate the Memory Write and Invalidate
        /// command; otherwise, the Memory Write command must be used.
        pub const MEMORY_WRITE_AND_INVALIDATE_ENABLED: bool;

        /// VGA Palette Snoop
        ///
        /// If set to 1 the device does not respond to palette register writes
        /// and will snoop the data; otherwise, the device will treat palette
        /// write accesses like all other accesses.
        pub const VGA_PALETTE_SNOOP: bool;

        /// Parity Error Response enabled
        ///
        /// If set to 1 the device will take its normal action when a parity
        /// error is detected; otherwise, when an error is detected, the device
        /// will set bit 15 of the [`Status`] register
        /// ([`DETECTED_PARITY_ERROR`]), but will not assert the PERR# (Parity
        /// Error) pin and will continue operation as normal.
        ///
        /// [`DETECTED_PARITY_ERROR`]: Status::DETECTED_PARITY_ERROR
        pub const PARITY_ERROR_RESPONSE_ENABLED: bool;

        /// As of revision 3.0 of the PCI local bus specification this bit is
        /// hardwired to 0. In earlier versions of the specification this bit
        /// was used by devices and may have been hardwired to 0, 1, or
        /// implemented as a read/write bit.
        const _RES0 = 1;

        /// `SERR#` Enabled
        ///
        /// If set to 1 the `SERR#` driver is enabled; otherwise, the driver is disabled.
        pub const SERR_ENABLED: bool;

        /// Fast Back-To-Back Enabled
        ///
        /// If set to 1, indicates a device is allowed to generate fast
        /// back-to-back transactions; otherwise, fast back-to-back
        /// transactions are only allowed to the same agent.
        pub const FAST_BACK_TO_BACK_ENABLED: bool;

        /// Interrupt Disable
        ///
        /// If set to 1, the assertion of the devices `INTx#` signal is
        /// disabled; otherwise, assertion of the signal is enabled.
        pub const INTERRUPT_DISABLE: bool;
    }
}

bitfield! {
    /// Built-In Self Test (BIST) register.
    #[derive(Eq, PartialEq)]
    pub struct Bist<u8> {
        /// The completion code set by running a built-in self test.
        ///
        /// If the test completed successfully, this should be 0.
        pub const COMPLETION_CODE = 3;

        /// Reserved
        const _RES = 2;

        /// Start BIST
        ///
        /// Set to 1 by the OS to start a BIST. This bit is reset when BIST
        /// completes. If BIST does not complete after 2 seconds the device
        /// should be failed by system software.
        pub const START_BIST: bool;
        /// BIST Capable
        ///
        /// If this is 1, the device supports BIST. If it is 0, this device does
        /// not support a built-in self test.
        pub const BIST_CAPABLE: bool;
    }
}

impl Bist {
    /// Returns the device's BIST completion code, if a BIST has completed.
    ///
    /// If the BIST is still in progress, this method returns `None`.
    pub fn completion_code(self) -> Option<u8> {
        if self.get(Self::START_BIST) {
            return None;
        }

        Some(self.get(Self::COMPLETION_CODE))
    }
}

/// Slowest time that a device will assert `DEVSEL#` for any bus command except
/// Configuration Space reads/writes.
#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum DevselTiming {
    Fast = 0x0,
    Medium = 0x1,
    Slow = 0x2,
}

bitfield! {
    pub(crate) struct RegisterWord<u32> {
        pub(crate) const COMMAND: Command;
        pub(crate) const STATUS: Status;
    }
}

// === impl Status ===

impl Status {
    /// Returns `true` if this device received an abort (either a Master Abort
    /// or Target Abort).
    ///
    /// This returns `true` if the [`RECEIVED_MASTER_ABORT`] or
    /// [`RECEIVED_TARGET_ABORT`] bits are set.
    pub fn received_abort(self) -> bool {
        self.get(Self::RECEIVED_MASTER_ABORT) || self.get(Self::RECEIVED_TARGET_ABORT)
    }
}

impl FromBits<u32> for Status {
    const BITS: u32 = 16;
    type Error = core::convert::Infallible;

    fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
        Ok(Self::from_bits((bits >> 16) as u16))
    }

    fn into_bits(self) -> u32 {
        (self.bits() as u32) << 16
    }
}

// === impl Command ===

impl FromBits<u32> for Command {
    const BITS: u32 = 16;
    type Error = core::convert::Infallible;

    fn try_from_bits(bits: u32) -> Result<Self, Self::Error> {
        Ok(Self::from_bits(bits as u16))
    }

    fn into_bits(self) -> u32 {
        self.bits() as u32
    }
}

// === impl DevselTiming ===

impl FromBits<u16> for DevselTiming {
    const BITS: u32 = 2;
    type Error = crate::error::UnexpectedValue<u16>;

    fn try_from_bits(bits: u16) -> Result<Self, Self::Error> {
        match bits as u8 {
            bits if bits == Self::Fast as u8 => Ok(Self::Fast),
            bits if bits == Self::Medium as u8 => Ok(Self::Medium),
            bits if bits == Self::Slow as u8 => Ok(Self::Slow),
            _ => Err(crate::error::unexpected(bits).named("DEVSEL timing")),
        }
    }

    fn into_bits(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_valid() {
        Status::assert_valid();
    }

    #[test]
    fn command_is_valid() {
        Status::assert_valid();
    }

    #[test]
    fn register_word_is_valid() {
        RegisterWord::assert_valid();
    }
}
