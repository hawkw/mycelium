use crate::error;
use core::convert::TryFrom;
use core::fmt;

macro_rules! replace_tt {
    ($old:tt $new:tt) => {
        $new
    };
}

macro_rules! class_enum {
    (
        $(#[$m:meta])*
        $v:vis enum $name:ident<NoProgIf> {
            $(
                $(#[$($mm:tt)*])*
                $variant:ident = $value:expr
            ),+
            $(,)?
        }
    ) => {
        class_enum! {
            $(#[$m])*
            $v enum $name {
                $(
                    $(#[$($mm)*])*
                    $variant = $value
                ),+
            }
        }

        impl TryFrom<(u8, u8)> for $name {
            type Error = error::UnexpectedValue<u8>;
            fn try_from((u, rest): (u8, u8)) -> Result<Self, Self::Error> {
                if rest != 0 {
                    return Err(error::unexpected(rest));
                }

                Self::try_from(u)
            }
        }
    };

    (
        $(#[$m:meta])*
        $v:vis enum $name:ident<$kind:ident, $rest:ty> {
            $(
                $(#[$($mm:tt)*])*
                $variant:ident $(($next:ty))? = $value:expr
            ),+
            $(,)?
        }
    ) => {
        $(#[$m])*
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        $v enum $name {
            $(
                $(#[$($mm)*])*
                $variant $( ($next) )?
            ),+
        }

        impl TryFrom<(u8, $rest)> for $name {
            type Error = error::UnexpectedValue<u8>;
            fn try_from((u, rest): (u8, $rest)) -> Result<Self, Self::Error> {
                match $kind::try_from(u)? {
                    $(
                        $kind::$variant => Ok($name::$variant $((<$next>::try_from(rest)?) )?)
                    ),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                match self {
                    $(
                        $name::$variant $((replace_tt!($next next)))? => {
                            fmt::Display::fmt(&$kind::$variant, f)?;
                            $(next_display_helper(f, replace_tt!($next next))?;)?
                        }
                    ),*
                }
                Ok(())
            }
        }

        class_enum!{
            enum $kind {
                $(
                    $(#[$($mm)*])*
                    $variant = $value
                ),+
            }
        }
    };

    (
        $(#[$m:meta])*
        $v:vis enum $name:ident {
            $(
                #[doc = $doc:expr]
                $(#[$mm:meta])*
                $variant:ident = $value:expr
            ),+
            $(,)?
        }
    ) => {
        $(#[$m])*
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        #[repr(u8)]
        $v enum $name {
            $(
                #[doc = $doc]
                $(#[$mm])*
                $variant = $value
            ),+
        }

        impl TryFrom<u8> for $name {
            type Error = error::UnexpectedValue<u8>;
            fn try_from(num: u8) -> Result<Self, Self::Error> {
                match num {
                    $(
                        $value => Ok($name::$variant),
                    )+
                    num => Err(error::unexpected(num)),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                match self {
                    $(
                        $name::$variant => f.write_str($doc.trim())
                    ),*
                }
            }
        }
    };
}

fn next_display_helper(f: &mut fmt::Formatter, next: &impl fmt::Display) -> fmt::Result {
    // Precision of `0` means we're done, no precision implies infinite precision.
    match f.precision() {
        Some(0) => Ok(()),
        Some(precision) => write!(f, ": {:.*}", precision - 1, next),
        None => write!(f, ": {}", next),
    }
}

class_enum! {
    pub enum Class<ClassValue, (u8, u8)> {
        /// Unclassified
        Unclassified(Unclassified) = 0x00,
        /// Mass Storage Controller
        MassStorage(MassStorage) = 0x01,
        /// Network Controller
        Network(Network) = 0x02,
        /// Display Controller
        Display(Display) = 0x03,
        /// Multimedia Controller
        Multimedia(Multimedia) = 0x04,
        /// Memory Controller
        Memory = 0x05,
        /// Bridge Device
        Bridge = 0x06,
        /// Simple Communication Controller
        SimpleComm = 0x07,
        /// Base System Peripheral
        BaseSystemPeripheral = 0x08,
        /// Input Device Controller
        Input = 0x09,
        /// Docking Station
        DockingStation = 0x0A,
        /// Processor
        Processor = 0x0B,
        /// Serial Bus Controller
        SerialBus = 0x0C,
        /// Wireless Controller
        Wireless = 0x0D,
        /// Intelligent Controller
        Intelligent = 0x0E,
        /// Satellite Communication Controller
        SatelliteComm = 0x0F,
        /// Encryption Controller
        Encryption = 0x10,
        /// Signal Processing Controller
        SignalProcessing = 0x11,
        /// Processing Accelerator
        ProcessingAccelerator = 0x12,
        /// Non-Essential Instrumentation
        NonEssentialInstrumentation = 0x13
    }
}

class_enum! {
    pub enum Unclassified<NoProgIf> {
        /// Non-VGA-Compatible Device
        NonVga = 0x00,
        /// VGA-Compatible Device
        Vga = 0x01
    }
}

class_enum! {
    pub enum MassStorage<MassStorageKind, u8> {
        /// SCSI Bus Controller
        ScsiBus = 0x00,
        /// IDE Controller
        Ide(iface::Ide) = 0x01,
        /// Floppy Disk Controller
        Floppy = 0x02,
        /// IPI Bus Controller
        IpiBus = 0x03,
        /// RAID Controller
        Raid = 0x04,
        /// ATA Controller
        Ata(iface::Ata) = 0x05,
        /// Serial ATA
        Sata(iface::Sata) = 0x06,
        /// Serial Attached SCSI
        SerialAttachedScsi(iface::SerialAttachedScsi) = 0x07,
        /// Non-Volatile Memory Controller
        NonVolatileMem(iface::Nvm) = 0x08,
        /// Other
        Other = 0x80
    }
}

class_enum! {
    pub enum Network<NoProgIf> {
        /// Ethernet Controller
        Ethernet = 0x00,
        /// Token Ring Controller
        TokenRing = 0x01,
        /// FDDI Controller
        Fddi = 0x02,
        /// ATM Controller
        Atm = 0x03,
        /// ISDN Controller
        Isdn = 0x04,
        /// WorldFlip Controller
        WorldFip = 0x05,
        /// PICMG 2.14 Multi Computing
        Picmig2_14 = 0x06,
        /// Infiniband Controller
        Infiniband = 0x07,
        /// Fabric Controller
        Fabric = 0x08,
        /// Other
        Other = 0x80,
    }
}

class_enum! {
    pub enum Display<DisplayValue, u8> {
        /// VGA Compatible Controller
        VgaCompatible(iface::VgaCompatible) = 0x00,
        /// XGA Controller
        Xga = 0x01,
        /// 3D Controller (Not VGA-Compatible)
        ThreeD = 0x02,
        /// Other
        Other = 0x80,
    }
}

class_enum! {
    pub enum Multimedia<NoProgIf> {
        /// Multimedia Video Controller
        MultimediaVideo = 0x00,
        /// Multimedia Audio Controller
        MultimediaAudio = 0x01,
        /// Computer Telephony Device
        ComputerTelephony = 0x02,
        /// Audio Device
        Audio = 0x03,
        /// Other
        Other = 0x80,
    }
}

class_enum! {
    pub enum Memory<NoProgIf> {
        /// RAM Controller
        Ram = 0x00,
        /// Flash Controller
        Flash = 0x01,
        /// Other
        Other = 0x80,
    }
}

impl TryFrom<(u8, u8, u8)> for Class {
    type Error = error::UnexpectedValue<u8>;
    fn try_from((class, subclass, prog_if): (u8, u8, u8)) -> Result<Self, Self::Error> {
        Self::try_from((class, (subclass, prog_if)))
    }
}

pub mod iface {
    use super::*;

    #[derive(Debug, Eq, PartialEq, Copy, Clone)]
    #[repr(transparent)]
    pub struct Ide(u8);

    impl Ide {
        const PCI_NATIVE: u8 = 0b0101;
        const SWITCHABLE: u8 = 0b1010;
        const BUS_MASTERING: u8 = 0x8;

        pub fn supports_bus_mastering(&self) -> bool {
            self.0 & Self::BUS_MASTERING == Self::BUS_MASTERING
        }

        pub fn is_switchable(&self) -> bool {
            self.0 & Self::SWITCHABLE == Self::SWITCHABLE
        }

        pub fn is_isa_native(&self) -> bool {
            !self.is_pci_native()
        }

        pub fn is_pci_native(&self) -> bool {
            self.0 & Self::PCI_NATIVE == Self::PCI_NATIVE
        }
    }

    impl TryFrom<u8> for Ide {
        type Error = error::UnexpectedValue<u8>;
        fn try_from(u: u8) -> Result<Self, Self::Error> {
            if u > 0x8f {
                return Err(error::unexpected(u));
            }
            Ok(Self(u))
        }
    }

    impl fmt::Display for Ide {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let mode = match (self.is_pci_native(), self.is_switchable()) {
                (false, false) => "ISA compatibility mode-only controller",
                (false, true) => {
                    "ISA compatibility mode controller, supports both channels switched to PCI native mode"
                }
                (true, false) => "PCI native mode-only controller",
                (true, true) => {
                    "PCI native mode controller, supports both channels switched to ISA compatibility mode"
                }
            };

            if self.supports_bus_mastering() {
                write!(f, "{}, supports bus mastering", mode)?;
            } else {
                write!(f, "{}", mode)?;
            }
            Ok(())
        }
    }

    class_enum! {
        pub enum Ata {
            /// Single DMA
            SingleDma = 0x20,
            /// Chained DMA
            ChainedDma = 0x30,
        }
    }

    class_enum! {
        pub enum Sata {
            /// Vendor Specific Interface
            VendorSpecific = 0x00,
            /// AHCI 1.0
            Achi1 = 0x01,
            /// Serial Storage Bus
            SerialStorageBus = 0x02,
        }
    }

    class_enum! {
        pub enum SerialAttachedScsi {
            /// SAS
            Sas = 0x00,
            /// Serial Storage Bus
            SerialStorageBus = 0x02,
        }
    }

    class_enum! {
        pub enum Nvm {
            /// NVMHCI
            Nvmhci = 0x01,
            /// NVM Express
            NvmExpress = 0x02,
        }
    }

    class_enum! {
        pub enum VgaCompatible {
            /// VGA Controller
            VgaController = 0x00,
            /// 8514-Compatible Controller
            Compat8514 = 0x01,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parsing() {
        let mass_storage_sata_achi = (0x01, 0x06, 0x01);
        let class = Class::try_from(mass_storage_sata_achi);
        assert_eq!(
            class,
            Ok(Class::MassStorage(MassStorage::Sata(iface::Sata::Achi1))),
        );
    }

    #[test]
    fn test_display() {
        assert_eq!(
            Class::MassStorage(MassStorage::Sata(iface::Sata::Achi1)).to_string(),
            "Mass Storage Controller: Serial ATA: AHCI 1.0"
        );

        let ide_iface = iface::Ide::try_from(0x8F).unwrap();
        assert_eq!(
            Class::MassStorage(MassStorage::Ide(ide_iface)).to_string(),
            "Mass Storage Controller: IDE Controller: PCI native mode controller, supports both channels switched to ISA compatibility mode, supports bus mastering"
        );
        assert_eq!(
            format!("{:.1}", Class::MassStorage(MassStorage::Ide(ide_iface))),
            "Mass Storage Controller: IDE Controller"
        );
    }
}
