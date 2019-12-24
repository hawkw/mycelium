use crate::error;
use core::convert::TryFrom;

macro_rules! class_enum {
    (pub enum $name:ident<NoProgIf> {
        $(
            // $($m:meta)*
            $variant:ident = $value:expr
        ),+
        $(,)?
    }) => {
        class_enum! {
            pub enum $name {
                $(
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
    (pub enum $name:ident<$kind:ident, $rest:ty> {
        $(
            // $($m:meta)*
            $variant:ident $(($next:ty))? = $value:expr
        ),+
        $(,)?
    }) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub enum $name {
            $(
                $variant $( ($next) )?
            ),+
        }

        impl TryFrom<(u8, $rest)> for $name {
            type Error = error::UnexpectedValue<u8>;
            fn try_from((u, rest): (u8, $rest)) -> Result<Self, Self::Error> {
                match $kind::try_from(u)? {
                    $(
                        $kind::$variant => Ok($name::$variant $((<$next>::try_from(rest)?) )? )
                    ),+
                }
            }
        }

        class_enum!{
            enum $kind {
                // $($m:meta)*
                $(
                    $variant = $value
                ),+
            }
        }
    };
    (pub enum $name:ident {
        $(
            // $($m:meta)*
            $variant:ident = $value:expr
        ),+
        $(,)?
    }) => {

        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub enum $name {
            $(
                // $($m:meta)*
                $variant = $value
            ),+
        }

        class_enum! { @tryfrom $name, $($variant = $value),+ }
    };

    (enum $name:ident {
        $(
            // $($m:meta)*
            $variant:ident = $value:expr
        ),+
        $(,)?
    }) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        #[repr(u8)]
        enum $name {
            $(
                // $($m:meta)*
                $variant = $value
            ),+
        }
        class_enum! { @tryfrom $name, $($variant = $value),+ }
    };
    (@tryfrom $name:ident, $(
        $variant:ident = $value:expr
    ),+ ) => {
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
    }
}

class_enum! {
    pub enum Class<ClassValue, (u8, u8)> {
        Unclassified(Unclassified) = 0x00,
        MassStorage(MassStorage) = 0x01,
        Network = 0x02,
        Display = 0x03,
        Multimedia = 0x04,
        Memory = 0x05,
        Bridge = 0x06,
        SimpleComm = 0x07,
        BaseSystemPeripheral = 0x08,
        Input = 0x09,
        DockingStation = 0x0A,
        Processor = 0x0B,
        SerialBus = 0x0C,
        Wireless = 0x0D,
        Intelligent = 0x0E,
        SatelliteComm = 0x0F,
        Encryption = 0x10,
        SignalProcessing = 0x11,
        ProcessingAccelerator = 0x12,
        NonEssentialInstrumentation = 0x13
    }
}

class_enum! {
    pub enum Unclassified<NoProgIf> {
        NonVga = 0x00,
        Vga = 0x01
    }
}

class_enum! {
    pub enum MassStorage<MassStorageKind, u8> {
        ScsiBus = 0x00,
        Ide(iface::Ide) = 0x01,
        Floppy = 0x02,
        IpiBus = 0x03,
        Raid = 0x04,
        Ata(iface::Ata) = 0x05,
        Sata(iface::Sata) = 0x06,
        SerialAttachedScsi(iface::SerialAttachedScsi) = 0x07,
        NonVolatileMem(iface::Nvm) = 0x08,
        Other = 0x80
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

    class_enum! {
        pub enum Ide {
            IsaOnly = 0x00,
            PciOnly = 0x05,
            IsaWithPci = 0x0A,
            PciWithIsa = 0x0F,
            IsaOnlyBusMastering = 0x80,
            PciOnlyBusMastering = 0x85,
            IsaWithPciBusMastering = 0x8A,
            PciWithIsaBusMastering = 0x8F
        }
    }

    class_enum! {
        pub enum Ata {
            SingleDma = 0x20,
            ChainedDma = 0x30,
        }
    }

    class_enum! {
        pub enum Sata {
            VendorSpecific = 0x00,
            Achi1 = 0x01,
            SerialStorageBus = 0x02,
        }
    }

    class_enum! {
        pub enum SerialAttachedScsi {
            Sas = 0x00,
            SerialStorageBus = 0x02,
        }
    }

    class_enum! {
        pub enum Nvm {
            Nvmhci = 0x01,
            NvmExpress = 0x02,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_parsing() {
        let pci_mass_storage_ide = (0x01, 0x01, 0x05);
        let class = Class::try_from(pci_mass_storage_ide);
        assert_eq!(
            class,
            Ok(Class::MassStorage(MassStorage::Ide(IdeIface::PciOnly))),
        );
    }
}
